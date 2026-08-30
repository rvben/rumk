//! Continuation-aware Make statements built on the lossless syntax tree.

use crate::syntax::{SourceSpan, SyntaxKind, SyntaxTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalKind {
    Ifdef,
    Ifndef,
    Ifeq,
    Ifneq,
    Else,
    Endif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalKind {
    Blank,
    Comment,
    Recipe,
    Assignment,
    Rule,
    Include(IncludeKind),
    Conditional(ConditionalKind),
    Define,
    DefineBody,
    Endef,
    Directive,
    Unknown,
}

/// One logical Make statement. Its span retains the exact source while `text`
/// contains the continuation-folded form used for structural parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalStatement {
    pub kind: LogicalKind,
    pub span: SourceSpan,
    pub start_line: usize,
    pub end_line: usize,
    text: String,
}

impl LogicalStatement {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn raw<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.byte_range()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalDocument {
    statements: Vec<LogicalStatement>,
}

impl LogicalDocument {
    pub fn parse(syntax: &SyntaxTree) -> Self {
        let nodes = syntax.nodes();
        let source = syntax.source();
        let mut statements = Vec::new();
        let mut index = 0;

        while index < nodes.len() {
            let first = &nodes[index];
            let first_kind = first.kind;
            let start = index;

            if can_continue(first_kind) {
                while index + 1 < nodes.len() && has_continuation(nodes[index].content(source)) {
                    index += 1;
                }
            }

            let last = &nodes[index];
            let text = if first_kind == SyntaxKind::Recipe {
                join_recipe_lines(&nodes[start..=index], source)
            } else {
                fold_lines(&nodes[start..=index], source)
            };
            let kind = classify(&text, first_kind);

            statements.push(LogicalStatement {
                kind,
                span: SourceSpan {
                    start: first.span.start,
                    end: last.span.end,
                },
                start_line: first.span.start.line,
                end_line: last.content_span.end.line,
                text,
            });
            index += 1;
        }

        Self { statements }
    }

    pub fn statements(&self) -> &[LogicalStatement] {
        &self.statements
    }
}

fn can_continue(kind: SyntaxKind) -> bool {
    !matches!(
        kind,
        SyntaxKind::DefineBody | SyntaxKind::Define | SyntaxKind::Endef
    )
}

fn has_continuation(line: &str) -> bool {
    line.as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn fold_lines(nodes: &[crate::syntax::SyntaxNode], source: &str) -> String {
    let mut folded = String::new();

    for (index, node) in nodes.iter().enumerate() {
        let content = node.content(source);
        let continued = index + 1 < nodes.len();
        let part = if continued {
            content
                .strip_suffix('\\')
                .expect("non-final logical line has a continuation")
                .trim_end()
        } else if index == 0 {
            content
        } else {
            content.trim_start()
        };
        if index == 0 {
            folded.push_str(part);
        } else {
            if !folded.is_empty() && !part.is_empty() {
                folded.push(' ');
            }
            folded.push_str(part.trim_start());
        }
    }

    folded
}

fn join_recipe_lines(nodes: &[crate::syntax::SyntaxNode], source: &str) -> String {
    nodes
        .iter()
        .map(|node| node.content(source))
        .collect::<Vec<_>>()
        .join("\n")
}

fn classify(text: &str, physical_kind: SyntaxKind) -> LogicalKind {
    match physical_kind {
        SyntaxKind::Blank => return LogicalKind::Blank,
        SyntaxKind::Comment => return LogicalKind::Comment,
        SyntaxKind::Recipe => return LogicalKind::Recipe,
        SyntaxKind::DefineBody => return LogicalKind::DefineBody,
        SyntaxKind::Endef => return LogicalKind::Endef,
        _ => {}
    }

    let trimmed = text.trim_start();
    let keyword = effective_keyword(trimmed);
    match keyword {
        Some("include") => return LogicalKind::Include(IncludeKind::Required),
        Some("-include" | "sinclude") => return LogicalKind::Include(IncludeKind::Optional),
        Some("ifdef") => return LogicalKind::Conditional(ConditionalKind::Ifdef),
        Some("ifndef") => return LogicalKind::Conditional(ConditionalKind::Ifndef),
        Some("ifeq") => return LogicalKind::Conditional(ConditionalKind::Ifeq),
        Some("ifneq") => return LogicalKind::Conditional(ConditionalKind::Ifneq),
        Some("else") => return LogicalKind::Conditional(ConditionalKind::Else),
        Some("endif") => return LogicalKind::Conditional(ConditionalKind::Endif),
        Some("define") => return LogicalKind::Define,
        Some("endef") => return LogicalKind::Endef,
        Some("undefine" | "vpath") => return LogicalKind::Directive,
        _ => {}
    }

    let assignment = find_top_level_assignment(trimmed);
    let rule = find_top_level_rule_separator(trimmed);
    if assignment.is_some_and(|(position, _)| rule.is_none_or(|rule| position <= rule.position)) {
        LogicalKind::Assignment
    } else if rule.is_some() {
        LogicalKind::Rule
    } else if starts_with_modifier(trimmed) {
        LogicalKind::Directive
    } else {
        LogicalKind::Unknown
    }
}

fn effective_keyword(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|word| !matches!(*word, "export" | "unexport" | "override" | "private"))
        .or_else(|| line.split_whitespace().next())
}

fn starts_with_modifier(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|word| matches!(word, "export" | "unexport" | "override" | "private"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuleSeparator {
    pub position: usize,
    pub length: usize,
    pub grouped: bool,
    pub double_colon: bool,
}

pub(crate) fn find_top_level_assignment(line: &str) -> Option<(usize, &'static str)> {
    const OPERATORS: [&str; 7] = [":::=", "::=", ":=", "?=", "+=", "!=", "="];

    top_level_indices(line)
        .into_iter()
        .filter_map(|position| {
            OPERATORS
                .iter()
                .find(|operator| line[position..].starts_with(**operator))
                .map(|operator| (position, *operator))
        })
        .next()
}

pub(crate) fn find_top_level_rule_separator(line: &str) -> Option<RuleSeparator> {
    for position in top_level_indices(line) {
        let suffix = &line[position..];
        let (length, grouped, double_colon) = if suffix.starts_with("&::") {
            (3, true, true)
        } else if suffix.starts_with("&:") {
            (2, true, false)
        } else if suffix.starts_with("::") {
            (2, false, true)
        } else if suffix.starts_with(':') {
            (1, false, false)
        } else {
            continue;
        };
        return Some(RuleSeparator {
            position,
            length,
            grouped,
            double_colon,
        });
    }
    None
}

pub(crate) fn find_top_level_char(line: &str, needle: char) -> Option<usize> {
    top_level_indices(line)
        .into_iter()
        .find(|position| line[*position..].starts_with(needle))
}

pub(crate) fn split_top_level_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut closers = Vec::new();
    let mut characters = line.chars().peekable();
    let mut escaped = false;

    while let Some(character) = characters.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }

        if character == '$' {
            current.push(character);
            if let Some(next) = characters.peek().copied() {
                if next == '$' {
                    current.push(next);
                    characters.next();
                    continue;
                }
                if matches!(next, '(' | '{') {
                    current.push(next);
                    closers.push(if next == '(' { ')' } else { '}' });
                    characters.next();
                    continue;
                }
            }
            continue;
        }

        if closers.last().copied() == Some(character) {
            closers.pop();
            current.push(character);
        } else if character.is_whitespace() && closers.is_empty() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }

    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Returns UTF-8 byte offsets that are outside Make variable/function
/// expansions and are not backslash-escaped.
fn top_level_indices(line: &str) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut closers = Vec::new();
    let mut characters = line.char_indices().peekable();
    let mut escaped = false;

    while let Some((index, character)) = characters.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }

        if character == '$' {
            if let Some((_, next)) = characters.peek().copied() {
                if next == '$' {
                    characters.next();
                    continue;
                }
                if matches!(next, '(' | '{') {
                    closers.push(if next == '(' { ')' } else { '}' });
                    characters.next();
                    continue;
                }
            }
        }

        if closers.last().copied() == Some(character) {
            closers.pop();
            continue;
        }
        if closers.is_empty() {
            indices.push(index);
        }
    }

    indices
}
