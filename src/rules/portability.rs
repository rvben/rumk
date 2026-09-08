//! Explicit POSIX edition checks over GNU Make's parsed source structure.
use crate::analysis::ReferenceKind;
use crate::diagnostic::{Diagnostic, Severity};
use crate::logical::{IncludeKind, LogicalKind};
use crate::parser::{AssignmentOperator, Makefile, VariableScope};
use crate::rules::{Rule, RuleCategory};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub enum Edition {
    Posix2017,
    Posix2024,
}
pub struct PosixPortability(pub Edition);
impl Rule for PosixPortability {
    fn id(&self) -> &'static str {
        "MK301"
    }
    fn name(&self) -> &'static str {
        "Construct is outside the selected POSIX Make edition"
    }
    fn description(&self) -> &'static str {
        "Checks GNU constructs against an explicit POSIX.1-2017 or POSIX.1-2024 source-syntax profile. Reports nonportable assignments, directives, rules, special targets, functions, and automatic variables. Does not execute Make or certify recipe shell portability."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check_project(&self, project: &crate::project::Project) -> Vec<Diagnostic> {
        let mut diagnostics: Vec<_> = project
            .files()
            .iter()
            .flat_map(|file| {
                self.check(&file.makefile, &file.content)
                    .into_iter()
                    .map(|d| d.with_source(file.path.clone()))
            })
            .collect();
        let root = project.file(project.root());
        let first = root
            .makefile
            .logical
            .statements()
            .iter()
            .find(|s| !matches!(s.kind, LogicalKind::Blank | LogicalKind::Comment));
        let marker = first.is_some_and(|statement| {
            statement.kind == LogicalKind::Rule
                && root.makefile.rules.iter().any(|rule| {
                    rule.line == statement.start_line
                        && rule.targets == [".POSIX"]
                        && rule.prerequisites.is_empty()
                        && rule.order_only_prerequisites.is_empty()
                        && rule.recipes.is_empty()
                        && !rule.double_colon
                        && !rule.grouped
                        && rule.target_pattern.is_none()
                        && rule.target_assignment.is_none()
                })
        });
        if !marker {
            diagnostics.push(
                Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    "A portable entry Makefile must begin with .POSIX: before other statements",
                    1,
                    1,
                )
                .with_source(root.path.clone()),
            );
        }
        diagnostics
    }
    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let newer = matches!(self.0, Edition::Posix2024);
        let edition = if newer {
            "POSIX.1-2024"
        } else {
            "POSIX.1-2017"
        };
        let mut findings = BTreeSet::new();
        let statements: std::collections::BTreeMap<_, _> = makefile
            .logical
            .statements()
            .iter()
            .map(|statement| (statement.start_line, statement.text()))
            .collect();
        let mut flag = |line, column, feature: String| {
            findings.insert((line, column, feature));
        };
        for assignment in &makefile.assignments {
            if newer && assignment.operator != AssignmentOperator::Recursive {
                if let Some(raw) = statements.get(&assignment.line) {
                    if let Some((position, _)) = crate::logical::find_top_level_assignment(raw) {
                        if position > 0 && !raw[..position].ends_with([' ', '\t']) {
                            flag(
                                assignment.line,
                                assignment.column,
                                "Assignment operator without preceding blank".into(),
                            );
                        }
                    }
                }
            }
            if assignment.operator == AssignmentOperator::Simple
                || (!newer && assignment.operator != AssignmentOperator::Recursive)
            {
                flag(
                    assignment.line,
                    assignment.column,
                    format!("Assignment operator '{}'", assignment.operator.as_str()),
                );
            }
            if assignment.scope != VariableScope::Global {
                flag(
                    assignment.line,
                    assignment.column,
                    "Target-specific assignment".into(),
                );
            }
            let m = &assignment.modifiers;
            if m.export || m.unexport || m.override_ || m.private {
                flag(
                    assignment.line,
                    assignment.column,
                    "GNU assignment modifier".into(),
                );
            }
            if matches!(
                assignment.name.as_str(),
                ".RECIPEPREFIX" | ".DEFAULT_GOAL" | ".SHELLFLAGS" | ".EXTRA_PREREQS"
            ) {
                flag(
                    assignment.line,
                    assignment.column,
                    format!("GNU special variable '{}'", assignment.name),
                );
            }
        }
        for statement in makefile.logical.statements() {
            if statement.kind == LogicalKind::Rule && !newer {
                if let Some(separator) =
                    crate::logical::find_top_level_rule_separator(statement.text())
                {
                    for name in crate::logical::split_top_level_words(
                        &statement.text()[..separator.position],
                    ) {
                        if matches!(name.as_str(), ".PHONY" | ".WAIT" | ".NOTPARALLEL") {
                            flag(statement.start_line, 1, format!("Special target '{name}'"));
                        }
                    }
                }
            }
            // Conditional syntax itself is nonportable, including inactive branches.
            let feature = match statement.kind {
                LogicalKind::Conditional(_) => Some("GNU conditional directive"),
                LogicalKind::Define | LogicalKind::Endef => {
                    Some("GNU multiline variable directive")
                }
                LogicalKind::Directive => Some("GNU directive"),
                LogicalKind::Include(IncludeKind::Optional) if !newer => Some("Optional include"),
                _ => None,
            };
            if let Some(feature) = feature {
                flag(statement.start_line, 1, feature.into());
            }
            if matches!(statement.kind, LogicalKind::Include(_)) {
                let text = statement.text();
                if text
                    .trim_start()
                    .strip_prefix("sinclude")
                    .is_some_and(|rest| rest.starts_with(char::is_whitespace))
                {
                    flag(statement.start_line, 1, "sinclude directive".into());
                }
                if statement.start_line != statement.end_line {
                    flag(statement.start_line, 1, "Continued include line".into());
                }
                if !newer
                    && makefile
                        .includes
                        .iter()
                        .filter(|i| i.line == statement.start_line)
                        .map(|i| i.paths.len())
                        .sum::<usize>()
                        > 1
                {
                    flag(
                        statement.start_line,
                        1,
                        "Multiple include paths on one line".into(),
                    );
                }
            }
        }
        let portable = [
            ".DEFAULT",
            ".IGNORE",
            ".POSIX",
            ".PRECIOUS",
            ".SCCS_GET",
            ".SILENT",
            ".SUFFIXES",
        ];
        for rule in &makefile.rules {
            for target in &rule.targets {
                if !rule.recipes.is_empty()
                    && matches!(
                        target.as_str(),
                        ".IGNORE" | ".POSIX" | ".PRECIOUS" | ".SILENT" | ".SUFFIXES"
                    )
                {
                    flag(
                        rule.line,
                        rule.column,
                        format!("Commands on special target '{target}'"),
                    );
                }
                if !rule.prerequisites.is_empty()
                    && matches!(target.as_str(), ".POSIX" | ".DEFAULT" | ".SCCS_GET")
                {
                    flag(
                        rule.line,
                        rule.column,
                        format!("Prerequisites on special target '{target}'"),
                    );
                }
            }
            if newer
                && rule
                    .targets
                    .iter()
                    .any(|t| matches!(t.as_str(), ".NOTPARALLEL" | ".WAIT"))
                && (!rule.prerequisites.is_empty() || !rule.recipes.is_empty())
            {
                flag(
                    rule.line,
                    rule.column,
                    "Prerequisites or commands on .NOTPARALLEL/.WAIT".into(),
                );
            }
            if rule.double_colon {
                flag(rule.line, rule.column, "Double-colon rule".into());
            }
            if rule.grouped {
                flag(rule.line, rule.column, "Grouped-target rule".into());
            }
            if rule.target_pattern.is_some() {
                flag(rule.line, rule.column, "Static pattern rule".into());
            }
            if rule.targets.iter().any(|name| name.contains('%')) {
                flag(rule.line, rule.column, "Pattern target".into());
            }
            if !rule.order_only_prerequisites.is_empty() {
                flag(rule.line, rule.column, "Order-only prerequisite".into());
            }
            for target in &rule.targets {
                if target.starts_with('.')
                    && target[1..]
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_')
                    && !portable.contains(&target.as_str())
                    && !(newer && [".PHONY", ".NOTPARALLEL", ".WAIT"].contains(&target.as_str()))
                {
                    flag(rule.line, rule.column, format!("Special target '{target}'"));
                }
            }
            if !newer && rule.prerequisites.iter().any(|p| p == ".WAIT") {
                flag(rule.line, rule.column, ".WAIT prerequisite".into());
            }
        }
        let lines: Vec<_> = content.lines().collect();
        for reference in &makefile.analysis().references {
            let loc = reference.location;
            if reference.kind == ReferenceKind::Automatic {
                let name = reference.name.chars().next().unwrap_or(' ');
                if name == '|' || (!newer && matches!(name, '^' | '+')) {
                    flag(
                        loc.line,
                        loc.column,
                        format!("Automatic variable '${}'", reference.name),
                    );
                }
            }
            let Some(row) = lines.get(loc.line.saturating_sub(1)) else {
                continue;
            };
            let rest: String = row.chars().skip(loc.column.saturating_sub(1)).collect();
            if rest.starts_with("$(") || rest.starts_with("${") {
                let body = &rest[2..];
                if reference.kind == ReferenceKind::Function
                    && body
                        .strip_prefix(&reference.name)
                        .is_some_and(|s| s.starts_with(char::is_whitespace))
                {
                    flag(
                        loc.line,
                        loc.column,
                        format!("GNU function '{}'", reference.name),
                    );
                }
                if !newer {
                    if let Some(len) = crate::expansion::reference_length(&rest, 0) {
                        let body = &rest[2..len - 1];
                        if body
                            .split_once(':')
                            .is_some_and(|(_, sub)| sub.contains('%') && sub.contains('='))
                        {
                            flag(loc.line, loc.column, "Pattern macro substitution".into());
                        }
                    }
                }
            }
        }
        findings
            .into_iter()
            .map(|(line, column, feature)| {
                Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!("{feature} is not portable to {edition}"),
                    line,
                    column,
                )
            })
            .collect()
    }
}
