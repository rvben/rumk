//! Lossless, source-ordered Makefile syntax.
//!
//! This module deliberately starts with physical-line nodes. GNU Make syntax is
//! highly contextual, so retaining every byte and assigning conservative kinds
//! gives later parser stages a dependable foundation without inventing syntax
//! that was not present in the input.

/// A zero-based byte offset paired with a one-based, character-oriented source
/// location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

/// A half-open source range: `start` is included and `end` is excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    pub fn byte_range(self) -> std::ops::Range<usize> {
        self.start.offset..self.end.offset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    None,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::None => "",
        }
    }
}

/// A conservative lexical classification. `Unknown` is intentional: callers
/// can inspect unfamiliar or dynamic Make syntax without losing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxKind {
    Blank,
    Comment,
    Recipe,
    Assignment,
    Rule,
    Include,
    Conditional,
    Define,
    DefineBody,
    Endef,
    Directive,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxNode {
    pub kind: SyntaxKind,
    /// The full physical line, including its line ending when present.
    pub span: SourceSpan,
    /// The physical line without its line ending.
    pub content_span: SourceSpan,
    pub line_ending: LineEnding,
}

impl SyntaxNode {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.byte_range()]
    }

    pub fn content<'a>(&self, source: &'a str) -> &'a str {
        &source[self.content_span.byte_range()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTree {
    source: String,
    nodes: Vec<SyntaxNode>,
}

impl SyntaxTree {
    pub fn parse(source: &str) -> Self {
        let mut nodes = Vec::new();
        let mut offset = 0;
        let mut line = 1;
        let mut define_depth = 0usize;
        let mut recipe_prefix = '\t';

        while offset < source.len() {
            let remaining = &source[offset..];
            let newline_offset = remaining.find('\n');
            let full_end = newline_offset.map_or(source.len(), |index| offset + index + 1);
            let (content_end, line_ending) = match newline_offset {
                Some(index) if index > 0 && remaining.as_bytes()[index - 1] == b'\r' => {
                    (offset + index - 1, LineEnding::CrLf)
                }
                Some(index) => (offset + index, LineEnding::Lf),
                None => (source.len(), LineEnding::None),
            };

            let content = &source[offset..content_end];
            let kind = classify(content, &mut define_depth, recipe_prefix);
            if define_depth == 0 {
                if let Some(prefix) = assigned_recipe_prefix(content) {
                    recipe_prefix = prefix;
                }
            }
            let content_columns = content.chars().count() + 1;
            let next_position = if line_ending == LineEnding::None {
                SourcePosition {
                    offset: full_end,
                    line,
                    column: content_columns,
                }
            } else {
                SourcePosition {
                    offset: full_end,
                    line: line + 1,
                    column: 1,
                }
            };

            nodes.push(SyntaxNode {
                kind,
                span: SourceSpan {
                    start: SourcePosition {
                        offset,
                        line,
                        column: 1,
                    },
                    end: next_position,
                },
                content_span: SourceSpan {
                    start: SourcePosition {
                        offset,
                        line,
                        column: 1,
                    },
                    end: SourcePosition {
                        offset: content_end,
                        line,
                        column: content_columns,
                    },
                },
                line_ending,
            });

            offset = full_end;
            line += 1;
        }

        Self {
            source: source.to_owned(),
            nodes,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    /// Rendering an unmodified tree is byte-for-byte lossless.
    pub fn render(&self) -> &str {
        &self.source
    }
}

fn classify(line: &str, define_depth: &mut usize, recipe_prefix: char) -> SyntaxKind {
    let trimmed = line.trim_start();
    let keyword = directive_keyword(trimmed);

    if *define_depth > 0 {
        if keyword == Some("endef") {
            *define_depth -= 1;
            return SyntaxKind::Endef;
        }
        if keyword == Some("define") {
            *define_depth += 1;
            return SyntaxKind::Define;
        }
        return SyntaxKind::DefineBody;
    }

    if trimmed.is_empty() {
        return SyntaxKind::Blank;
    }
    if trimmed.starts_with('#') {
        return SyntaxKind::Comment;
    }
    if line.starts_with(recipe_prefix) {
        return SyntaxKind::Recipe;
    }

    match keyword {
        Some("include" | "-include" | "sinclude") => return SyntaxKind::Include,
        Some("ifdef" | "ifndef" | "ifeq" | "ifneq" | "else" | "endif") => {
            return SyntaxKind::Conditional;
        }
        Some("define") => {
            *define_depth = 1;
            return SyntaxKind::Define;
        }
        Some("endef") => return SyntaxKind::Endef,
        Some("export" | "unexport" | "override" | "private" | "undefine" | "vpath") => {
            return SyntaxKind::Directive;
        }
        _ => {}
    }

    if is_assignment(trimmed) {
        SyntaxKind::Assignment
    } else if is_rule(trimmed) {
        SyntaxKind::Rule
    } else if starts_with_modifier(trimmed) {
        SyntaxKind::Directive
    } else {
        SyntaxKind::Unknown
    }
}

fn assigned_recipe_prefix(line: &str) -> Option<char> {
    let trimmed = line.trim_start();
    let separator = assignment_separator(trimmed)?;
    let name = trimmed[..separator].split_whitespace().last()?;
    if name != ".RECIPEPREFIX" {
        return None;
    }

    let operator_length = [":::=", "::=", ":=", "?=", "+=", "!=", "="]
        .iter()
        .find(|operator| trimmed[separator..].starts_with(**operator))?
        .len();
    Some(
        trimmed[separator + operator_length..]
            .trim_start()
            .chars()
            .next()
            .unwrap_or('\t'),
    )
}

fn directive_keyword(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|word| !matches!(*word, "export" | "unexport" | "override" | "private"))
        .or_else(|| line.split_whitespace().next())
}

fn starts_with_modifier(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|word| matches!(word, "export" | "unexport" | "override" | "private"))
}

fn is_assignment(line: &str) -> bool {
    assignment_separator(line).is_some_and(|position| {
        let name = line[..position].trim();
        !name.is_empty() && !name.contains(':')
    })
}

fn assignment_separator(line: &str) -> Option<usize> {
    const SEPARATORS: [&str; 7] = [":::=", "::=", ":=", "?=", "+=", "!=", "="];

    SEPARATORS
        .iter()
        .filter_map(|separator| line.find(separator))
        .min()
}

fn is_rule(line: &str) -> bool {
    line.find(':').is_some_and(|colon| {
        let target = line[..colon].trim();
        !target.is_empty() && !target.contains('=')
    })
}
