//! Paths as the reader of a diagnostic sees them.

use std::path::{Path, PathBuf};

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
