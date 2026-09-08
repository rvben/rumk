//! Remove duplicate literal command flags without interpreting shell programs.

use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::eval::Truth;
use crate::logical::Reach;
use crate::parser::Makefile;
use crate::project::Project;
use crate::rules::{Rule, RuleCategory};
use std::collections::BTreeSet;

pub struct RepeatedRecipePrefix;

impl Rule for RepeatedRecipePrefix {
    fn id(&self) -> &'static str {
        "MK218"
    }
    fn name(&self) -> &'static str {
        "Repeated recipe command prefix"
    }
    fn description(&self) -> &'static str {
        "Repeating @, -, or + in a recipe's leading command flags has no additional effect. Keep each flag once. Leaves .ONESHELL projects and uncertain recipe syntax alone."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn fixable(&self) -> bool {
        true
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        if !makefile.includes.is_empty()
            || content.contains(".ONESHELL")
            || makefile
                .rules
                .iter()
                .any(|rule| rule.targets.iter().any(|name| name.contains('$')))
            || makefile.logical.statements().iter().any(|statement| {
                matches!(statement.kind, crate::logical::LogicalKind::Unknown)
                    || statement.text().contains("$(eval")
                    || statement.text().contains("${eval")
            })
        {
            return Vec::new();
        }
        let active: BTreeSet<_> = makefile
            .logical
            .statements()
            .iter()
            .filter(|statement| statement.reach == Reach::Always)
            .map(|statement| statement.start_line)
            .collect();
        prefixes(makefile, content, |line| active.contains(&line))
    }
    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        // With a non-POSIX shell, internal .ONESHELL prefixes are script text.
        // Includes can set this mode for recipes in any file.
        if !super::policy::complete_target_graph(project)
            || project.analysis().targets.contains_key(".ONESHELL")
            || project.files().iter().any(|file| file.makefile.oneshell)
        {
            return Vec::new();
        }
        project
            .files()
            .iter()
            .flat_map(|file| {
                prefixes(&file.makefile, &file.content, |line| {
                    project.evaluation().activity(file.id, line) == Truth::True
                })
                .into_iter()
                .map(|diagnostic| diagnostic.with_source(file.path.clone()))
            })
            .collect()
    }
}

fn prefixes(makefile: &Makefile, content: &str, active: impl Fn(usize) -> bool) -> Vec<Diagnostic> {
    if makefile.oneshell || !makefile.syntax_errors.is_empty() {
        return Vec::new();
    }
    let lines: Vec<_> = content.lines().collect();
    let mut diagnostics = Vec::new();
    for recipe in makefile.rules.iter().flat_map(|rule| &rule.recipes) {
        if !recipe.silent && !recipe.ignore_errors && !recipe.recursive {
            continue;
        }
        if !active(recipe.line) || !makefile.syntax.recipe_prefix_at(recipe.line).known {
            continue;
        }
        let Some(raw) = lines.get(recipe.line - 1) else {
            continue;
        };
        let before: String = raw.chars().take(recipe.column - 1).collect();
        // Never include the recipe indentation itself, even if .RECIPEPREFIX=@.
        let floor = if recipe.inline {
            0
        } else {
            recipe.indentation.len()
        };
        let Some(before) = before.get(floor..) else {
            continue;
        };
        let flags = &before[before.trim_end_matches(['@', '-', '+']).len()..];
        let mut unique = String::new();
        for flag in flags.chars() {
            if !unique.contains(flag) {
                unique.push(flag);
            }
        }
        if flags.len() == unique.len() {
            continue;
        }
        let column = recipe.column - flags.len();
        diagnostics.push(
            Diagnostic::new(
                "MK218",
                Severity::Warning,
                "Repeated recipe command prefix has no additional effect",
                recipe.line,
                column,
            )
            .with_fix(Fix::new("Keep each recipe command prefix once").add_edit(
                Edit::new(recipe.line, column, recipe.line, recipe.column, unique),
            )),
        );
    }
    diagnostics
}
