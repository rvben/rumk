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

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for rule in &makefile.rules {
            for recipe in &rule.recipes {
                if !recipe.indentation.starts_with('\t') {
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
                            recipe.column,
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
        "Variable names should follow Make conventions and not contain invalid characters."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for variable in makefile.variables.values() {
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

    name.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}
