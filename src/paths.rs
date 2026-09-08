//! Paths as the reader of a diagnostic sees them.

use std::path::{Path, PathBuf};

/// Resolve a buffer's identity even before its file or parent directory exists.
/// Canonicalizing the existing ancestor reconciles Windows path spelling and
/// symlinked workspaces with identities used by disk-backed project analysis.
pub fn resolve_buffer_path(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    for ancestor in absolute.ancestors() {
        if ancestor.exists() {
            let resolved = dunce::canonicalize(ancestor)?;
            return crate::project::canonical_or_normalized(
                &resolved.join(absolute.strip_prefix(ancestor)?),
            );
        }
    }
    crate::project::canonical_or_normalized(&absolute)
}

/// Formats resolved paths relative to one invocation's working directory.
/// Keeping this state local also supports library callers that change directories.
pub struct PathDisplay {
    current_dir: Option<PathBuf>,
}

impl Default for PathDisplay {
    fn default() -> Self {
        Self {
            current_dir: std::env::current_dir()
                .ok()
                .and_then(|path| dunce::canonicalize(path).ok()),
        }
    }
}

impl PathDisplay {
    /// `path` is already resolved, or the original path if resolution failed.
    pub fn resolved(&self, path: &Path) -> String {
        let relative = self
            .current_dir
            .as_deref()
            .and_then(|current_dir| path.strip_prefix(current_dir).ok())
            .unwrap_or(path);
        relative
            .strip_prefix(".")
            .unwrap_or(relative)
            .display()
            .to_string()
    }
}

/// Resolves and formats a path for callers without an existing file identity.
pub fn display_path(path: &Path) -> String {
    let display = PathDisplay::default();
    let canonical = dunce::canonicalize(path).ok();
    display.resolved(canonical.as_deref().unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffers_use_the_canonical_existing_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let existing = dunce::canonicalize(directory.path()).unwrap();
        let missing = directory.path().join("new/Makefile");
        assert_eq!(
            resolve_buffer_path(&missing).unwrap(),
            existing.join("new/Makefile")
        );
        assert!(!missing.exists());
        #[cfg(windows)]
        {
            let alias = PathBuf::from(directory.path().to_string_lossy().to_ascii_uppercase());
            assert_eq!(
                resolve_buffer_path(&alias.join("Makefile")).unwrap(),
                existing.join("Makefile")
            );
        }
    }
}
