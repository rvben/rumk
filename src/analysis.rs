//! Deterministic semantic indexes derived from a parsed Makefile.

use std::collections::{BTreeMap, BTreeSet};

use crate::logical::{find_top_level_char, ConditionalKind, LogicalKind};
use crate::parser::{AssignmentOperator, Makefile, VariableScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDefinition {
    pub operator: AssignmentOperator,
    pub scope: VariableScope,
    pub location: Location,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSymbol {
    pub name: String,
    pub definitions: Vec<VariableDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Variable,
    Automatic,
    Function,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceContext {
    Assignment,
    Rule,
    Recipe,
    Include,
    Conditional,
    Definition,
    Directive,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    pub kind: ReferenceKind,
    pub context: ReferenceContext,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDeclaration {
    pub location: Location,
    pub end_line: usize,
    pub double_colon: bool,
    pub grouped: bool,
    pub target_pattern: Option<String>,
    pub has_recipe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    pub prerequisite: String,
    pub order_only: bool,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSymbol {
    pub name: String,
    pub phony: bool,
    pub special: bool,
    pub declarations: Vec<TargetDeclaration>,
    pub dependencies: Vec<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeReference {
    pub path: String,
    pub optional: bool,
    pub dynamic: bool,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalBlock {
    pub kind: ConditionalKind,
    pub start_line: usize,
    pub branch_lines: Vec<usize>,
    pub else_line: Option<usize>,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralIssueKind {
    UnexpectedElse,
    DuplicateElse,
    UnexpectedEndif,
    UnterminatedConditional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralIssue {
    pub kind: StructuralIssueKind,
    pub location: Location,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    pub variables: BTreeMap<String, VariableSymbol>,
    pub targets: BTreeMap<String, TargetSymbol>,
    pub references: Vec<Reference>,
    pub includes: Vec<IncludeReference>,
    pub conditional_blocks: Vec<ConditionalBlock>,
    pub structural_issues: Vec<StructuralIssue>,
}

impl SemanticIndex {
    pub fn build(makefile: &Makefile) -> Self {
        let mut index = Self::default();
        index.index_variables(makefile);
        index.index_targets(makefile);
        index.index_includes(makefile);
        index.index_conditionals(makefile);
        index.references = extract_references(makefile);
        index
    }

    pub fn variable(&self, name: &str) -> Option<&VariableSymbol> {
        self.variables.get(name)
    }

    pub fn target(&self, name: &str) -> Option<&TargetSymbol> {
        self.targets.get(name)
    }

    pub fn references_to<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Reference> {
        self.references
            .iter()
            .filter(move |reference| reference.name == name)
    }

    /// Returns strongly connected components in the explicit, static target
    /// graph. Dynamic and pattern names are excluded because their concrete
    /// relationships are only known after Make expansion.
    pub fn dependency_cycles(&self) -> Vec<Vec<String>> {
        if !self.structural_issues.is_empty() {
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
                    .filter(|edge| !self.is_conditional_line(edge.location.line))
                    .map(|edge| &edge.prerequisite)
                    .filter(|prerequisite| {
                        is_static_name(prerequisite) && self.targets.contains_key(*prerequisite)
                    })
                    .cloned()
                    .collect();
                (name.clone(), dependencies)
            })
            .collect();
        Tarjan::new(&graph).cycles()
    }

    pub fn is_conditional_line(&self, line: usize) -> bool {
        self.conditional_blocks
            .iter()
            .any(|block| block.start_line < line && line < block.end_line)
    }

    fn index_variables(&mut self, makefile: &Makefile) {
        for variable in &makefile.assignments {
            let symbol = self
                .variables
                .entry(variable.name.clone())
                .or_insert_with(|| VariableSymbol {
                    name: variable.name.clone(),
                    definitions: Vec::new(),
                });
            symbol.definitions.push(VariableDefinition {
                operator: variable.operator,
                scope: variable.scope.clone(),
                location: Location {
                    line: variable.line,
                    column: variable.column,
                },
                end_line: variable.end_line,
            });
        }
    }

    fn index_targets(&mut self, makefile: &Makefile) {
        let phonies: BTreeSet<_> = makefile.phonies.iter().collect();
        for rule in &makefile.rules {
            for target in &rule.targets {
                let symbol = self
                    .targets
                    .entry(target.clone())
                    .or_insert_with(|| TargetSymbol {
                        name: target.clone(),
                        phony: phonies.contains(target),
                        special: target.starts_with('.'),
                        declarations: Vec::new(),
                        dependencies: Vec::new(),
                    });
                symbol.phony |= phonies.contains(target);
                symbol.declarations.push(TargetDeclaration {
                    location: Location {
                        line: rule.line,
                        column: rule.column,
                    },
                    end_line: rule.end_line,
                    double_colon: rule.double_colon,
                    grouped: rule.grouped,
                    target_pattern: rule.target_pattern.clone(),
                    has_recipe: !rule.recipes.is_empty(),
                });
                symbol
                    .dependencies
                    .extend(
                        rule.prerequisites
                            .iter()
                            .map(|prerequisite| DependencyEdge {
                                prerequisite: prerequisite.clone(),
                                order_only: false,
                                location: Location {
                                    line: rule.line,
                                    column: rule.column,
                                },
                            }),
                    );
                symbol
                    .dependencies
                    .extend(rule.order_only_prerequisites.iter().map(|prerequisite| {
                        DependencyEdge {
                            prerequisite: prerequisite.clone(),
                            order_only: true,
                            location: Location {
                                line: rule.line,
                                column: rule.column,
                            },
                        }
                    }));
            }
        }

        for phony in &makefile.phonies {
            self.targets
                .entry(phony.clone())
                .or_insert_with(|| TargetSymbol {
                    name: phony.clone(),
                    phony: true,
                    special: phony.starts_with('.'),
                    declarations: Vec::new(),
                    dependencies: Vec::new(),
                })
                .phony = true;
        }
    }

    fn index_includes(&mut self, makefile: &Makefile) {
        for include in &makefile.includes {
            self.includes
                .extend(include.paths.iter().map(|path| IncludeReference {
                    path: path.clone(),
                    optional: include.optional,
                    dynamic: path.contains('$') || path.contains('%'),
                    location: Location {
                        line: include.line,
                        column: 1,
                    },
                }));
        }
    }

    fn index_conditionals(&mut self, makefile: &Makefile) {
        #[derive(Debug)]
        struct OpenBlock {
            kind: ConditionalKind,
            line: usize,
            branch_lines: Vec<usize>,
            else_line: Option<usize>,
        }

        let mut stack: Vec<OpenBlock> = Vec::new();
        for conditional in &makefile.conditionals {
            match conditional.kind {
                kind @ (ConditionalKind::Ifdef
                | ConditionalKind::Ifndef
                | ConditionalKind::Ifeq
                | ConditionalKind::Ifneq) => stack.push(OpenBlock {
                    kind,
                    line: conditional.line,
                    branch_lines: Vec::new(),
                    else_line: None,
                }),
                ConditionalKind::Else => match stack.last_mut() {
                    Some(block)
                        if block.else_line.is_none()
                            && is_else_if_expression(&conditional.expression) =>
                    {
                        block.branch_lines.push(conditional.line);
                    }
                    Some(block) if block.else_line.is_none() => {
                        block.branch_lines.push(conditional.line);
                        block.else_line = Some(conditional.line);
                    }
                    Some(_) => self.structural_issues.push(StructuralIssue {
                        kind: StructuralIssueKind::DuplicateElse,
                        location: Location {
                            line: conditional.line,
                            column: 1,
                        },
                    }),
                    None => self.structural_issues.push(StructuralIssue {
                        kind: StructuralIssueKind::UnexpectedElse,
                        location: Location {
                            line: conditional.line,
                            column: 1,
                        },
                    }),
                },
                ConditionalKind::Endif => {
                    if let Some(block) = stack.pop() {
                        self.conditional_blocks.push(ConditionalBlock {
                            kind: block.kind,
                            start_line: block.line,
                            branch_lines: block.branch_lines,
                            else_line: block.else_line,
                            end_line: conditional.end_line,
                        });
                    } else {
                        self.structural_issues.push(StructuralIssue {
                            kind: StructuralIssueKind::UnexpectedEndif,
                            location: Location {
                                line: conditional.line,
                                column: 1,
                            },
                        });
                    }
                }
            }
        }

        for block in stack {
            self.structural_issues.push(StructuralIssue {
                kind: StructuralIssueKind::UnterminatedConditional,
                location: Location {
                    line: block.line,
                    column: 1,
                },
            });
        }
        self.conditional_blocks
            .sort_by_key(|block| block.start_line);
        self.structural_issues.sort_by_key(|issue| issue.location);
    }
}

fn is_static_name(name: &str) -> bool {
    !name.contains('$') && !name.contains('%')
}

struct Tarjan<'a> {
    graph: &'a BTreeMap<String, Vec<String>>,
    next_index: usize,
    stack: Vec<String>,
    on_stack: BTreeSet<String>,
    indices: BTreeMap<String, usize>,
    lowlinks: BTreeMap<String, usize>,
    components: Vec<Vec<String>>,
}

impl<'a> Tarjan<'a> {
    fn new(graph: &'a BTreeMap<String, Vec<String>>) -> Self {
        Self {
            graph,
            next_index: 0,
            stack: Vec::new(),
            on_stack: BTreeSet::new(),
            indices: BTreeMap::new(),
            lowlinks: BTreeMap::new(),
            components: Vec::new(),
        }
    }

    fn cycles(mut self) -> Vec<Vec<String>> {
        for node in self.graph.keys() {
            if !self.indices.contains_key(node) {
                self.visit(node.clone());
            }
        }

        let mut cycles: Vec<_> = self
            .components
            .into_iter()
            .filter(|component| {
                component.len() > 1
                    || self
                        .graph
                        .get(&component[0])
                        .is_some_and(|edges| edges.contains(&component[0]))
            })
            .collect();
        for cycle in &mut cycles {
            cycle.sort();
        }
        cycles.sort();
        cycles
    }

    fn visit(&mut self, node: String) {
        let index = self.next_index;
        self.next_index += 1;
        self.indices.insert(node.clone(), index);
        self.lowlinks.insert(node.clone(), index);
        self.stack.push(node.clone());
        self.on_stack.insert(node.clone());

        for neighbour in self.graph.get(&node).cloned().unwrap_or_default() {
            if !self.indices.contains_key(&neighbour) {
                self.visit(neighbour.clone());
                let neighbour_lowlink = self.lowlinks[&neighbour];
                self.lowlinks
                    .entry(node.clone())
                    .and_modify(|lowlink| *lowlink = (*lowlink).min(neighbour_lowlink));
            } else if self.on_stack.contains(&neighbour) {
                let neighbour_index = self.indices[&neighbour];
                self.lowlinks
                    .entry(node.clone())
                    .and_modify(|lowlink| *lowlink = (*lowlink).min(neighbour_index));
            }
        }

        if self.lowlinks[&node] == self.indices[&node] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack.remove(&member);
                component.push(member.clone());
                if member == node {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

fn extract_references(makefile: &Makefile) -> Vec<Reference> {
    let source = makefile.syntax.source();
    let line_index = LineIndex::new(source);
    let mut references = Vec::new();

    for statement in makefile.logical.statements() {
        let context = reference_context(statement.kind);
        if matches!(
            statement.kind,
            LogicalKind::Blank | LogicalKind::Comment | LogicalKind::Endef
        ) {
            continue;
        }
        let raw = statement.raw(source);
        let relevant = if matches!(
            statement.kind,
            LogicalKind::Recipe | LogicalKind::DefineBody
        ) || (statement.kind == LogicalKind::Rule
            && find_top_level_char(raw, ';').is_some_and(|semicolon| {
                find_top_level_char(raw, '#').is_none_or(|hash| semicolon < hash)
            })) {
            raw
        } else if let Some(comment) = find_top_level_char(raw, '#') {
            &raw[..comment]
        } else {
            raw
        };
        scan_references(
            relevant,
            statement.span.start.offset,
            context,
            &line_index,
            &mut references,
        );
    }

    references.sort_by_key(|reference| reference.location);
    references
}

fn is_else_if_expression(expression: &str) -> bool {
    expression
        .split_whitespace()
        .next()
        .is_some_and(|keyword| matches!(keyword, "ifdef" | "ifndef" | "ifeq" | "ifneq"))
}

fn reference_context(kind: LogicalKind) -> ReferenceContext {
    match kind {
        LogicalKind::Assignment => ReferenceContext::Assignment,
        LogicalKind::Rule => ReferenceContext::Rule,
        LogicalKind::Recipe => ReferenceContext::Recipe,
        LogicalKind::Include(_) => ReferenceContext::Include,
        LogicalKind::Conditional(_) => ReferenceContext::Conditional,
        LogicalKind::Define | LogicalKind::DefineBody => ReferenceContext::Definition,
        LogicalKind::Directive => ReferenceContext::Directive,
        _ => ReferenceContext::Other,
    }
}

fn scan_references(
    text: &str,
    base_offset: usize,
    context: ReferenceContext,
    lines: &LineIndex<'_>,
    output: &mut Vec<Reference>,
) {
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character != '$' {
            continue;
        }
        let Some((next_index, next)) = characters.peek().copied() else {
            continue;
        };
        if next == '$' {
            characters.next();
            continue;
        }
        if matches!(next, '(' | '{') {
            characters.next();
            let closing = if next == '(' { ')' } else { '}' };
            let body_start = next_index + next.len_utf8();
            if let Some(end) = matching_delimiter(text, body_start, closing) {
                let body = &text[body_start..end];
                let (name, kind) = classify_reference(body);
                if !name.is_empty() {
                    output.push(Reference {
                        name,
                        kind,
                        context,
                        location: lines.location(base_offset + index),
                    });
                }
                scan_references(body, base_offset + body_start, context, lines, output);
                while characters
                    .peek()
                    .is_some_and(|(position, _)| *position <= end)
                {
                    characters.next();
                }
            }
        } else {
            characters.next();
            output.push(Reference {
                name: next.to_string(),
                kind: if is_automatic_variable(next) {
                    ReferenceKind::Automatic
                } else {
                    ReferenceKind::Variable
                },
                context,
                location: lines.location(base_offset + index),
            });
        }
    }
}

fn matching_delimiter(text: &str, body_start: usize, initial_closing: char) -> Option<usize> {
    let mut closers = vec![initial_closing];
    let mut characters = text[body_start..].char_indices().peekable();
    while let Some((relative, character)) = characters.next() {
        if character == '$' {
            if let Some((_, next)) = characters.peek().copied() {
                if next == '$' {
                    characters.next();
                    continue;
                }
                if matches!(next, '(' | '{') {
                    closers.push(if next == '(' { ')' } else { '}' });
                    characters.next();
                    continue;
                }
            }
        }
        if closers.last().copied() == Some(character) {
            closers.pop();
            if closers.is_empty() {
                return Some(body_start + relative);
            }
        }
    }
    None
}

fn classify_reference(body: &str) -> (String, ReferenceKind) {
    let trimmed = body.trim_start();
    let head = trimmed
        .split(|character: char| character.is_whitespace() || character == ',')
        .next()
        .unwrap_or_default();
    if is_make_function(head) {
        return (head.to_string(), ReferenceKind::Function);
    }
    if trimmed.contains('$') {
        return (trimmed.to_string(), ReferenceKind::Dynamic);
    }
    let name = head.split(':').next().unwrap_or_default().trim();
    let kind = if is_automatic_reference(name) {
        ReferenceKind::Automatic
    } else {
        ReferenceKind::Variable
    };
    (name.to_string(), kind)
}

fn is_automatic_variable(character: char) -> bool {
    matches!(character, '@' | '%' | '<' | '?' | '^' | '+' | '*' | '|')
}

fn is_automatic_reference(name: &str) -> bool {
    let mut characters = name.chars();
    is_automatic_variable(characters.next().unwrap_or_default())
        && characters
            .next()
            .is_none_or(|suffix| matches!(suffix, 'D' | 'F') && characters.next().is_none())
}

fn is_make_function(name: &str) -> bool {
    matches!(
        name,
        "subst"
            | "patsubst"
            | "strip"
            | "findstring"
            | "filter"
            | "filter-out"
            | "sort"
            | "word"
            | "wordlist"
            | "words"
            | "firstword"
            | "lastword"
            | "dir"
            | "notdir"
            | "suffix"
            | "basename"
            | "addsuffix"
            | "addprefix"
            | "join"
            | "wildcard"
            | "realpath"
            | "abspath"
            | "if"
            | "or"
            | "and"
            | "intcmp"
            | "foreach"
            | "let"
            | "file"
            | "call"
            | "value"
            | "eval"
            | "origin"
            | "flavor"
            | "shell"
            | "error"
            | "warning"
            | "info"
            | "guile"
    )
}

struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self { source, starts }
    }

    fn location(&self, offset: usize) -> Location {
        let line_index = self.starts.partition_point(|start| *start <= offset) - 1;
        Location {
            line: line_index + 1,
            column: self.source[self.starts[line_index]..offset].chars().count() + 1,
        }
    }
}
