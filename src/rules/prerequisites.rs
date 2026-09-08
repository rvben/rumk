//! A deliberately incomplete proof of missing ordinary prerequisites.
//! Unknown build mechanisms cost coverage, never an invented error.
mod inputs;
use inputs::InputIndex;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, Severity};
use crate::eval::{pattern_stem, Truth};
use crate::logical::LogicalKind;
use crate::parser::Makefile;
use crate::project::{IncludeResolution, Project};
use crate::rules::{Rule, RuleCategory};

pub struct MissingPrerequisite;

impl Rule for MissingPrerequisite {
    fn id(&self) -> &'static str {
        "MK216"
    }
    fn name(&self) -> &'static str {
        "Static prerequisite has no visible file or usable build rule"
    }
    fn description(&self) -> &'static str {
        "Reports absent literal prerequisites in complete static projects when no declared or plausible implicit producer is visible. Opt-in; uncertain graphs are left alone."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check(&self, _: &Makefile, _: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        analyze(project, None)
    }
}

/// Read-only MK216 coverage, independent of whether the rule is enabled.
/// Edges are the evaluated semantic inventory, not every possible runtime edge.
#[derive(Debug, Default, serde::Serialize)]
pub struct Coverage {
    pub root_blockers: BTreeMap<&'static str, usize>,
    pub outcomes: BTreeMap<&'static str, usize>,
    pub edges: Vec<CoverageEdge>,
}

#[derive(Debug, serde::Serialize)]
pub struct CoverageEdge {
    pub target: String,
    pub prerequisite: String,
    pub source: PathBuf,
    pub line: usize,
    pub order_only: bool,
    pub outcome: &'static str,
}

/// Uses exactly the same decisions as the diagnostic rule; never executes Make.
pub fn coverage(project: &Project) -> Coverage {
    let mut coverage = Coverage::default();
    analyze(project, Some(&mut coverage));
    coverage
}

fn analyze(project: &Project, mut coverage: Option<&mut Coverage>) -> Vec<Diagnostic> {
    let index = project.analysis();
    let mut blockers = root_blockers(project);
    let (selective, uncertain_selective) = selective_search(project);
    if uncertain_selective {
        *blockers.entry("unresolved_selective_vpath").or_default() += 1;
    }
    let mut directories = vec![project.working_directory().join(".")];
    match project.evaluation().expand("$(VPATH)").value {
        Some(value) => {
            if value.contains(['\\', ';'])
                || value.contains(":/")
                || (cfg!(windows) && value.contains(':'))
            {
                *blockers.entry("unsupported_vpath").or_default() += 1;
            } else {
                directories.extend(
                    value
                        .split(|c: char| c == ':' || c.is_whitespace())
                        .filter(|part| !part.is_empty())
                        .map(|part| project.working_directory().join(part)),
                );
            }
        }
        None if index.variables.contains_key("VPATH") => {
            *blockers.entry("unresolved_vpath").or_default() += 1;
        }
        None => {}
    }
    if coverage.is_none() && !blockers.is_empty() {
        return Vec::new();
    }
    // Implicit producers may consume another suffix. Include all selective
    // directories for that conservative proof, never just the output's matches.
    let implicit_directories: Vec<_> = directories
        .iter()
        .cloned()
        .chain(
            selective
                .iter()
                .flat_map(|(_, paths)| paths.iter().cloned()),
        )
        .collect();
    let inputs = InputIndex::new(project);
    let mut seen = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for target in index.targets.values() {
        for edge in &target.dependencies {
            let name = normalized(&edge.prerequisite);
            let outcome = if target.special {
                "special_target"
            } else if target.name.contains('%') {
                "pattern_declaration"
            } else if !blockers.is_empty() {
                "root_excluded"
            } else if !index.is_definitely_active(edge.location) {
                "inactive_or_unknown_edge"
            } else if !literal(name) {
                "unsupported_name"
            } else if inputs.declared(name) {
                "declared_target"
            } else if directories
                .iter()
                .chain(
                    selective
                        .iter()
                        .filter(|(pattern, _)| {
                            pattern == name || pattern_stem(pattern, name).is_some()
                        })
                        .flat_map(|(_, paths)| paths.iter()),
                )
                .any(|directory| may_exist(&directory.join(name)))
            {
                "file_or_io_uncertainty"
            } else if inputs.plausible(name, &implicit_directories) {
                "possible_builtin_input"
            } else if possible_pattern_producer(project, name, &implicit_directories, &inputs) {
                "possible_pattern_producer"
            } else if !seen.insert((edge.location, name.to_string())) {
                "duplicate_finding"
            } else {
                "missing"
            };
            if let Some(report) = coverage.as_deref_mut() {
                *report.outcomes.entry(outcome).or_default() += 1;
                report.edges.push(CoverageEdge {
                    target: target.name.clone(),
                    prerequisite: name.into(),
                    source: project.file(edge.location.source).path.clone(),
                    line: edge.location.line,
                    order_only: edge.order_only,
                    outcome,
                });
            }
            if outcome == "missing" {
                diagnostics.push(Diagnostic::new("MK216", Severity::Warning,
                    format!("Prerequisite '{name}' of target '{}' was not found and has no usable visible build rule", target.name),
                    edge.location.line, edge.location.column)
                    .with_source(project.file(edge.location.source).path.clone()));
            }
        }
    }
    if let Some(report) = coverage {
        report.root_blockers = blockers;
    }
    diagnostics
}

