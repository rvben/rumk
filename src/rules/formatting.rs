//! Conservative layout transformations over exact source ranges.

use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::logical::{find_top_level_assignment, LogicalKind};
use crate::parser::Makefile;
use crate::rules::{Rule, RuleCategory};

pub struct AssignmentSpacing;

impl Rule for AssignmentSpacing {
    fn id(&self) -> &'static str {
        "MK105"
    }
    fn name(&self) -> &'static str {
        "Use spaces around assignment operators"
    }
    fn description(&self) -> &'static str {
        "Use one space around ordinary single-line assignment operators. Preserve the complete value, including trailing whitespace. Dynamic names, continuations, define bodies, and target-specific assignments are left alone."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Style
    }
    fn fixable(&self) -> bool {
        true
    }
    fn layout(&self) -> bool {
        true
    }
    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        makefile
            .logical
            .statements()
            .iter()
            .filter_map(|statement| {
                if statement.kind != LogicalKind::Assignment
                    || statement.start_line != statement.end_line
                {
                    return None;
                }
                let raw = statement.raw(content).trim_end_matches(['\r', '\n']);
                if raw.ends_with('\\') {
                    return None;
                }
                let (position, operator) = find_top_level_assignment(raw)?;
                let left = &raw[..position];
                // Restrict names to ordinary literal words, with optional modifiers.
                // Escapes, expansions, and unusual delimiters can change tokenization.
                if left.is_empty()
                    || !left
                        .chars()
                        .all(|c| c.is_alphanumeric() || "_.- \t".contains(c))
                {
                    return None;
                }
                let name_end = left.trim_end_matches([' ', '\t']).len();
                if name_end == 0 {
                    return None;
                }
                let right = &raw[position + operator.len()..];
                let padding = right.len() - right.trim_start_matches([' ', '\t']).len();
                let end = position + operator.len() + padding;
                // Empty values need no trailing space; comments and nonempty values do.
                let replacement = format!(" {operator}{}", if end == raw.len() { "" } else { " " });
                if raw[name_end..end] == replacement {
                    return None;
                }
                let start_column = raw[..name_end].chars().count() + 1;
                let end_column = raw[..end].chars().count() + 1;
                Some(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        "Use one space around the assignment operator",
                        statement.start_line,
                        start_column,
                    )
                    .with_fix(
                        Fix::new("Normalize assignment operator spacing").add_edit(Edit::new(
                            statement.start_line,
                            start_column,
                            statement.start_line,
                            end_column,
                            replacement,
                        )),
                    ),
                )
            })
            .collect()
    }
}
