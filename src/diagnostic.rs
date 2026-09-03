use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Source file for project-level diagnostics. Single-file diagnostics use
    /// the path supplied by their report instead.
    #[serde(skip)]
    pub source: Option<PathBuf>,
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub fixable: bool,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Whether applying a fix can change what Make does with the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Applicability {
    /// Make reads the fixed file the way it read the original, or the original
    /// was not a file Make would read at all and the fix is the only reading
    /// that makes it one.
    #[default]
    Safe,
    /// The fix can change what Make does, so a run applies it only when it is
    /// asked for.
    Unsafe,
}

impl Applicability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Unsafe => "unsafe",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    pub description: String,
    #[serde(default)]
    pub applicability: Applicability,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub replacement: String,
}

impl Diagnostic {
    pub fn new(
        rule_id: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        line: usize,
        column: usize,
    ) -> Self {
        Self {
            source: None,
            rule_id: rule_id.into(),
            severity,
            message: message.into(),
            line,
            column,
            end_line: None,
            end_column: None,
            fixable: false,
            fix: None,
        }
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixable = true;
        self.fix = Some(fix);
        self
    }

    pub fn with_source(mut self, source: impl Into<PathBuf>) -> Self {
        self.source = Some(source.into());
        self
    }
}

impl Fix {
    /// A fix Make cannot tell apart from the text it replaces.
    pub fn new(description: impl Into<String>) -> Self {
        Self::with_applicability(description, Applicability::Safe)
    }

    /// A fix that can change what Make does with the file, which a run applies
    /// only when it asks for unsafe fixes.
    pub fn unsafe_fix(description: impl Into<String>) -> Self {
        Self::with_applicability(description, Applicability::Unsafe)
    }

    fn with_applicability(description: impl Into<String>, applicability: Applicability) -> Self {
        Self {
            description: description.into(),
            applicability,
            edits: Vec::new(),
        }
    }

    pub fn add_edit(mut self, edit: Edit) -> Self {
        self.edits.push(edit);
        self
    }
}

impl Edit {
    pub fn new(
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            start_line,
            start_column,
            end_line,
            end_column,
            replacement: replacement.into(),
        }
    }
}
