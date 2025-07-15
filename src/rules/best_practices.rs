use crate::diagnostic::{Diagnostic, Severity, Fix, Edit};
use crate::parser::Makefile;
use crate::rules::{Rule, RuleCategory};
use std::path::Path;

pub struct MissingPhony;

impl Rule for MissingPhony {
    fn id(&self) -> &'static str {
        "MK201"
    }
    
    fn name(&self) -> &'static str {
        "Non-file targets should be .PHONY"
    }
    
    fn description(&self) -> &'static str {
        "Targets that don't represent actual files should be declared as .PHONY to ensure \
         they always run and to improve performance."
    }
    
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    
    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let common_phony_targets = ["all", "clean", "test", "check", "install", "build", "help"];
        
        for rule in &makefile.rules {
            for target in &rule.targets {
                if common_phony_targets.contains(&target.as_str()) && !makefile.phonies.contains(target) {
                    diagnostics.push(
                        Diagnostic::new(
                            self.id(),
                            Severity::Warning,
                            format!("Target '{}' should be declared .PHONY", target),
                            rule.line,
                            rule.column,
                        )
                    );
                }
            }
        }
        
        diagnostics
    }
}

pub struct HardcodedPath;

impl Rule for HardcodedPath {
    fn id(&self) -> &'static str {
        "MK202"
    }
    
    fn name(&self) -> &'static str {
        "Avoid hardcoded absolute paths"
    }
    
    fn description(&self) -> &'static str {
        "Hardcoded absolute paths reduce portability and make the Makefile less flexible. \
         Use variables or relative paths instead."
    }
    
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    
    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        
        for variable in makefile.variables.values() {
            if contains_absolute_path(&variable.value) {
                diagnostics.push(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!(
                            "Variable '{}' contains hardcoded absolute path",
                            variable.name
                        ),
                        variable.line,
                        variable.column,
                    )
                );
            }
        }
        
        for rule in &makefile.rules {
            for recipe in &rule.recipes {
                if contains_absolute_path(&recipe.command) {
                    diagnostics.push(
                        Diagnostic::new(
                            self.id(),
                            Severity::Warning,
                            "Recipe contains hardcoded absolute path",
                            recipe.line,
                            recipe.column,
                        )
                    );
                }
            }
        }
        
        diagnostics
    }
}

fn contains_absolute_path(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        (word.starts_with('/') && word.len() > 1 && !word.starts_with("//"))
            || (word.len() > 2 && word.chars().nth(1) == Some(':') && word.chars().nth(2) == Some('\\'))
    })
}