// Directives are replayed after evaluation, but their values were captured at
// read time: later variable reassignments must not change an earlier search path.
fn selective_search(project: &Project) -> (Vec<(String, Vec<PathBuf>)>, bool) {
    let mut paths: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut uncertain = false;
    for directive in project.evaluation().vpaths() {
        let Some(value) = directive else {
            uncertain = true;
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            paths.clear();
            uncertain = false;
            continue;
        }
        let (pattern, directories) = value
            .split_once(char::is_whitespace)
            .map_or((value, ""), |(pattern, directories)| {
                (pattern, directories.trim())
            });
        if pattern.contains(['\\', '$'])
            || pattern.matches('%').count() > 1
            || directories.contains(['\\', ';'])
            || directories.contains(":/")
            || (cfg!(windows) && directories.contains(':'))
        {
            uncertain = true;
            continue;
        }
        if directories.is_empty() {
            paths.retain(|(existing, _)| existing != pattern);
        } else {
            paths.push((
                pattern.into(),
                directories
                    .split(|c: char| c == ':' || c.is_whitespace())
                    .filter(|part| !part.is_empty())
                    .map(|part| project.working_directory().join(part))
                    .collect(),
            ));
        }
    }
    (paths, uncertain)
}

// Follow assignment readers back from graph-facing references. Unknown names,
// scopes and exhausted work retain the old project exclusion. Recipe expansion
// itself is outside MK216's static prerequisite model.
fn recipe_only_assignments(
    project: &Project,
    mut budget: usize,
) -> BTreeSet<(crate::project::SourceId, usize)> {
    use crate::analysis::{ReferenceContext, ReferenceKind};
    use crate::parser::VariableScope;
    let index = project.analysis();
    let mut owners: BTreeMap<_, Vec<String>> = BTreeMap::new();
    let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut needed = BTreeSet::new();
    for (name, symbol) in &index.variables {
        if !plain_recipe_setting(name)
            || symbol
                .definitions
                .iter()
                .any(|definition| definition.scope != VariableScope::Global)
        {
            needed.insert(name.clone());
        }
        for definition in &symbol.definitions {
            for line in definition.location.line..=definition.end_line {
                let Some(remaining) = budget.checked_sub(1) else {
                    return BTreeSet::new();
                };
                budget = remaining;
                owners
                    .entry((definition.location.source, line))
                    .or_default()
                    .push(name.clone());
            }
        }
    }
    for reference in &index.references {
        let Some(remaining) = budget.checked_sub(1) else {
            return BTreeSet::new();
        };
        budget = remaining;
        if reference.context == ReferenceContext::Recipe {
            continue;
        }
        if reference.kind == ReferenceKind::Dynamic {
            return BTreeSet::new();
        }
        if reference.kind != ReferenceKind::Variable {
            continue;
        }
        if reference.context == ReferenceContext::Assignment {
            match owners.get(&(reference.location.source, reference.location.line)) {
                Some(names) if names.len() == 1 => {
                    dependencies
                        .entry(names[0].clone())
                        .or_default()
                        .insert(reference.name.clone());
                }
                _ => {
                    needed.insert(reference.name.clone());
                }
            }
        } else {
            needed.insert(reference.name.clone());
        }
    }
    let mut pending: Vec<_> = needed.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        if let Some(inputs) = dependencies.get(&name) {
            for input in inputs {
                let Some(remaining) = budget.checked_sub(1) else {
                    return BTreeSet::new();
                };
                budget = remaining;
                if needed.insert(input.clone()) {
                    pending.push(input.clone());
                }
            }
        }
    }
    index
        .variables
        .iter()
        .filter(|(name, _)| !needed.contains(*name))
        .flat_map(|(_, symbol)| {
            symbol
                .definitions
                .iter()
                .map(|definition| (definition.location.source, definition.location.line))
        })
        .collect()
}

