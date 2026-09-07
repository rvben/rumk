//! Paths as the reader of a diagnostic sees them.

use std::path::Path;

/// How a path reads beside the directory Rumk was run from. A file inside that
/// directory is named the way the reader would type it, and one outside it
/// keeps the name it has. Every path Rumk prints goes through here, so the file
/// a message points at and the file it names inside its own text read alike.
pub fn display_path(path: &Path) -> String {
    let current_dir = std::env::current_dir()
        .ok()
        .and_then(|path| dunce::canonicalize(path).ok());
    let canonical_path = dunce::canonicalize(path).ok();
    let comparable_path = canonical_path.as_deref().unwrap_or(path);
    let relative = current_dir
        .as_deref()
        .and_then(|current_dir| comparable_path.strip_prefix(current_dir).ok())
        .unwrap_or(comparable_path);
    relative
        .strip_prefix(".")
        .unwrap_or(relative)
        .display()
        .to_string()
}
