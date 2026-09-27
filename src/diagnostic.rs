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

    /// Ends the reported range before `end_column` on `end_line`, for a rule
    /// that knows the text it is about.
    pub fn with_end(mut self, end_line: usize, end_column: usize) -> Self {
        self.end_line = Some(end_line);
        self.end_column = Some(end_column);
        self
    }
}

/// Ends each range a rule left open at the end of its start line, so an editor
/// or annotation marks the statement instead of a single position. Trailing
/// whitespace is left out unless the finding is in it, and a position past
/// the text keeps no end.
pub fn fill_spans(diagnostics: &mut [Diagnostic], content: &str) {
    if diagnostics
        .iter()
        .all(|diagnostic| diagnostic.end_line.is_some())
    {
        return;
    }
    let lines: Vec<_> = content.lines().collect();
    for diagnostic in diagnostics
        .iter_mut()
        .filter(|diagnostic| diagnostic.end_line.is_none())
    {
        let Some(line) = diagnostic
            .line
            .checked_sub(1)
            .and_then(|index| lines.get(index))
        else {
            continue;
        };
        let end = [line.trim_end(), line]
            .map(|text| text.chars().count() + 1)
            .into_iter()
            .find(|&end| end > diagnostic.column);
        if let Some(end) = end {
            diagnostic.end_line = Some(diagnostic.line);
            diagnostic.end_column = Some(end);
        }
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
