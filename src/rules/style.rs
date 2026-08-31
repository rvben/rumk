use std::collections::BTreeSet;

use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::logical::{
    find_top_level_char, find_top_level_rule_separator, split_top_level_words, LogicalKind,
};
use crate::parser::Makefile;
use crate::rules::{Rule, RuleCategory};

use super::phony::{format_continued_declaration, preferred_line_ending, COMMON_PHONY_TARGETS};

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

    fn fixable(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let phony_fix_isolated = !has_missing_conventional_phony(makefile);
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
                let mut diagnostic = Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!(
                        "Line length {} exceeds maximum of {}",
                        line_length, self.max_length
                    ),
                    line_num + 1,
                    self.max_length + 1,
                );
                if phony_fix_isolated {
                    if let Some(fix) =
                        phony_wrap_fix(makefile, content, line_num + 1, line, self.max_length)
                    {
                        diagnostic = diagnostic.with_fix(fix);
                    }
                }
                diagnostics.push(diagnostic);
            }
        }

        diagnostics
    }
}

fn has_missing_conventional_phony(makefile: &Makefile) -> bool {
    makefile.rules.iter().any(|rule| {
        rule.targets.iter().any(|target| {
            COMMON_PHONY_TARGETS.contains(&target.as_str()) && !makefile.phonies.contains(target)
        })
    })
}

fn phony_wrap_fix(
    makefile: &Makefile,
    content: &str,
    line_number: usize,
    source_line: &str,
    max_length: usize,
) -> Option<Fix> {
    makefile.logical.statements().iter().find(|statement| {
        statement.kind == LogicalKind::Rule
            && statement.start_line == line_number
            && statement.end_line == line_number
    })?;
    if makefile.analysis().is_conditional_line(line_number)
        || source_line != source_line.trim_start()
    {
        return None;
    }

    let separator = find_top_level_rule_separator(source_line)?;
    if separator.position != ".PHONY".len()
        || separator.length != 1
        || separator.grouped
        || separator.double_colon
        || &source_line[..separator.position] != ".PHONY"
    {
        return None;
    }

    let body = &source_line[separator.position + separator.length..];
    if find_top_level_char(body, ';').is_some() || find_top_level_char(body, '|').is_some() {
        return None;
    }
    let comment = find_top_level_char(body, '#');
    let names_source = comment.map_or(body, |index| &body[..index]);
    let names = split_top_level_words(names_source);
    if names.is_empty()
        || names
            .iter()
            .any(|name| name.contains('$') || name.contains('\\'))
    {
        return None;
    }

    let comment_suffix = comment.map_or("", |index| {
        let names_end = body[..index].trim_end().len();
        &body[names_end..]
    });
    let replacement = format_continued_declaration(
        &names,
        comment_suffix,
        preferred_line_ending(content, line_number),
        max_length,
    );
    if replacement == source_line
        || replacement
            .lines()
            .any(|line| line.chars().count() > max_length)
    {
        return None;
    }

    Some(
        Fix::new(format!("Wrap .PHONY declaration to {max_length} columns")).add_edit(Edit::new(
            line_number,
            1,
            line_number,
            source_line.chars().count() + 1,
            replacement,
        )),
    )
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
