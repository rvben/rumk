//! Safe, non-evaluating loading of statically included Makefiles.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

use crate::parser::{self, Makefile};
use crate::project_analysis::ProjectSemanticIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub usize);

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub id: SourceId,
    pub path: PathBuf,
    pub content: String,
    pub makefile: Makefile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncludeResolution {
    Resolved(SourceId),
    Missing { searched: Vec<PathBuf> },
    Dynamic,
    Unreadable { path: PathBuf, message: String },
    Invalid { path: PathBuf, message: String },
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeEdge {
    pub from: SourceId,
    pub expression: String,
    pub optional: bool,
    pub line: usize,
    pub resolution: IncludeResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeCycle {
    /// The first and last source are the same, making the cycle explicit.
    pub sources: Vec<SourceId>,
    pub edge_line: usize,
}

#[derive(Debug, Clone)]
pub struct Project {
    root: SourceId,
    files: Vec<ProjectFile>,
    edges: Vec<IncludeEdge>,
    cycles: Vec<IncludeCycle>,
    analysis: OnceLock<ProjectSemanticIndex>,
}

impl Project {
    pub fn load(path: &Path, options: &ProjectOptions) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read Makefile: {}", path.display()))?;
        Self::load_with_root_content(path, content, options)
    }

    pub fn load_with_root_content(
        path: &Path,
        content: String,
        options: &ProjectOptions,
    ) -> Result<Self> {
        let path = canonical_or_normalized(path)?;
        let makefile = parser::parse(&content)
            .with_context(|| format!("Failed to parse Makefile: {}", path.display()))?;
        let mut loader = Loader::new(options);
        let root = loader.insert_file(path, content, makefile);
        loader.paths.insert(loader.files[root.0].path.clone(), root);
        loader.visit(root);
        Ok(Self {
            root,
            files: loader.files,
            edges: loader.edges,
            cycles: loader.cycles,
            analysis: OnceLock::new(),
        })
    }

    pub fn root(&self) -> SourceId {
        self.root
    }

    pub fn files(&self) -> &[ProjectFile] {
        &self.files
    }

    pub fn file(&self, id: SourceId) -> &ProjectFile {
        &self.files[id.0]
    }

    pub fn edges(&self) -> &[IncludeEdge] {
        &self.edges
    }

    pub fn cycles(&self) -> &[IncludeCycle] {
        &self.cycles
    }

    /// Returns the cross-file semantic index, building it once on first use.
    pub fn analysis(&self) -> &ProjectSemanticIndex {
        self.analysis
            .get_or_init(|| ProjectSemanticIndex::build(self))
    }
}

#[derive(Debug, Clone)]
pub struct ProjectOptions {
    pub include_paths: Vec<PathBuf>,
    pub max_files: usize,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            include_paths: Vec::new(),
            max_files: 1024,
        }
    }
}

struct Loader<'a> {
    options: &'a ProjectOptions,
    files: Vec<ProjectFile>,
    paths: BTreeMap<PathBuf, SourceId>,
    edges: Vec<IncludeEdge>,
    cycles: Vec<IncludeCycle>,
    visiting: Vec<SourceId>,
    visited: BTreeSet<SourceId>,
}

impl<'a> Loader<'a> {
    fn new(options: &'a ProjectOptions) -> Self {
        Self {
            options,
            files: Vec::new(),
            paths: BTreeMap::new(),
            edges: Vec::new(),
            cycles: Vec::new(),
            visiting: Vec::new(),
            visited: BTreeSet::new(),
        }
    }

    fn insert_file(&mut self, path: PathBuf, content: String, makefile: Makefile) -> SourceId {
        let id = SourceId(self.files.len());
        self.files.push(ProjectFile {
            id,
            path,
            content,
            makefile,
        });
        id
    }

    fn visit(&mut self, source: SourceId) {
        if !self.visited.insert(source) {
            return;
        }
        self.visiting.push(source);
        let including_path = self.files[source.0].path.clone();
        let includes = self.files[source.0].makefile.includes.clone();

        for include in includes {
            for expression in include.paths {
                let (resolution, discovered) =
                    self.resolve_include(&including_path, &expression, include.line);
                self.edges.push(IncludeEdge {
                    from: source,
                    expression,
                    optional: include.optional,
                    line: include.line,
                    resolution,
                });
                if let Some(discovered) = discovered {
                    self.visit(discovered);
                }
            }
        }

        self.visiting.pop();
    }

    fn resolve_include(
        &mut self,
        including_path: &Path,
        expression: &str,
        line: usize,
    ) -> (IncludeResolution, Option<SourceId>) {
        if is_dynamic_include(expression) {
            return (IncludeResolution::Dynamic, None);
        }

        let candidates = include_candidates(including_path, expression, self.options);
        let Some(path) = candidates.iter().find(|candidate| candidate.is_file()) else {
            return (
                IncludeResolution::Missing {
                    searched: candidates,
                },
                None,
            );
        };
        let path = match canonical_or_normalized(path) {
            Ok(path) => path,
            Err(error) => {
                return (
                    IncludeResolution::Unreadable {
                        path: path.clone(),
                        message: error.to_string(),
                    },
                    None,
                );
            }
        };

        if let Some(id) = self.paths.get(&path).copied() {
            self.record_cycle(id, line);
            return (IncludeResolution::Resolved(id), None);
        }
        if self.files.len() >= self.options.max_files {
            return (IncludeResolution::LimitExceeded, None);
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                return (
                    IncludeResolution::Unreadable {
                        path,
                        message: error.to_string(),
                    },
                    None,
                );
            }
        };
        let makefile = match parser::parse(&content) {
            Ok(makefile) => makefile,
            Err(error) => {
                return (
                    IncludeResolution::Invalid {
                        path,
                        message: error.to_string(),
                    },
                    None,
                );
            }
        };
        let id = self.insert_file(path.clone(), content, makefile);
        self.paths.insert(path, id);
        (IncludeResolution::Resolved(id), Some(id))
    }

    fn record_cycle(&mut self, to: SourceId, edge_line: usize) {
        let Some(start) = self.visiting.iter().position(|source| *source == to) else {
            return;
        };
        let mut sources = self.visiting[start..].to_vec();
        sources.push(to);
        let key: Vec<_> = sources.iter().map(|source| source.0).collect();
        if self.cycles.iter().any(|cycle| {
            cycle
                .sources
                .iter()
                .map(|source| source.0)
                .collect::<Vec<_>>()
                == key
        }) {
            return;
        }
        self.cycles.push(IncludeCycle { sources, edge_line });
    }
}

fn include_candidates(
    including_path: &Path,
    expression: &str,
    options: &ProjectOptions,
) -> Vec<PathBuf> {
    let path = Path::new(expression);
    if path.is_absolute() {
        return vec![normalize_path(path)];
    }

    let parent = including_path.parent().unwrap_or_else(|| Path::new("."));
    std::iter::once(parent.to_path_buf())
        .chain(options.include_paths.iter().cloned())
        .map(|directory| normalize_path(&directory.join(path)))
        .collect()
}

fn is_dynamic_include(expression: &str) -> bool {
    expression.contains(['$', '%', '*', '?', '['])
}

fn canonical_or_normalized(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("Failed to resolve path: {}", path.display()));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to determine current directory")?
            .join(path)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