fn plain_recipe_setting(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !matches!(
            name,
            "VPATH"
                | "GPATH"
                | "MAKEFILES"
                | "MAKEFLAGS"
                | "MFLAGS"
                | "GNUMAKEFLAGS"
                | "MAKEOVERRIDES"
                | "SHELL"
        )
}

// Count all independent exclusions, rather than only the first early return.
// Counts are occurrences; a root can have several blockers simultaneously.
fn root_blockers(project: &Project) -> BTreeMap<&'static str, usize> {
    let recipe_only = recipe_only_assignments(project, 10_000);
    let mut reasons = BTreeMap::new();
    let mut add = |reason| *reasons.entry(reason).or_default() += 1;
    if !matches!(
        project
            .file(project.root())
            .path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("Makefile" | "makefile" | "GNUmakefile")
    ) {
        add("fragment_root");
    }
    let index = project.analysis();
    if index.has_structural_issues() {
        add("structural_issues");
    }
    for edge in project.edges() {
        if !matches!(
            edge.resolution,
            IncludeResolution::Resolved(_) | IncludeResolution::Inactive
        ) {
            add("unresolved_include");
        }
    }
    // Nonstandard suffix names can contain paths or omit the leading dot.
    // Keep those lists opaque rather than mistaking their conversions for files.
    if index.targets.get(".SUFFIXES").is_some_and(|symbol| {
        symbol.dependencies.iter().any(|edge| {
            !edge.prerequisite.starts_with('.') || edge.prerequisite.contains(['/', '\\'])
        })
    }) {
        add("suffix_rule");
    }
    for (name, symbol) in &index.targets {
        if name == ".DEFAULT" {
            add("default_recipe");
        }
        if name == ".SECONDEXPANSION" {
            add("secondary_expansion");
        }
        let suffix_name = normalized(name);
        if suffix_name.starts_with('.')
            && !suffix_name.contains(['%', '/'])
            && suffix_name[1..].contains('.')
        {
            add("suffix_rule");
        }
        if symbol
            .declarations
            .iter()
            .any(|declaration| declaration.target_pattern.is_some())
        {
            add("static_pattern_rule");
        }
    }
    for file in project.files() {
        for statement in file.makefile.logical.statements() {
            let activity = project.evaluation().activity(file.id, statement.start_line);
            if activity == Truth::False {
                continue;
            }
            if activity == Truth::Unknown {
                add("unknown_activity");
            }
            let text = statement.text().trim_start();
            if matches!(statement.kind, LogicalKind::Unknown) {
                add("opaque_syntax");
            }
            for (round, brace, reason) in [
                ("$(eval", "${eval", "eval_function"),
                ("$(shell", "${shell", "shell_function"),
                ("$(file", "${file", "file_function"),
            ] {
                if text.contains(round) || text.contains(brace) {
                    add(reason);
                }
            }
            if statement.kind == LogicalKind::Assignment {
                if text.contains("!=") {
                    add("shell_assignment");
                }
                if text.contains('$')
                    && project.evaluation().expand(text).value.is_none()
                    && !recipe_only.contains(&(file.id, statement.start_line))
                {
                    add("unresolved_assignment");
                }
            }
            if statement.kind == LogicalKind::Rule {
                let rules = project.evaluation().rules(file.id, statement.start_line);
                if rules.is_empty()
                    || rules.iter().any(|rule| {
                        rule.targets
                            .iter()
                            .chain(&rule.prerequisites)
                            .chain(&rule.order_only_prerequisites)
                            .any(|name| name.contains('$'))
                    })
                {
                    add("unresolved_rule");
                }
            }
        }
    }
    reasons
}

