use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    pub description: String,
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
}

impl Fix {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
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
