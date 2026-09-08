//! Per-analysis indexes for declared targets and possible implicit source names.
//! Filesystem observations never survive a lint pass.
use super::{may_exist, normalized};
use crate::project::Project;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_DIRECTORY_ENTRIES: usize = 100_000;
const MAX_DIRECTORIES: usize = 1024;

#[derive(Default)]
struct Names {
    values: BTreeSet<String>,
    uncertain: bool,
}
impl Names {
    fn insert(&mut self, name: Option<&str>) {
        match name {
            Some(name) if name.is_ascii() => {
                self.values.insert(name.to_ascii_lowercase());
            }
            _ => self.uncertain = true,
        }
    }
    fn related(&self, filename: &str) -> bool {
        if self.uncertain || (!self.values.is_empty() && !filename.is_ascii()) {
            return true;
        }
        let filename = filename.to_ascii_lowercase();
        let stem = Path::new(&filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&filename);
        self.values.contains(stem)
            || self.values.contains(&format!("s.{filename}"))
            || [format!("{stem}."), format!("{filename},")]
                .iter()
                .any(|prefix| {
                    self.values
                        .range(prefix.clone()..)
                        .next()
                        .is_some_and(|name| name.starts_with(prefix))
                })
    }
    fn read(directory: &Path, remaining: &mut usize) -> Self {
        let mut names = Self::default();
        if may_exist(&directory.join("RCS")) || may_exist(&directory.join("SCCS")) {
            names.uncertain = true;
            return names;
        }
        match std::fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    if *remaining == 0 {
                        names.uncertain = true;
                        break;
                    }
                    *remaining -= 1;
                    match entry {
                        Ok(entry) => names.insert(entry.file_name().to_str()),
                        Err(_) => names.uncertain = true,
                    }
                    if names.uncertain {
                        break;
                    }
                }
            }
            Err(error) => names.uncertain = error.kind() != std::io::ErrorKind::NotFound,
        }
        names
    }
}

struct Filesystem {
    directories: BTreeMap<PathBuf, Names>,
    remaining: usize,
}

pub(super) struct InputIndex<'a> {
    targets: BTreeSet<&'a str>,
    parents: BTreeMap<PathBuf, Names>,
    filesystem: RefCell<Filesystem>,
}
impl<'a> InputIndex<'a> {
    pub fn new(project: &'a Project) -> Self {
        let targets: BTreeSet<_> = project
            .analysis()
            .targets
            .keys()
            .map(|name| normalized(name))
            .collect();
        let mut parents: BTreeMap<PathBuf, Names> = BTreeMap::new();
        for name in &targets {
            let path = Path::new(name);
            if let (Some(parent), Some(filename)) =
                (path.parent(), path.file_name().and_then(|s| s.to_str()))
            {
                parents
                    .entry(parent.into())
                    .or_default()
                    .insert(Some(filename));
            }
        }
        Self {
            targets,
            parents,
            filesystem: RefCell::new(Filesystem {
                directories: BTreeMap::new(),
                remaining: MAX_DIRECTORY_ENTRIES,
            }),
        }
    }
    pub fn declared(&self, name: &str) -> bool {
        self.targets.contains(name)
    }
    pub fn plausible(&self, name: &str, directories: &[PathBuf]) -> bool {
        let path = Path::new(name);
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            return true;
        };
        let parent = path.parent().unwrap_or(Path::new(""));
        if self
            .parents
            .get(parent)
            .is_some_and(|names| names.related(filename))
        {
            return true;
        }
        directories.iter().any(|directory| {
            let directory = directory.join(parent);
            let mut filesystem = self.filesystem.borrow_mut();
            if !filesystem.directories.contains_key(&directory) {
                if filesystem.directories.len() >= MAX_DIRECTORIES {
                    return true;
                }
                let names = Names::read(&directory, &mut filesystem.remaining);
                filesystem.directories.insert(directory.clone(), names);
            }
            filesystem.directories[&directory].related(filename)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The former linear relation is an independent oracle for the indexed query.
    fn related(candidate: &str, filename: &str) -> bool {
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
    #[test]
    fn indexed_relations_preserve_case_suffix_and_unicode_uncertainty() {
        let cases = [
            "file.o", "file", "file.c", "file.o.c", "FILE.C", "s.file.o", "file.o,v", "fileXc",
            ".hidden", "a.b.c", "é.o", "other",
        ];
        for filename in cases {
            assert!(!Names::default().related(filename));
        }
        for candidate in cases {
            let mut index = Names::default();
            index.insert(Some(candidate));
            for filename in cases {
                assert_eq!(
                    index.related(filename),
                    related(candidate, filename),
                    "{candidate}/{filename}"
                );
            }
        }
    }
    #[test]
    fn exhausted_directory_work_withholds_absence_and_fresh_passes_see_changes() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("unrelated"), "").unwrap();
        assert!(Names::read(directory.path(), &mut 0).related("missing.o"));
        assert!(!Names::read(directory.path(), &mut 10).related("missing.o"));
        std::fs::write(directory.path().join("missing.c"), "").unwrap();
        assert!(Names::read(directory.path(), &mut 10).related("missing.o"));
        assert!(!Names::read(&directory.path().join("absent"), &mut 10).related("missing.o"));
    }
}
