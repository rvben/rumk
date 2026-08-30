//! Deterministic semantic indexes spanning a Makefile include graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::{dependency_cycles_for_graph, ReferenceContext, ReferenceKind};
use crate::eval::Truth;
use crate::parser::{AssignmentOperator, VariableScope};
use crate::project::{IncludeResolution, Project, SourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLocation {
    pub source: SourceId,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectVariableDefinition {
    pub operator: AssignmentOperator,
    pub scope: VariableScope,
    pub location: SourceLocation,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectVariableSymbol {
    pub name: String,
    pub definitions: Vec<ProjectVariableDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTargetDeclaration {
    pub location: SourceLocation,
    pub end_line: usize,
    pub double_colon: bool,
    pub grouped: bool,
    pub target_pattern: Option<String>,
    pub has_recipe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDependencyEdge {
    pub prerequisite: String,
    pub order_only: bool,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTargetSymbol {
    pub name: String,
    pub phony: bool,
    pub special: bool,
    pub declarations: Vec<ProjectTargetDeclaration>,
    pub dependencies: Vec<ProjectDependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectReference {
    pub name: String,
    pub kind: ReferenceKind,
    pub context: ReferenceContext,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIncludeReference {
    pub path: String,
    pub optional: bool,
    pub dynamic: bool,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSemanticIndex {
    pub variables: BTreeMap<String, ProjectVariableSymbol>,
    pub targets: BTreeMap<String, ProjectTargetSymbol>,
    pub references: Vec<ProjectReference>,
    pub includes: Vec<ProjectIncludeReference>,
    conditional_ranges: BTreeMap<SourceId, Vec<(usize, usize)>>,
    activity: BTreeMap<(SourceId, usize), Truth>,
    has_structural_issues: bool,
}

impl ProjectSemanticIndex {
    pub fn build(project: &Project) -> Self {
        let mut index = Self::default();
        for file in project.files() {
            let source = file.id;
            let local = file.makefile.analysis();

            for (name, variable) in &local.variables {
                let symbol =
                    index
                        .variables
                        .entry(name.clone())
                        .or_insert_with(|| ProjectVariableSymbol {
                            name: name.clone(),
                            definitions: Vec::new(),
                        });
                symbol
                    .definitions
                    .extend(variable.definitions.iter().map(|definition| {
                        ProjectVariableDefinition {
                            operator: definition.operator,
                            scope: definition.scope.clone(),
                            location: SourceLocation {
                                source,
                                line: definition.location.line,
                                column: definition.location.column,
                            },
                            end_line: definition.end_line,
                        }
                    }));
            }

            for rule in &file.makefile.rules {
                let location = SourceLocation {
                    source,
                    line: rule.line,
                    column: rule.column,
                };
                for evaluated in project.evaluation().rules(source, rule.line) {
                    for name in &evaluated.targets {
                        let symbol = index.targets.entry(name.clone()).or_insert_with(|| {
                            ProjectTargetSymbol {
                                name: name.clone(),
                                phony: false,
                                special: name.starts_with('.'),
                                declarations: Vec::new(),
                                dependencies: Vec::new(),
                            }
                        });
                        symbol.special |= name.starts_with('.');
                        symbol.declarations.push(ProjectTargetDeclaration {
                            location,
                            end_line: rule.end_line,
                            double_colon: rule.double_colon,
                            grouped: rule.grouped,
                            target_pattern: rule.target_pattern.clone(),
                            has_recipe: !rule.recipes.is_empty(),
                        });
                        symbol
                            .dependencies
                            .extend(evaluated.prerequisites.iter().map(|prerequisite| {
                                ProjectDependencyEdge {
                                    prerequisite: prerequisite.clone(),
                                    order_only: false,
                                    location,
                                }
                            }));
                        symbol
                            .dependencies
                            .extend(evaluated.order_only_prerequisites.iter().map(
                                |prerequisite| ProjectDependencyEdge {
                                    prerequisite: prerequisite.clone(),
                                    order_only: true,
                                    location,
                                },
                            ));
                    }
                }
            }

            index
                .references
                .extend(local.references.iter().map(|reference| ProjectReference {
                    name: reference.name.clone(),
                    kind: reference.kind,
                    context: reference.context,
                    location: SourceLocation {
                        source,
                        line: reference.location.line,
                        column: reference.location.column,
                    },
                }));
            index.includes.extend(
                local
                    .includes
                    .iter()
                    .map(|include| ProjectIncludeReference {
                        path: include.path.clone(),
                        optional: include.optional,
                        dynamic: include.dynamic,
                        location: SourceLocation {
                            source,
                            line: include.location.line,
                            column: include.location.column,
                        },
                    }),
            );

            index.has_structural_issues |= !local.structural_issues.is_empty();
            let ranges = index.conditional_ranges.entry(source).or_default();
            for block in &local.conditional_blocks {
                ranges.push((block.start_line, block.end_line));
            }
            for statement in file.makefile.logical.statements() {
                index.activity.insert(
                    (source, statement.start_line),
                    project.evaluation().activity(source, statement.start_line),
                );
            }
        }
        for phony in project.evaluation().active_phonies() {
            index
                .targets
                .entry(phony.clone())
                .or_insert_with(|| ProjectTargetSymbol {
                    name: phony.clone(),
                    phony: true,
                    special: phony.starts_with('.'),
                    declarations: Vec::new(),
                    dependencies: Vec::new(),
                })
                .phony = true;
        }
        index.sort_by_evaluation_order(project);
        index
    }

    fn sort_by_evaluation_order(&mut self, project: &Project) {
        let ranks = evaluation_ranks(project);
        let key = |location: SourceLocation| {
            (
                ranks
                    .get(&(location.source, location.line))
                    .copied()
                    .unwrap_or(usize::MAX),
                location.line,
                location.column,
            )
        };
        for variable in self.variables.values_mut() {
            variable
                .definitions
                .sort_by_key(|definition| key(definition.location));
        }
        for target in self.targets.values_mut() {
            target
                .declarations
                .sort_by_key(|declaration| key(declaration.location));
            target
                .dependencies
                .sort_by_key(|dependency| key(dependency.location));
        }
        self.references
            .sort_by_key(|reference| key(reference.location));
        self.includes.sort_by_key(|include| key(include.location));
    }

    pub fn variable(&self, name: &str) -> Option<&ProjectVariableSymbol> {
        self.variables.get(name)
    }

    pub fn target(&self, name: &str) -> Option<&ProjectTargetSymbol> {
        self.targets.get(name)
    }

    pub fn references_to<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a ProjectReference> {
        self.references
            .iter()
            .filter(move |reference| reference.name == name)
    }

    pub fn is_conditional_location(&self, location: SourceLocation) -> bool {
        self.conditional_ranges
            .get(&location.source)
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|(start, end)| *start < location.line && location.line < *end)
            })
    }

    pub fn activity_at(&self, location: SourceLocation) -> Truth {
        self.activity
            .get(&(location.source, location.line))
            .copied()
            .unwrap_or(Truth::Unknown)
    }

    pub fn is_definitely_active(&self, location: SourceLocation) -> bool {
        self.activity_at(location) == Truth::True
    }

    pub fn has_structural_issues(&self) -> bool {
        self.has_structural_issues
    }

    /// Returns strongly connected components in the concrete, unconditional
    /// target graph assembled from every statically included file.
    pub fn dependency_cycles(&self) -> Vec<Vec<String>> {
        if self.has_structural_issues {
            return Vec::new();
        }
        let graph: BTreeMap<String, Vec<String>> = self
            .targets
            .iter()
            .filter(|(name, _)| is_static_name(name))
            .map(|(name, symbol)| {
                let dependencies = symbol
                    .dependencies
                    .iter()
                    .filter(|edge| self.is_definitely_active(edge.location))
                    .map(|edge| &edge.prerequisite)
                    .filter(|prerequisite| {
                        is_static_name(prerequisite) && self.targets.contains_key(*prerequisite)
                    })
                    .cloned()
                    .collect();
                (name.clone(), dependencies)
            })
            .collect();
        dependency_cycles_for_graph(&graph)
    }
}

fn is_static_name(name: &str) -> bool {
    !name.contains('$') && !name.contains('%')
}

fn evaluation_ranks(project: &Project) -> BTreeMap<(SourceId, usize), usize> {
    fn visit(
        project: &Project,
        source: SourceId,
        emitted: &mut BTreeSet<SourceId>,
        active: &mut BTreeSet<SourceId>,
        ranks: &mut BTreeMap<(SourceId, usize), usize>,
        next_rank: &mut usize,
    ) {
        if !emitted.insert(source) || !active.insert(source) {
            return;
        }
        let file = project.file(source);
        for statement in file.makefile.logical.statements() {
            ranks.insert((source, statement.start_line), *next_rank);
            *next_rank += 1;
            for included in project
                .edges()
                .iter()
                .filter(|edge| edge.from == source && edge.line == statement.start_line)
                .filter_map(|edge| match edge.resolution {
                    IncludeResolution::Resolved(included) => Some(included),
                    _ => None,
                })
            {
                if !active.contains(&included) {
                    visit(project, included, emitted, active, ranks, next_rank);
                }
            }
        }
        active.remove(&source);
    }

    let mut ranks = BTreeMap::new();
    visit(
        project,
        project.root(),
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut ranks,
        &mut 0,
    );
    ranks
}
