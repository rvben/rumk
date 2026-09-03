//! The lint pipeline: every rule a configuration enables, run over a Makefile's
//! text, and the fixes those rules offer applied until the text stops changing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::diagnostic::{Applicability, Diagnostic};
use crate::project::Project;
use crate::{fix, inline_config, parser};

/// How many fix passes a text gets before Rumk stops waiting for it to settle.
const MAX_FIX_ITERATIONS: usize = 10;

/// What a text is judged by: the rules a configuration enables, the path they
/// are applied for, and which of them this pass is responsible for.
#[derive(Clone, Copy)]
pub struct LintContext<'a> {
    /// The enabled rules, their options, and the paths each is ignored for.
    pub config: &'a Config,
    /// The path the text was read from, which decides the per-path ignores and
    /// roots the include graph a project pass walks.
    pub path: &'a Path,
    /// Whether the project-aware rules run over the include graph rooted at
    /// this file.
    pub project_root: bool,
    /// Whether a project pass covers this file, in which case the project-aware
    /// rules are left to that pass rather than run over the file on its own.
    pub contextual: bool,
    /// The files the run reports on itself, which a project pass over an
    /// including file does not report on their behalf.
    pub covered_files: &'a BTreeSet<PathBuf>,
}

/// A text after every fix its diagnostics offered has been applied.
pub struct Fixed {
    /// The text as it stands once no fix changes it any more.
    pub content: String,
    /// What the rules report about that text.
    pub diagnostics: Vec<Diagnostic>,
    /// The diagnostics whose fixes were applied to reach it.
    pub applied: Vec<Diagnostic>,
}

/// Everything the enabled rules report about `content`, in source order.
pub fn lint(content: &str, context: &LintContext<'_>) -> Result<Vec<Diagnostic>> {
    let LintContext {
        config,
        path,
        project_root,
        contextual,
        covered_files,
    } = *context;
    let makefile = parser::parse(content);
    let mut diagnostics = config
        .rules
        .iter()
        .filter(|rule| !contextual || !rule.project_aware())
        .flat_map(|rule| rule.check(&makefile, content))
        .filter(|diagnostic| !config.is_rule_ignored_for_path(path, &diagnostic.rule_id))
        .map(|mut diagnostic| {
            resolve_fix(config, &mut diagnostic);
            diagnostic
        })
        .collect::<Vec<_>>();
    diagnostics = inline_config::apply_inline_suppressions(content, diagnostics)
        .map_err(anyhow::Error::msg)?;
    if project_root {
        let project = Project::load_with_root_content(
            path,
            content.to_string(),
            &config.project_options(path),
        )?;
        let mut project_diagnostics = config
            .rules
            .iter()
            .flat_map(|rule| rule.check_project(&project))
            .map(|mut diagnostic| {
                if diagnostic.source.is_none() {
                    diagnostic.source = Some(project.file(project.root()).path.clone());
                }
                resolve_fix(config, &mut diagnostic);
                diagnostic
            })
            .collect::<Vec<_>>();
        for file in project.files().iter().filter(|file| {
            file.id != project.root()
                && !covered_files.contains(&file.path)
                && !config.is_path_ignored(&file.path)
        }) {
            project_diagnostics.extend(
                config
                    .rules
                    .iter()
                    .filter(|rule| !rule.project_aware())
                    .flat_map(|rule| rule.check(&file.makefile, &file.content))
                    .filter(|diagnostic| {
                        !config.is_rule_ignored_for_path(&file.path, &diagnostic.rule_id)
                    })
                    .map(|mut diagnostic| {
                        diagnostic.source = Some(file.path.clone());
                        diagnostic.fixable = false;
                        diagnostic.fix = None;
                        diagnostic
                    }),
            );
        }
        for file in project.files() {
            let source_diagnostics = project_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.source.as_deref() == Some(file.path.as_path()))
                .filter(|diagnostic| {
                    !config.is_rule_ignored_for_path(&file.path, &diagnostic.rule_id)
                })
                .cloned()
                .collect();
            diagnostics.extend(
                inline_config::apply_inline_suppressions(&file.content, source_diagnostics)
                    .map_err(anyhow::Error::msg)?,
            );
        }
    }
    sort_diagnostics(&mut diagnostics);
    Ok(diagnostics)
}

/// Decides whether this run applies the fix a rule offered.
///
/// A rule the configuration marks unfixable loses its fix outright: nothing is
/// to be reported about a fix that will never be applied. An unsafe fix a run
/// did not ask for is kept and only withheld, so `rumk check` can still say the
/// fix exists and `--unsafe-fixes` can be offered for it.
fn resolve_fix(config: &Config, diagnostic: &mut Diagnostic) {
    if !config.is_rule_fixable(&diagnostic.rule_id) {
        diagnostic.fixable = false;
        diagnostic.fix = None;
        return;
    }
    if !config.unsafe_fixes()
        && diagnostic
            .fix
            .as_ref()
            .is_some_and(|fix| fix.applicability == Applicability::Unsafe)
    {
        diagnostic.fixable = false;
    }
}

/// Applies the fixes the rules offer, re-linting after each pass until no fix
/// changes the text any more. `diagnostics` are the ones [`lint`] reported for
/// `content`.
///
/// Two rules can each undo what the other did, and a fix can uncover work for
/// the pass after it, so the loop stops on a text it has already produced and
/// on a pass count, rather than trusting the rules to agree.
pub fn fix(
    content: &str,
    diagnostics: Vec<Diagnostic>,
    context: &LintContext<'_>,
) -> Result<Fixed> {
    let mut current = Fixed {
        content: content.to_string(),
        diagnostics,
        applied: Vec::new(),
    };
    let mut seen = BTreeSet::from([current.content.clone()]);
    for iteration in 0..MAX_FIX_ITERATIONS {
        let applied = fix::apply_fixes(&current.content, &current.diagnostics);
        if applied.content == current.content {
            break;
        }
        if !seen.insert(applied.content.clone()) {
            bail!(
                "Fix cycle detected while formatting {}",
                context.path.display()
            );
        }
        // Only what this pass wrote is reported as fixed. A fix it left out for
        // overlapping one it applied is offered again by the next pass, and
        // counting it here as well would report it twice.
        current.applied.extend(
            applied
                .fixed
                .iter()
                .map(|index| current.diagnostics[*index].clone()),
        );
        current.content = applied.content;
        current.diagnostics = lint(&current.content, context).with_context(|| {
            format!(
                "Failed to parse formatted Makefile: {}",
                context.path.display()
            )
        })?;
        if iteration + 1 == MAX_FIX_ITERATIONS
            && current
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.fixable)
        {
            bail!(
                "Fixes did not stabilize after {MAX_FIX_ITERATIONS} iterations for {}",
                context.path.display()
            );
        }
    }
    Ok(current)
}

/// Orders diagnostics the way they are reported: by the file they belong to,
/// then their place in it, then the rule that found them.
pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.source.clone(),
            diagnostic.line,
            diagnostic.column,
            diagnostic.rule_id.clone(),
        )
    });
}
