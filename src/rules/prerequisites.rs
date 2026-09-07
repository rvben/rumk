//! A deliberately incomplete proof of missing ordinary prerequisites.
//! Unknown build mechanisms cost coverage, never an invented error.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, Severity};
use crate::eval::Truth;
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
        "Static prerequisite has no visible file or build rule"
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
        let index = project.analysis();
        let root = &project.file(project.root()).path;
        if !matches!(
            root.file_name().and_then(|name| name.to_str()),
            Some("Makefile" | "makefile" | "GNUmakefile")
        ) || index.has_structural_issues()
            || incomplete(project)
        {
            return Vec::new();
        }
        // The CLI may supply the bare relative path "Makefile". Its parent is
        // the empty path, which joins files correctly but read_dir rejects.
        let mut directories = vec![project.working_directory().join(".")];
        match project.evaluation().expand("$(VPATH)").value {
            Some(value) => {
                // Drive letters and escaped separators need platform-specific parsing.
                if value.contains(['\\', ';'])
                    || value.contains(":/")
                    || (cfg!(windows) && value.contains(':'))
                {
                    return Vec::new();
                }
                directories.extend(
                    value
                        .split(|c: char| c == ':' || c.is_whitespace())
                        .filter(|part| !part.is_empty())
                        .map(|part| project.working_directory().join(part)),
                );
            }
            None if index.variables.contains_key("VPATH") => return Vec::new(),
            None => {}
        }
        let mut seen = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for target in index.targets.values().filter(|target| !target.special) {
            for edge in &target.dependencies {
                let name = normalized(&edge.prerequisite);
                if !index.is_definitely_active(edge.location)
                    || !literal(name)
                    || index.targets.keys().any(|key| normalized(key) == name)
                    || directories
                        .iter()
                        .any(|directory| may_exist(&directory.join(name)))
                    || plausible_implicit_input(project, name, &directories)
                    || !seen.insert((edge.location, name.to_string()))
                {
                    continue;
                }
                diagnostics.push(Diagnostic::new(self.id(), Severity::Warning,
                    format!("Prerequisite '{name}' of target '{}' was not found and has no visible build rule", target.name),
                    edge.location.line, edge.location.column)
                    .with_source(project.file(edge.location.source).path.clone()));
            }
        }
        diagnostics
    }
}

fn incomplete(project: &Project) -> bool {
    if project.edges().iter().any(|edge| {
        !matches!(
            edge.resolution,
            IncludeResolution::Resolved(_) | IncludeResolution::Inactive
        )
    }) {
        return true;
    }
    let index = project.analysis();
    if index.targets.iter().any(|(name, symbol)| {
        name.contains('%')
            || matches!(name.as_str(), ".DEFAULT" | ".SECONDEXPANSION")
            || (name.starts_with('.') && name[1..].contains('.'))
            || symbol
                .declarations
                .iter()
                .any(|declaration| declaration.target_pattern.is_some())
    }) {
        return true;
    }
    for file in project.files() {
        for statement in file.makefile.logical.statements() {
            let activity = project.evaluation().activity(file.id, statement.start_line);
            if activity == Truth::False {
                continue;
            }
            if activity == Truth::Unknown {
                return true;
            }
            let text = statement.text().trim_start();
            if matches!(statement.kind, LogicalKind::Unknown)
                || text.split_whitespace().next() == Some("vpath")
                || text.contains("$(eval")
                || text.contains("${eval")
                || text.contains("$(shell")
                || text.contains("${shell")
                || text.contains("$(file")
                || text.contains("${file")
                || (statement.kind == LogicalKind::Assignment && text.contains("!="))
                || (statement.kind == LogicalKind::Assignment
                    && text.contains('$')
                    && project.evaluation().expand(text).value.is_none())
            {
                return true;
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
                    return true;
                }
            }
        }
    }
    false
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

fn related(candidate: &str, filename: &str) -> bool {
    // Withhold on case variants too: the filesystem may be case-insensitive
    // even though Make's target table is not. Non-ASCII spellings may also
    // compare equal through filesystem normalization, which we do not model.
    if !candidate.is_ascii() || !filename.is_ascii() {
        return true;
    }
    let candidate = candidate.to_ascii_lowercase();
    let filename = filename.to_ascii_lowercase();
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&filename);
    candidate == stem
        || candidate == format!("s.{filename}")
        || candidate.starts_with(&format!("{stem}."))
        || candidate.starts_with(&format!("{filename},"))
}

// GNU's built-in conversions retain a basename. Any same-stem source, including
// a declared but not-yet-created source, is enough to withhold this warning.
// This deliberately overapproximates instead of copying a host's rule database.
fn plausible_implicit_input(project: &Project, name: &str, directories: &[PathBuf]) -> bool {
    let path = Path::new(name);
    let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
        return true;
    };
    let parent = path.parent().unwrap_or(Path::new(""));
    if project.analysis().targets.keys().any(|key| {
        let candidate = Path::new(normalized(key));
        candidate.parent() == Some(parent)
            && candidate
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|candidate| related(candidate, filename))
    }) {
        return true;
    }
    directories.iter().any(|directory| {
        let directory = directory.join(parent);
        if may_exist(&directory.join("RCS")) || may_exist(&directory.join("SCCS")) {
            return true;
        }
        match std::fs::read_dir(directory) {
            Ok(entries) => entries.into_iter().any(|entry| match entry {
                Ok(entry) => entry
                    .file_name()
                    .to_str()
                    .is_none_or(|candidate| related(candidate, filename)),
                Err(_) => true,
            }),
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        }
    })
}
