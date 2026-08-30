use crate::analysis::StructuralIssueKind;
use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::parser::Makefile;
use crate::rules::{Rule, RuleCategory};

pub struct TabInRecipe;

impl Rule for TabInRecipe {
    fn id(&self) -> &'static str {
        "MK001"
    }

    fn name(&self) -> &'static str {
        "Recipe must use tab indentation"
    }

    fn description(&self) -> &'static str {
        "Makefile recipes (commands) must be indented with a tab character, not spaces. \
         This is a requirement of the Make syntax."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn fixable(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for rule in &makefile.rules {
            for recipe in &rule.recipes {
                if !recipe.inline && recipe.indentation.starts_with(' ') {
                    let fix = Fix::new("Replace spaces with tab").add_edit(Edit::new(
                        recipe.line,
                        1,
                        recipe.line,
                        recipe.indentation.len() + 1,
                        "\t".to_string(),
                    ));

                    diagnostics.push(
                        Diagnostic::new(
                            self.id(),
                            Severity::Error,
                            "Recipe must be indented with tab, not spaces",
                            recipe.line,
                            1,
                        )
                        .with_fix(fix),
                    );
                }
            }
        }

        diagnostics
    }
}

pub struct InvalidVariableSyntax;

impl Rule for InvalidVariableSyntax {
    fn id(&self) -> &'static str {
        "MK002"
    }

    fn name(&self) -> &'static str {
        "Invalid variable syntax"
    }

    fn description(&self) -> &'static str {
        "Literal variable names must not contain ':', '#', or '='. Internal whitespace and \
         computed variable names are accepted by GNU Make."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for variable in &makefile.assignments {
            if !is_valid_variable_name(&variable.name) {
                diagnostics.push(Diagnostic::new(
                    self.id(),
                    Severity::Error,
                    format!("Invalid variable name: '{}'", variable.name),
                    variable.line,
                    variable.column,
                ));
            }
        }

        diagnostics
    }
}

fn is_valid_variable_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    if name.contains('$') {
        return true;
    }

    name.chars()
        .all(|character| !matches!(character, ':' | '#' | '='))
}

pub struct ConditionalStructure;

impl Rule for ConditionalStructure {
    fn id(&self) -> &'static str {
        "MK003"
    }

    fn name(&self) -> &'static str {
        "Malformed conditional structure"
    }

    fn description(&self) -> &'static str {
        "Make conditionals must have balanced if/endif directives and at most one else branch."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        makefile
            .analysis()
            .structural_issues
            .iter()
            .map(|issue| {
                let message = match issue.kind {
                    StructuralIssueKind::UnexpectedElse => "Unexpected else without a matching if",
                    StructuralIssueKind::DuplicateElse => {
                        "Conditional block contains more than one else"
                    }
                    StructuralIssueKind::UnexpectedEndif => {
                        "Unexpected endif without a matching if"
                    }
                    StructuralIssueKind::UnterminatedConditional => {
                        "Conditional block is missing an endif"
                    }
                };
                Diagnostic::new(
                    self.id(),
                    Severity::Error,
                    message,
                    issue.location.line,
                    issue.location.column,
                )
            })
            .collect()
    }
}

pub struct SpecialTargetPlacement;

impl Rule for SpecialTargetPlacement {
    fn id(&self) -> &'static str {
        "MK005"
    }

    fn name(&self) -> &'static str {
        "Special target must stand alone"
    }

    fn description(&self) -> &'static str {
        "GNU Make special targets should be the sole target on the left-hand side; combining one with ordinary targets changes the declaration's meaning."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        makefile
            .rules
            .iter()
            .filter(|rule| rule.targets.len() > 1)
            .flat_map(|rule| {
                rule.targets
                    .iter()
                    .filter(|target| SPECIAL_TARGETS.contains(&target.as_str()))
                    .map(|target| {
                        Diagnostic::new(
                            self.id(),
                            Severity::Error,
                            format!("Special target '{target}' must be declared by itself"),
                            rule.line,
                            rule.column,
                        )
                    })
            })
            .collect()
    }
}

const SPECIAL_TARGETS: &[&str] = &[
    ".DEFAULT",
    ".DELETE_ON_ERROR",
    ".EXPORT_ALL_VARIABLES",
    ".IGNORE",
    ".INTERMEDIATE",
    ".LOW_RESOLUTION_TIME",
    ".NOTINTERMEDIATE",
    ".NOTPARALLEL",
    ".ONESHELL",
    ".PHONY",
    ".POSIX",
    ".PRECIOUS",
    ".SECONDARY",
    ".SECONDEXPANSION",
    ".SILENT",
    ".SUFFIXES",
    ".WAIT",
];
