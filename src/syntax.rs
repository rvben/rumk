//! Lossless, source-ordered Makefile syntax.
//!
//! This module deliberately starts with physical-line nodes. GNU Make syntax is
//! highly contextual, so retaining every byte and assigning conservative kinds
//! gives later parser stages a dependable foundation without inventing syntax
//! that was not present in the input.

use crate::logical::{
    conditional_expression, strip_top_level_comment, ConditionalKind, Conditions, Reach,
};

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
    /// The recipe prefix in effect when the line was read.
    pub recipe_prefix: RecipePrefix,
}

/// The recipe prefix on a line, and whether Rumk knows the character GNU Make
/// reads there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipePrefix {
    /// The character Rumk reads recipe lines with. While `known` is false it
    /// is the prefix that holds if the statement Rumk could not follow left
    /// the variable alone.
    pub character: char,
    /// Whether `character` is the prefix GNU Make uses. A `.RECIPEPREFIX`
    /// value Rumk cannot evaluate, and one assigned in a branch Make decides
    /// at run time, leave it unknown.
    pub known: bool,
    /// Whether a value Rumk could not evaluate stands behind the prefix, which
    /// leaves every character possible, a space included. A prefix Rumk read
    /// from the source is never a space, because Make drops the whitespace
    /// between the assignment operator and the value.
    pub unevaluated: bool,
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
        let mut prefix = PrefixTracker::default();
        let mut conditions = Conditions::default();

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
            let recipe_prefix = prefix.current();
            let inside_define = define_depth > 0;
            let kind = classify(content, &mut define_depth, recipe_prefix.character);
            // The prefix is decided here, before conditionals are folded,
            // so a physical conditional line is tracked as far as it goes
            // to skip an assignment in a branch Make never reads.
            if !inside_define {
                if let Some(conditional) = conditional_kind(content, kind) {
                    conditions.track(conditional, conditional_expression(content));
                }
                let reach = conditions.reach();
                if reach != Reach::Never {
                    if let Some(effect) = recipe_prefix_effect(content, kind) {
                        prefix.apply(effect, reach);
                    }
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
                recipe_prefix,
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

    /// The recipe prefix in effect when `line` was read.
    pub fn recipe_prefix_at(&self, line: usize) -> RecipePrefix {
        line.checked_sub(1)
            .and_then(|index| self.nodes.get(index))
            .map_or_else(
                || PrefixTracker::default().current(),
                |node| node.recipe_prefix,
            )
    }

    /// Rendering an unmodified tree is byte-for-byte lossless.
    pub fn render(&self) -> &str {
        &self.source
    }
}

fn classify(line: &str, define_depth: &mut usize, recipe_prefix: char) -> SyntaxKind {
    let trimmed = line.trim_start();
    let keyword = directive_keyword(trimmed);

    // Inside a definition, Make nests on a line whose first word is `define`
    // and ends on one whose first word is `endef`, whatever follows the
    // word, unless the line starts with the recipe prefix.
    if *define_depth > 0 {
        let first_word = (!line.starts_with(recipe_prefix))
            .then(|| trimmed.split_whitespace().next())
            .flatten();
        return match first_word {
            Some("endef") => {
                *define_depth -= 1;
                SyntaxKind::Endef
            }
            Some("define") => {
                *define_depth += 1;
                SyntaxKind::Define
            }
            _ => SyntaxKind::DefineBody,
        };
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
        Some("define") if opens_define_block(trimmed) => {
            *define_depth = 1;
            return SyntaxKind::Define;
        }
        Some("endef") => return SyntaxKind::Endef,
        Some(
            "export" | "unexport" | "override" | "private" | "undefine" | "vpath" | "load"
            | "-load",
        ) => {
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

/// The kind of a conditional line, when Make reads `line` as one.
fn conditional_kind(line: &str, kind: SyntaxKind) -> Option<ConditionalKind> {
    if kind != SyntaxKind::Conditional {
        return None;
    }
    let trimmed = line.trim_start();
    if is_assignment(trimmed) {
        return None;
    }
    ConditionalKind::from_keyword(directive_keyword(trimmed)?)
}

/// `.RECIPEPREFIX` as far as Rumk can follow it while reading the source.
#[derive(Debug, Clone, Copy)]
struct PrefixTracker {
    /// The first character of the assigned value, or `None` while the
    /// variable is empty and Make falls back to a tab.
    assigned: Option<char>,
    /// Whether every statement that changed the variable so far told Rumk
    /// the resulting character.
    known: bool,
    /// Whether the value is simply expanded, which decides whether a later
    /// `+=` appends its text or the text it expands to. Make defines
    /// `.RECIPEPREFIX` simply expanded and empty.
    expanded: bool,
    /// Whether the value Make holds is one Rumk could not evaluate, which
    /// leaves the character it starts with open rather than merely undecided
    /// between the values Rumk did read.
    unevaluated: bool,
}

impl Default for PrefixTracker {
    fn default() -> Self {
        Self {
            assigned: None,
            known: true,
            expanded: true,
            unevaluated: false,
        }
    }
}

impl PrefixTracker {
    fn current(self) -> RecipePrefix {
        RecipePrefix {
            character: self.assigned.unwrap_or('\t'),
            known: self.known,
            unevaluated: self.unevaluated,
        }
    }

    fn apply(&mut self, effect: PrefixEffect, reach: Reach) {
        let before = *self;
        match effect {
            PrefixEffect::Set { first, expanded } => {
                self.assigned = first;
                self.known = true;
                self.expanded = expanded;
                self.unevaluated = false;
            }
            PrefixEffect::SetUnknown => {
                self.known = false;
                self.unevaluated = true;
            }
            PrefixEffect::Append(first) => {
                if self.known && self.assigned.is_none() {
                    // A simply expanded value expands what it appends, so a
                    // reference stands for text Rumk cannot read here.
                    if self.expanded && first == Some('$') {
                        self.known = false;
                        self.unevaluated = true;
                    } else {
                        self.assigned = first;
                    }
                }
            }
        }
        // A branch Make decides at run time may or may not have assigned the
        // prefix. Rumk reads the rest of the file with the value it found,
        // which is the one the author indented for, and keeps claiming to know
        // it only when both outcomes agree on the character.
        if reach == Reach::Conditional {
            if !before.known || self.assigned != before.assigned {
                self.known = false;
            }
            // The flavor is as undecided as the branch, and reading the value
            // as expanded is the outcome that never claims a character an
            // expansion could replace.
            self.expanded |= before.expanded;
            // A branch that was not taken leaves the value it found, so a value
            // Rumk could not evaluate is still one of the outcomes.
            self.unevaluated |= before.unevaluated;
        }
    }
}

/// What a statement does to `.RECIPEPREFIX`.
#[derive(Debug, Clone, Copy)]
enum PrefixEffect {
    /// Replaces the value with one Rumk read; `first` is `None` when the value
    /// is empty, which makes Make fall back to a tab, and `expanded` is the
    /// flavor the operator leaves behind.
    Set { first: Option<char>, expanded: bool },
    /// Replaces the value with one Rumk cannot evaluate.
    SetUnknown,
    /// Appends to the value, which sets the first character only while the
    /// variable is still empty.
    Append(Option<char>),
}

/// What `line` does to `.RECIPEPREFIX`, or `None` when it leaves the variable
/// alone. Make defines it empty and simply expanded at startup, so `?=` assigns
/// nothing. `=` keeps the value unexpanded, so text starting with a reference
/// makes `$` the prefix, while the operators that expand or run the value first
/// leave a character Rumk cannot read. An assignment qualified by targets, as in
/// `all: .RECIPEPREFIX = >`, defines a variable for those targets and never
/// changes how Make reads the file. Only an assignment updates the prefix Make
/// reads with, so `undefine .RECIPEPREFIX` leaves it exactly as it was.
fn recipe_prefix_effect(line: &str, kind: SyntaxKind) -> Option<PrefixEffect> {
    if !matches!(kind, SyntaxKind::Assignment | SyntaxKind::Define) {
        return None;
    }
    // Make strips the comment before storing the value, so
    // `.RECIPEPREFIX := # reset` empties it and the prefix returns to a tab.
    let trimmed = strip_top_level_comment(line).trim_start();

    if kind == SyntaxKind::Define {
        // A definition Rumk does not read line by line; its value is whatever
        // the body holds.
        let mut words = trimmed
            .split_whitespace()
            .skip_while(|word| is_modifier(word));
        let name = strip_assignment_operator(words.nth(1)?);
        return (name == ".RECIPEPREFIX").then_some(PrefixEffect::SetUnknown);
    }

    let separator = assignment_separator(trimmed)?;
    let mut named = trimmed[..separator].split_whitespace().rev();
    if named.next()? != ".RECIPEPREFIX" || !named.all(is_modifier) {
        return None;
    }
    let operator = ASSIGNMENT_OPERATORS
        .iter()
        .find(|operator| trimmed[separator..].starts_with(**operator))?;
    let first = first_character(trimmed[separator + operator.len()..].trim_start());
    Some(match *operator {
        "?=" => return None,
        "=" => PrefixEffect::Set {
            first,
            expanded: false,
        },
        "+=" => PrefixEffect::Append(first),
        // A shell command's output is never the text Rumk reads here.
        "!=" => PrefixEffect::SetUnknown,
        _ if first == Some('$') => PrefixEffect::SetUnknown,
        _ => PrefixEffect::Set {
            first,
            expanded: true,
        },
    })
}

/// The first character Make stores from an assigned value, reading `\#` as the
/// escaped `#` it stands for.
fn first_character(value: &str) -> Option<char> {
    match value.strip_prefix('\\') {
        Some(escaped) if escaped.starts_with('#') => Some('#'),
        _ => value.chars().next(),
    }
}

/// The name in `NAME=`, for a `define` line that carries an operator.
fn strip_assignment_operator(word: &str) -> &str {
    ASSIGNMENT_OPERATORS
        .iter()
        .find_map(|operator| word.strip_suffix(operator))
        .unwrap_or(word)
}

/// The assignment operators GNU Make recognizes, longest first so that a
/// prefix match finds the whole operator.
pub(crate) const ASSIGNMENT_OPERATORS: [&str; 7] = [":::=", "::=", ":=", "?=", "+=", "!=", "="];

/// Whether a word is a keyword Make allows before a variable name.
pub(crate) fn is_modifier(word: &str) -> bool {
    matches!(word, "export" | "unexport" | "override" | "private")
}

fn directive_keyword(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|word| !is_modifier(word))
        .or_else(|| line.split_whitespace().next())
}

fn starts_with_modifier(line: &str) -> bool {
    line.split_whitespace().next().is_some_and(is_modifier)
}

/// Make reads `define = 1` as an assignment to a variable named `define`.
/// Only a `define` followed by a name, or by nothing, opens a block.
fn opens_define_block(line: &str) -> bool {
    let mut words = line.split_whitespace().skip_while(|word| is_modifier(word));
    words.next();
    words.next().is_none_or(|next| {
        !ASSIGNMENT_OPERATORS
            .iter()
            .any(|operator| next.starts_with(operator))
    })
}

fn is_assignment(line: &str) -> bool {
    assignment_separator(line).is_some_and(|position| {
        let name = line[..position].trim();
        !name.is_empty() && !name.contains(':')
    })
}

fn assignment_separator(line: &str) -> Option<usize> {
    ASSIGNMENT_OPERATORS
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
