use std::collections::BTreeSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::logical::LogicalKind;
use crate::parser::Makefile;
use crate::rules::{Rule, RuleCategory};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NamingStyle {
    Upper,
    Lower,
}

pub struct LineLength {
    max_length: usize,
    ignore_comments: bool,
    ignore_recipes: bool,
}

impl LineLength {
    pub fn new(max_length: usize) -> Self {
        Self {
            max_length,
            ignore_comments: true,
            ignore_recipes: true,
        }
    }

    pub fn ignore_comments(mut self, ignore: bool) -> Self {
        self.ignore_comments = ignore;
        self
    }

    pub fn ignore_recipes(mut self, ignore: bool) -> Self {
        self.ignore_recipes = ignore;
        self
    }
}

impl Rule for LineLength {
    fn id(&self) -> &'static str {
        "MK101"
    }

    fn name(&self) -> &'static str {
        "Line exceeds maximum length"
    }

    fn description(&self) -> &'static str {
        "Declarative Makefile lines should not exceed the configured maximum length. Full-line comments and recipes are ignored by default because wrapping them is often noisy or unsafe."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Style
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let ignored_lines: BTreeSet<_> = makefile
            .logical
            .statements()
            .iter()
            .filter(|statement| {
                (self.ignore_comments && statement.kind == LogicalKind::Comment)
                    || (self.ignore_recipes && statement.kind == LogicalKind::Recipe)
            })
            .flat_map(|statement| statement.start_line..=statement.end_line)
            .collect();

        for (line_num, line) in content.lines().enumerate() {
            if ignored_lines.contains(&(line_num + 1)) {
                continue;
            }
            let line_length = line.chars().count();
            if line_length > self.max_length {
                diagnostics.push(Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!(
                        "Line length {} exceeds maximum of {}",
                        line_length, self.max_length
                    ),
                    line_num + 1,
                    self.max_length + 1,
                ));
            }
        }

        diagnostics
    }
}

pub struct VariableNaming {
    style: NamingStyle,
}

impl VariableNaming {
    pub fn new(style: NamingStyle) -> Self {
        Self { style }
    }
}

impl Rule for VariableNaming {
    fn id(&self) -> &'static str {
        "MK102"
    }

    fn name(&self) -> &'static str {
        "Variable naming convention"
    }

    fn description(&self) -> &'static str {
        "Variables should follow the configured naming convention for consistency."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Style
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for variable in &makefile.assignments {
            if !matches_naming_style(&variable.name, self.style) {
                let expected = naming_style_description(self.style);
                diagnostics.push(Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!(
                        "Variable '{}' does not follow {} convention",
                        variable.name, expected
                    ),
                    variable.line,
                    variable.column,
                ));
            }
        }

        diagnostics
    }
}

pub struct TargetNaming {
    style: NamingStyle,
}

impl TargetNaming {
    pub fn new(style: NamingStyle) -> Self {
        Self { style }
    }
}

impl Rule for TargetNaming {
    fn id(&self) -> &'static str {
        "MK103"
    }

    fn name(&self) -> &'static str {
        "Target naming convention"
    }

    fn description(&self) -> &'static str {
        "Targets should follow the configured naming convention for consistency."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Style
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for rule in &makefile.rules {
            for target in &rule.targets {
                if !target.starts_with('.') && !matches_naming_style(target, self.style) {
                    let expected = naming_style_description(self.style);
                    diagnostics.push(Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Target '{target}' does not follow {expected} convention"),
                        rule.line,
                        rule.column,
                    ));
                }
            }
        }

        diagnostics
    }
}

fn matches_naming_style(name: &str, style: NamingStyle) -> bool {
    match style {
        NamingStyle::Upper => name.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()),
        NamingStyle::Lower => name.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()),
    }
}

fn naming_style_description(style: NamingStyle) -> &'static str {
    match style {
        NamingStyle::Upper => "UPPER_CASE",
        NamingStyle::Lower => "lower_case",
    }
}
