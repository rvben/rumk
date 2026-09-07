use crate::analysis::StructuralIssueKind;
use crate::binding::Expansions;
use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::parser::{Makefile, SyntaxErrorKind};
use crate::rules::{PathKind, ReadFailure, Rule, RuleCategory};

pub struct TabInRecipe;

impl Rule for TabInRecipe {
    fn id(&self) -> &'static str {
        "MK001"
    }

    fn name(&self) -> &'static str {
        "Recipe must use tab indentation"
    }

    fn description(&self) -> &'static str {
        "Makefile recipes (commands) must be indented with a tab character, or with the \
         prefix set by .RECIPEPREFIX, not spaces. This is a requirement of the Make syntax."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn fixable(&self) -> bool {
        true
    }

    fn layout(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for rule in &makefile.rules {
            for recipe in &rule.recipes {
                if recipe.inline || !recipe.indentation.starts_with(' ') {
                    continue;
                }
                // The replacement is the prefix Make expects at this line, which
                // an earlier .RECIPEPREFIX assignment can have changed from a tab.
                let prefix = makefile.syntax.recipe_prefix_at(recipe.line);
                if prefix.unevaluated {
                    // A value Rumk could not evaluate can be the space this
                    // line starts with, so Make may read the line as a recipe.
                    continue;
                }
                if !prefix.known || an_include_precedes(makefile, recipe.line) {
                    // The line is indented with a character Make does not read
                    // recipes with, whichever prefix it ends up using, but
                    // writing one Rumk had to guess would break a file Make
                    // accepts, so the line is reported without a fix.
                    diagnostics.push(Diagnostic::new(
                        self.id(),
                        Severity::Error,
                        "Recipe must be indented with the active recipe prefix, not spaces",
                        recipe.line,
                        1,
                    ));
                    continue;
                }
                let character = prefix.character;
                let (message, description) = if character == '\t' {
                    (
                        "Recipe must be indented with tab, not spaces".to_string(),
                        "Replace spaces with tab".to_string(),
                    )
                } else {
                    (
                        format!(
                            "Recipe must be indented with the recipe prefix '{character}', not \
                             spaces"
                        ),
                        format!("Replace spaces with the recipe prefix '{character}'"),
                    )
                };
                let fix = Fix::new(description).add_edit(Edit::new(
                    recipe.line,
                    1,
                    recipe.line,
                    recipe.indentation.len() + 1,
                    character.to_string(),
                ));

                diagnostics.push(
                    Diagnostic::new(self.id(), Severity::Error, message, recipe.line, 1)
                        .with_fix(fix),
                );
            }
        }

        diagnostics
    }
}

/// Whether a file included before `line` could have set the recipe prefix Make
/// reads there. Make keeps reading the including file with the prefix an
/// included file assigned, which Rumk does not follow across files.
fn an_include_precedes(makefile: &Makefile, line: usize) -> bool {
    makefile.includes.iter().any(|include| include.line < line)
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
        "Literal variable names must not contain ':', '#', or '='. Computed variable names \
         are accepted by GNU Make."
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
            .conditional_analysis()
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

pub struct UnreadableFile;

impl Rule for UnreadableFile {
    fn id(&self) -> &'static str {
        "MK007"
    }

    fn name(&self) -> &'static str {
        "Path could not be read"
    }

    fn description(&self) -> &'static str {
        "Rumk reports a path it cannot read instead of stopping the whole run. A file or \
         directory that cannot be opened is an error and nothing in it is checked. A file that \
         is not valid UTF-8 is a warning: Rumk lints it with the invalid bytes replaced but \
         never fixes it."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_read(&self, failure: &ReadFailure) -> Vec<Diagnostic> {
        let diagnostic = match failure {
            ReadFailure::Unreadable { kind, error } => {
                let message = match kind {
                    PathKind::File => format!("File could not be read: {error}"),
                    PathKind::Directory => format!(
                        "Directory could not be read, so any Makefile in it was missed: {error}"
                    ),
                    PathKind::Unknown => format!("Path could not be read: {error}"),
                };
                Diagnostic::new(self.id(), Severity::Error, message, 1, 1)
            }
            ReadFailure::InvalidUtf8 { line, column } => Diagnostic::new(
                self.id(),
                Severity::Warning,
                "File is not valid UTF-8; invalid bytes were replaced before linting and fixes \
                 are disabled",
                *line,
                *column,
            ),
        };
        vec![diagnostic]
    }
}

pub struct InvalidSyntax;

impl Rule for InvalidSyntax {
    fn id(&self) -> &'static str {
        "MK006"
    }

    fn name(&self) -> &'static str {
        "Statement is not valid GNU Make syntax"
    }

    fn description(&self) -> &'static str {
        "GNU Make stops reading a Makefile at a line it cannot parse: a missing separator, \
         a recipe before the first target, an unterminated variable or function reference, \
         an empty variable name, or an unbalanced define or conditional."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut expansions = None;
        makefile
            .syntax_errors
            .iter()
            .filter(|error| match &error.kind {
                // A broken value in a recursively expanded variable only
                // fails once something expands the variable while it still
                // holds that value.
                SyntaxErrorKind::UnterminatedReference {
                    deferred_in: Some(name),
                    ..
                } => expansions
                    .get_or_insert_with(|| Expansions::build(makefile))
                    .expands_value(name, error.line),
                _ => true,
            })
            .map(|error| {
                Diagnostic::new(
                    self.id(),
                    Severity::Error,
                    error.kind.to_string(),
                    error.line,
                    error.column,
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