// Bound declaration and prerequisite work per checked edge. Exhaustion is
// uncertainty. Nonterminal prerequisites are inspected for possible producers,
// but their chains are deliberately not expanded recursively.
const PATTERN_SEARCH_STEPS: usize = 10_000;

fn possible_pattern_producer(
    project: &Project,
    name: &str,
    directories: &[PathBuf],
    inputs: &InputIndex<'_>,
) -> bool {
    let mut budget = PATTERN_SEARCH_STEPS;
    pattern_producer(project, name, directories, true, &mut budget, None, inputs)
}

fn pattern_producer(
    project: &Project,
    name: &str,
    directories: &[PathBuf],
    inspect_inputs: bool,
    budget: &mut usize,
    excluded: Option<crate::project_analysis::SourceLocation>,
    inputs: &InputIndex<'_>,
) -> bool {
    project
        .analysis()
        .targets
        .iter()
        .filter(|(pattern, _)| pattern.contains('%'))
        .any(|(pattern, symbol)| {
            // GNU Make never uses one implicit rule twice in a chain. A
            // speculative built-in input must not revive its own producer.
            if symbol
                .declarations
                .iter()
                .all(|d| Some(d.location) == excluded)
            {
                return false;
            }
            if pattern.contains(['\\', '$']) {
                return true;
            }
            directories.iter().any(|directory| {
                let Some(remaining) = budget.checked_sub(1) else {
                    return true;
                };
                *budget = remaining;
                let candidate = directory.join(name);
                let Ok(candidate) = candidate.strip_prefix(project.working_directory()) else {
                    // Absolute/external search paths may use another spelling in
                    // the target table. Do not assume those names are unrelated.
                    return true;
                };
                let Some(candidate) = make_path(candidate) else {
                    return true;
                };
                let candidate = normalized(&candidate);
                let pattern = normalized(pattern);
                let full_candidate = candidate;
                let candidate = if pattern.contains('/') {
                    candidate
                } else {
                    // GNU ignores the directory while matching a slashless pattern.
                    candidate.rsplit('/').next().unwrap_or(candidate)
                };
                if let Some(stem) = pattern_stem(pattern, candidate) {
                    let suffix = pattern
                        .split_once('%')
                        .map(|(_, suffix)| suffix)
                        .unwrap_or("");
                    // A broad pattern may fail for this target but build a
                    // differently suffixed source used by a built-in rule.
                    // Only inspect patterns fixing the complete final extension.
                    if !inspect_inputs
                        || stem.is_empty()
                        || pattern.matches('%').count() != 1
                        || !suffix.starts_with('.')
                        || suffix[1..].contains(['.', '/'])
                    {
                        return true;
                    }
                    let parent = if pattern.contains('/') {
                        Path::new("")
                    } else {
                        Path::new(full_candidate).parent().unwrap_or(Path::new(""))
                    };
                    return declaration_may_build(
                        project,
                        symbol,
                        stem,
                        parent,
                        directories,
                        budget,
                        inputs,
                    );
                }
                let path = Path::new(candidate);
                let parent = path.parent().unwrap_or(Path::new(""));
                let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
                    return true;
                };
                // Built-in conversions can consume generated same-basename inputs.
                // Admit every suffix, rather than pinning a host-specific catalogue.
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(filename);
                let prefix = pattern.split('%').next().unwrap_or("");
                for base in [filename, stem] {
                    let source_prefix = parent.join(format!("{base}."));
                    let Some(source_prefix) = make_path(&source_prefix) else {
                        return true;
                    };
                    if source_prefix.starts_with(prefix) || prefix.starts_with(&*source_prefix) {
                        return true;
                    }
                }
                [
                    parent.join(format!("s.{filename}")),
                    parent.join(format!("{filename},v")),
                    parent.join("RCS").join(filename),
                    parent.join("SCCS").join(format!("s.{filename}")),
                ]
                .iter()
                .any(|source| {
                    make_path(source).is_none_or(|source| pattern_stem(pattern, &source).is_some())
                })
            })
        })
}

