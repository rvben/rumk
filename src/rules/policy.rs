//! Optional project conventions and checks for suppressed build failures.

use std::collections::BTreeSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::eval::Truth;
use crate::logical::{LogicalKind, Reach};
use crate::parser::Makefile;
use crate::project::{IncludeResolution, Project};
use crate::rules::{Rule, RuleCategory};

pub struct GlobalIgnore;

impl Rule for GlobalIgnore {
    fn id(&self) -> &'static str {
        "MK214"
    }
    fn name(&self) -> &'static str {
        "Global .IGNORE hides recipe failures"
    }
    fn description(&self) -> &'static str {
        "An active .IGNORE declaration without prerequisites suppresses recipe failures throughout the build. Scope it to specific targets or commands."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        project.files().iter().flat_map(|file| {
            file.makefile.rules.iter().filter_map(|rule| {
                let global = project.evaluation().rules(file.id, rule.line).iter().any(|evaluated| {
                    evaluated.targets.iter().any(|target| target == ".IGNORE")
                        && evaluated.prerequisites.is_empty()
                        && evaluated.order_only_prerequisites.is_empty()
                });
                global.then(|| Diagnostic::new(self.id(), Severity::Warning,
                    "Global .IGNORE suppresses recipe failures; scope it to specific targets or commands",
                    rule.line, rule.column).with_source(file.path.clone()))
            })
        }).collect()
    }
}

pub struct RequiredTargets {
    required: BTreeSet<String>,
}

impl Default for RequiredTargets {
    fn default() -> Self {
        Self::new(["all", "clean", "test"].map(str::to_string))
    }
}

impl RequiredTargets {
    pub fn new(required: impl IntoIterator<Item = String>) -> Self {
        Self {
            required: required.into_iter().collect(),
        }
    }
}

impl Rule for RequiredTargets {
    fn id(&self) -> &'static str {
        "MK215"
    }
    fn name(&self) -> &'static str {
        "Required project target is missing or not phony"
    }
    fn description(&self) -> &'static str {
        "Configured entry targets must have explicit declarations and be .PHONY across the include graph. Incomplete or uncertain graphs are left alone."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        if self.required.is_empty() || !complete_target_graph(project) {
            return Vec::new();
        }
        let root = project.file(project.root());
        self.required
            .iter()
            .filter_map(|name| {
                let symbol = project.analysis().targets.get(name);
                // A target-specific variable assignment names a target but does
                // not declare its rule. It cannot satisfy the project interface.
                let declaration = symbol.and_then(|target| {
                    target.declarations.iter().find(|declaration| {
                        project
                            .file(declaration.location.source)
                            .makefile
                            .rules
                            .iter()
                            .any(|rule| {
                                rule.line == declaration.location.line
                                    && rule.target_assignment.is_none()
                            })
                    })
                });
                let (message, source, line, column) = match declaration {
                    None => (
                        format!(
                            "Required target '{name}' has no explicit declaration in this project"
                        ),
                        root.path.clone(),
                        1,
                        1,
                    ),
                    Some(_) if symbol.is_some_and(|target| target.phony) => return None,
                    Some(declaration) => (
                        format!("Required target '{name}' must be declared .PHONY"),
                        project.file(declaration.location.source).path.clone(),
                        declaration.location.line,
                        declaration.location.column,
                    ),
                };
                Some(
                    Diagnostic::new(self.id(), Severity::Warning, message, line, column)
                        .with_source(source),
                )
            })
            .collect()
    }
}

/// Absence is evidence only when every source of target declarations is known.
/// In particular, an unread include or an eval/call can supply the entire API.
fn complete_target_graph(project: &Project) -> bool {
    if project.analysis().has_structural_issues()
        || !project.cycles().is_empty()
        || project.edges().iter().any(|edge| {
            !matches!(
                edge.resolution,
                IncludeResolution::Resolved(_) | IncludeResolution::Inactive
            )
        })
    {
        return false;
    }
    project.files().iter().all(|file| {
        file.makefile.syntax_errors.is_empty()
            && file.makefile.logical.statements().iter().all(|statement| {
                if matches!(
                    statement.kind,
                    LogicalKind::Recipe | LogicalKind::Comment | LogicalKind::Blank
                ) {
                    return true;
                }
                match project.evaluation().activity(file.id, statement.start_line) {
                    Truth::False => true,
                    Truth::Unknown => false,
                    Truth::True => {
                        !statement.text().contains("$(eval")
                            && !statement.text().contains("${eval")
                            && !matches!(statement.kind, LogicalKind::Unknown)
                    }
                }
            })
            && file.makefile.rules.iter().all(|rule| {
                project.evaluation().activity(file.id, rule.line) == Truth::False
                    || rule.target_assignment.is_some()
                    || !project.evaluation().rules(file.id, rule.line).is_empty()
            })
    })
}

pub struct RecipeLength {
    max_lines: usize,
}

impl RecipeLength {
    pub fn new(max_lines: usize) -> Self {
        Self { max_lines }
    }
}

impl Rule for RecipeLength {
    fn id(&self) -> &'static str {
        "MK104"
    }
    fn name(&self) -> &'static str {
        "Recipe exceeds configured length"
    }
    fn description(&self) -> &'static str {
        "Keep recipes within the configured number of logical command lines. Continuations count once; empty lines and shell comments do not count. This is an opt-in maintainability convention."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Style
    }
    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let inactive: BTreeSet<_> = makefile
            .logical
            .statements()
            .iter()
            .filter(|statement| statement.reach == Reach::Never)
            .map(|statement| statement.start_line)
            .collect();
        makefile.rules.iter().filter_map(|rule| {
            let count = rule.recipes.iter().filter(|recipe| {
                !inactive.contains(&recipe.line)
                    && !recipe.command.trim().is_empty() && !recipe.command.trim_start().starts_with('#')
            }).count();
            (count > self.max_lines).then(|| Diagnostic::new(self.id(), Severity::Warning,
                format!("Recipe has {count} logical command lines (maximum {}); consider extracting a script", self.max_lines),
                rule.line, rule.column))
        }).collect()
    }
}