// Dependencies belong to individual declarations, not to the union of all
// rules with this target pattern. One possible alternative is sufficient.
fn declaration_may_build(
    project: &Project,
    symbol: &crate::project_analysis::ProjectTargetSymbol,
    stem: &str,
    parent: &Path,
    directories: &[PathBuf],
    budget: &mut usize,
    inputs: &InputIndex<'_>,
) -> bool {
    symbol.declarations.iter().any(|declaration| {
        let Some(remaining) = budget.checked_sub(1) else {
            return true;
        };
        *budget = remaining;
        // Cancellation and grouped/multi-target rules need rule-set
        // semantics beyond this single-declaration proof.
        let rules = project
            .evaluation()
            .rules(declaration.location.source, declaration.location.line);
        if !declaration.has_recipe
            || declaration.grouped
            || rules.iter().any(|rule| rule.targets.len() != 1)
        {
            return true;
        }
        symbol
            .dependencies
            .iter()
            .filter(|edge| edge.location == declaration.location)
            .all(|edge| {
                let Some(remaining) = budget.checked_sub(1) else {
                    return true;
                };
                *budget = remaining;
                if edge.prerequisite.contains('\\') {
                    return true;
                }
                let input = edge.prerequisite.replacen('%', stem, 1);
                // Only prerequisite patterns regain the stripped directory;
                // literal inputs stay relative to the working directory.
                let input = if edge.prerequisite.contains('%') {
                    parent.join(input)
                } else {
                    PathBuf::from(input)
                };
                let Some(input) = make_path(&input) else {
                    return true;
                };
                let input = normalized(&input);
                !literal(input)
                    || inputs.declared(input)
                    || directories
                        .iter()
                        .any(|directory| may_exist(&directory.join(input)))
                    || (!declaration.double_colon
                        && (inputs.plausible(input, directories)
                            || pattern_producer(
                                project,
                                input,
                                directories,
                                false,
                                budget,
                                Some(declaration.location),
                                inputs,
                            )))
            })
    })
}

// Paths assembled by the host must use Make's slash spelling when compared
// with target names. Do not rewrite raw Make expressions, where backslashes
// can be escapes rather than directory separators.
fn make_path(path: &Path) -> Option<std::borrow::Cow<'_, str>> {
    path.to_str().map(|name| {
        if cfg!(windows) {
            std::borrow::Cow::Owned(name.replace('\\', "/"))
        } else {
            std::borrow::Cow::Borrowed(name)
        }
    })
}

fn normalized(mut name: &str) -> &str {
    while let Some(rest) = name.strip_prefix("./") {
        name = rest.trim_start_matches('/');
    }
    name
}

fn literal(name: &str) -> bool {
    !name.is_empty()
        && name != ".WAIT"
        && !name.starts_with('-')
        && !name.starts_with('~')
        && !name
            .chars()
            .any(|c| c.is_whitespace() || "$%*?[]()\\|:".contains(c))
}

// Permission and other I/O errors are uncertainty, not evidence of absence.
fn may_exist(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectOptions;

    #[test]
    fn exhausted_assignment_search_does_not_relax_exclusions() {
        let directory = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &directory.path().join("Makefile"),
            "LOCAL = $(OPTIONAL)\nCFLAGS = $(LOCAL)\nprobe: missing.txt\n\t@echo $(CFLAGS)\n"
                .into(),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(recipe_only_assignments(&project, 0).is_empty());
        assert!(!recipe_only_assignments(&project, 10_000).is_empty());
    }

    #[test]
    fn exhausted_pattern_search_preserves_uncertainty() {
        let directory = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &directory.path().join("Makefile"),
            "probe: generated/output.dat\ngenerated/%.dat: missing/%.src\n\t@echo generated\n"
                .into(),
            &ProjectOptions::default(),
        )
        .unwrap();
        let directories = [directory.path().to_path_buf()];
        for mut budget in 0..3 {
            assert!(pattern_producer(
                &project,
                "generated/output.dat",
                &directories,
                true,
                &mut budget,
                None,
                &InputIndex::new(&project),
            ));
        }
        assert!(!possible_pattern_producer(
            &project,
            "generated/output.dat",
            &directories,
            &InputIndex::new(&project),
        ));
    }
}
