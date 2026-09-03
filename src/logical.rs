//! Continuation-aware Make statements built on the lossless syntax tree.

use crate::eval::parse_comparison;
use crate::expansion::reference_length;
use crate::syntax::{
    is_modifier, LineEnding, SourceSpan, SyntaxKind, SyntaxNode, SyntaxTree, ASSIGNMENT_OPERATORS,
};

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

impl ConditionalKind {
    pub(crate) fn from_keyword(word: &str) -> Option<Self> {
        match word {
            "ifdef" => Some(Self::Ifdef),
            "ifndef" => Some(Self::Ifndef),
            "ifeq" => Some(Self::Ifeq),
            "ifneq" => Some(Self::Ifneq),
            "else" => Some(Self::Else),
            "endif" => Some(Self::Endif),
            _ => None,
        }
    }

    /// Whether the keyword opens a conditional block.
    pub(crate) fn opens_block(self) -> bool {
        !matches!(self, Self::Else | Self::Endif)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalKind {
    Blank,
    Comment,
    Recipe,
    /// A recipe-prefixed line with no rule to attach to. GNU Make reads it as
    /// an ordinary line, prefix included, and rejects it when it is not a
    /// directive or an assignment.
    OrphanRecipe,
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

/// Whether GNU Make reads a statement, decided from the literal
/// conditionals around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Outside every conditional, or inside branches whose literal
    /// conditions hold.
    Always,
    /// Inside a branch GNU Make decides at run time.
    Conditional,
    /// Inside a branch whose literal condition fails.
    Never,
}

/// One logical Make statement. Its span retains the exact source while `text`
/// contains the continuation-folded form used for structural parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalStatement {
    pub kind: LogicalKind,
    /// Whether GNU Make reads the statement.
    pub reach: Reach,
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
    /// Groups physical lines into statements and classifies each one the way
    /// GNU Make reads it. Recipe-prefixed lines are recipes only while a rule
    /// is pending: a rule line opens the pending state and any assignment,
    /// directive, include, define, or unrecognized line closes it. A line in
    /// a branch GNU Make never reads does neither.
    pub fn parse(syntax: &SyntaxTree) -> Self {
        let nodes = syntax.nodes();
        let source = syntax.source();
        let mut statements = Vec::new();
        let mut index = 0;
        let mut rule_pending = false;
        let mut conditions = Conditions::default();

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
            let group = &nodes[start..=index];
            let (kind, text) = if first_kind == SyntaxKind::Recipe && rule_pending {
                (LogicalKind::Recipe, join_recipe_lines(group, source))
            } else {
                let dangling = index + 1 == nodes.len()
                    && can_continue(first_kind)
                    && last.line_ending != LineEnding::None
                    && has_continuation(last.content(source));
                let text = fold_lines(group, source, dangling);
                let kind = if first_kind == SyntaxKind::Recipe {
                    classify_orphan(&text)
                } else {
                    match classify(&text, first_kind) {
                        LogicalKind::Unknown if rule_pending && text.starts_with(' ') => {
                            LogicalKind::Recipe
                        }
                        kind => kind,
                    }
                };
                (kind, text)
            };

            let reach = match kind {
                LogicalKind::Conditional(kind) => {
                    let expression = conditional_expression(&text);
                    let reach = conditions.reach_of_conditional(kind, expression);
                    conditions.track(kind, expression);
                    reach
                }
                _ => conditions.reach(),
            };

            if reach != Reach::Never {
                rule_pending = match kind {
                    LogicalKind::Rule => !defines_target_variable(&text),
                    LogicalKind::Assignment
                    | LogicalKind::Include(_)
                    | LogicalKind::Directive
                    | LogicalKind::Define
                    | LogicalKind::Unknown
                    | LogicalKind::OrphanRecipe => false,
                    LogicalKind::Blank
                    | LogicalKind::Comment
                    | LogicalKind::Recipe
                    | LogicalKind::Conditional(_)
                    | LogicalKind::DefineBody
                    | LogicalKind::Endef => rule_pending,
                };
            }

            statements.push(LogicalStatement {
                kind,
                reach,
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

/// Activity of one conditional branch, decided from literal `ifeq` and
/// `ifneq` arguments alone. `None` leaves the decision to Make at run time.
struct ConditionFrame {
    active: Option<bool>,
    taken: Option<bool>,
}

/// The conditional blocks open at the current statement.
#[derive(Default)]
pub(crate) struct Conditions {
    frames: Vec<ConditionFrame>,
}

impl Conditions {
    /// Whether Make reads a statement inside every open block.
    pub(crate) fn reach(&self) -> Reach {
        reach_within(&self.frames)
    }

    /// Whether Make reads a conditional line. An `else` line belongs to the
    /// block around the conditional, not to the branch it closes, so Make
    /// reads it even after a branch it skipped; but once a branch of the
    /// block has been taken, the condition of a following `else if...` is
    /// skipped unread.
    fn reach_of_conditional(&self, kind: ConditionalKind, expression: &str) -> Reach {
        if kind != ConditionalKind::Else {
            return self.reach();
        }
        let Some((innermost, enclosing)) = self.frames.split_last() else {
            return self.reach();
        };
        if innermost.taken == Some(true) && else_condition(expression).is_some() {
            return Reach::Never;
        }
        reach_within(enclosing)
    }

    pub(crate) fn track(&mut self, kind: ConditionalKind, expression: &str) {
        match kind {
            ConditionalKind::Ifdef
            | ConditionalKind::Ifndef
            | ConditionalKind::Ifeq
            | ConditionalKind::Ifneq => {
                let active = literal_truth(kind, expression);
                self.frames.push(ConditionFrame {
                    active,
                    taken: active,
                });
            }
            ConditionalKind::Else => {
                let Some(frame) = self.frames.last_mut() else {
                    return;
                };
                let untaken = frame.taken.map(|taken| !taken);
                frame.active = match else_condition(expression) {
                    Some((kind, expression)) => and(untaken, literal_truth(kind, expression)),
                    None => untaken,
                };
                frame.taken = or(frame.taken, frame.active);
            }
            ConditionalKind::Endif => {
                self.frames.pop();
            }
        }
    }
}

fn reach_within(frames: &[ConditionFrame]) -> Reach {
    let mut reach = Reach::Always;
    for frame in frames {
        match frame.active {
            Some(false) => return Reach::Never,
            None => reach = Reach::Conditional,
            Some(true) => {}
        }
    }
    reach
}

/// The condition of a conditional line: the text after its keyword, with
/// the comment removed, as Make reads it.
pub(crate) fn conditional_expression(text: &str) -> &str {
    let trimmed = text.trim_start();
    let keyword_length = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    strip_top_level_comment(&trimmed[keyword_length..]).trim()
}

/// Splits an `else` expression into its `else if...` continuation, if any.
fn else_condition(expression: &str) -> Option<(ConditionalKind, &str)> {
    let keyword_length = expression
        .find(char::is_whitespace)
        .unwrap_or(expression.len());
    let kind = ConditionalKind::from_keyword(&expression[..keyword_length])
        .filter(|kind| kind.opens_block())?;
    Some((kind, expression[keyword_length..].trim()))
}

/// Decides a condition from literal text alone. Anything involving a
/// variable stays undecided.
fn literal_truth(kind: ConditionalKind, expression: &str) -> Option<bool> {
    let negated = match kind {
        ConditionalKind::Ifeq => false,
        ConditionalKind::Ifneq => true,
        _ => return None,
    };
    let (left, right) = parse_comparison(expression)?;
    if left.contains('$') || right.contains('$') {
        return None;
    }
    Some((left == right) != negated)
}

fn and(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn or(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
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

/// Folds continuation lines the way GNU Make does: the backslash and the
/// blanks around the line break collapse into one space. A continuation on
/// the final line of the file (`dangling`) continues into nothing, so its
/// backslash is dropped as well.
fn fold_lines(nodes: &[SyntaxNode], source: &str, dangling: bool) -> String {
    let mut folded = String::new();

    for (index, node) in nodes.iter().enumerate() {
        let content = node.content(source);
        let continued = index + 1 < nodes.len() || dangling;
        let part = if continued {
            content
                .strip_suffix('\\')
                .expect("continued line ends with a backslash")
                .trim_end()
        } else {
            content
        };
        if index == 0 {
            folded.push_str(part);
        } else {
            let part = part.trim_start();
            if !folded.is_empty() && !part.is_empty() {
                folded.push(' ');
            }
            folded.push_str(part);
        }
    }

    folded
}

fn join_recipe_lines(nodes: &[SyntaxNode], source: &str) -> String {
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
        // The syntax tree already tells a nested `define` inside a body,
        // which Make opens on the word alone, from a top-level `define = 1`.
        SyntaxKind::Define => return LogicalKind::Define,
        SyntaxKind::DefineBody => return LogicalKind::DefineBody,
        SyntaxKind::Endef => return LogicalKind::Endef,
        _ => {}
    }

    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return LogicalKind::Blank;
    }

    // Make strips the comment before looking for a separator, so `X # = 1`
    // is neither an assignment nor a rule, and it looks for an assignment
    // before any keyword, so `ifdef = 1` assigns a variable named `ifdef`.
    let code = strip_top_level_comment(trimmed);
    let assignment = variable_assignment(code);
    let rule = find_top_level_rule_separator(code);
    if assignment.is_some_and(|(position, _)| rule.is_none_or(|rule| position <= rule.position)) {
        return LogicalKind::Assignment;
    }

    let keyword = effective_keyword(code);
    if let Some(kind) = keyword.and_then(ConditionalKind::from_keyword) {
        return LogicalKind::Conditional(kind);
    }
    match keyword {
        Some("include") => LogicalKind::Include(IncludeKind::Required),
        Some("-include" | "sinclude") => LogicalKind::Include(IncludeKind::Optional),
        Some("define") => LogicalKind::Define,
        Some("endef") => LogicalKind::Endef,
        Some("undefine" | "vpath" | "load" | "-load") => LogicalKind::Directive,
        _ if rule.is_some() => LogicalKind::Rule,
        // `export` and `unexport` take bare variable names, or nothing at
        // all. `override` and `private` only qualify an assignment, a
        // `define`, or an `undefine`, so a line that starts with one of
        // them and holds none of those is not a statement.
        _ if starts_with_export(code) => LogicalKind::Directive,
        _ => LogicalKind::Unknown,
    }
}

/// Classifies a recipe-prefixed line that no rule can claim. Make keeps the
/// prefix character in the line, so a rule is as fatal as text it cannot
/// parse; assignments and directives are read as written, and a line that
/// folds to nothing but whitespace is ignored.
fn classify_orphan(text: &str) -> LogicalKind {
    match classify(text, SyntaxKind::Unknown) {
        LogicalKind::Rule | LogicalKind::Unknown => LogicalKind::OrphanRecipe,
        kind => kind,
    }
}

fn effective_keyword(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|word| !is_modifier(word))
        .or_else(|| line.split_whitespace().next())
}

fn starts_with_export(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|word| matches!(word, "export" | "unexport"))
}

/// Locates the operator of a variable assignment. Make reads a name up to
/// the first whitespace outside a reference and expects the operator next,
/// so `two words = 1` is not an assignment while `$(two words) = 1` is.
pub(crate) fn variable_assignment(line: &str) -> Option<(usize, &'static str)> {
    find_top_level_assignment(line).filter(|(position, _)| names_one_variable(&line[..*position]))
}

fn names_one_variable(left_hand_side: &str) -> bool {
    let mut name = left_hand_side.trim();
    while let Some((word, rest)) = name.split_once(char::is_whitespace) {
        if !is_modifier(word) {
            return !has_whitespace_outside_references(name);
        }
        name = rest.trim_start();
    }
    true
}

fn has_whitespace_outside_references(name: &str) -> bool {
    let mut characters = name.chars();
    while let Some(character) = characters.next() {
        if character.is_whitespace() {
            return true;
        }
        if character != '$' {
            continue;
        }
        // `$$`, `$x`, `$(...)`, and `${...}` all belong to the name. Make
        // skips to the matching closer, counting nested openers of its kind.
        let Some(opener @ ('(' | '{')) = characters.next() else {
            continue;
        };
        let closer = if opener == '(' { ')' } else { '}' };
        let mut depth = 1usize;
        for inner in characters.by_ref() {
            if inner == opener {
                depth += 1;
            } else if inner == closer {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }
    false
}

/// Whether a rule line assigns a target-specific variable. Make decides this
/// on the prerequisite part, after cutting the inline recipe and comment, and
/// only when the assignment precedes any static-pattern separator.
fn defines_target_variable(rule_text: &str) -> bool {
    let trimmed = rule_text.trim_start();
    let Some(separator) = find_top_level_rule_separator(trimmed) else {
        return false;
    };
    let body = &trimmed[separator.position + separator.length..];
    target_assignment(strip_top_level_comment(split_once_top_level(body, ';').0)).is_some()
}

/// Locates the assignment operator that turns a prerequisite list into a
/// target-specific variable definition.
pub(crate) fn target_assignment(prerequisites: &str) -> Option<(usize, &'static str)> {
    let assignment = variable_assignment(prerequisites)?;
    let pattern = find_top_level_rule_separator(prerequisites);
    pattern
        .is_none_or(|pattern| assignment.0 <= pattern.position)
        .then_some(assignment)
}

pub(crate) fn strip_top_level_comment(line: &str) -> &str {
    split_once_top_level(line, '#').0
}

/// Locates the `;` that starts an inline recipe. A `#` before it turns the
/// rest of the line, semicolon included, into a comment.
pub(crate) fn inline_recipe_separator(rule_body: &str) -> Option<usize> {
    let comment = find_top_level_char(rule_body, '#');
    find_top_level_char(rule_body, ';')
        .filter(|semicolon| comment.is_none_or(|comment| *semicolon < comment))
}

pub(crate) fn split_once_top_level(line: &str, separator: char) -> (&str, Option<&str>) {
    if let Some(index) = find_top_level_char(line, separator) {
        let after = index + separator.len_utf8();
        (&line[..index], Some(&line[after..]))
    } else {
        (line, None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuleSeparator {
    pub position: usize,
    pub length: usize,
    pub grouped: bool,
    pub double_colon: bool,
}

pub(crate) fn find_top_level_assignment(line: &str) -> Option<(usize, &'static str)> {
    top_level_indices(line)
        .into_iter()
        .filter_map(|position| {
            ASSIGNMENT_OPERATORS
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

/// Splits a line at whitespace outside references. A reference is kept
/// whole, so `$(if a,(x) y,z)` is one word, and an unterminated one runs to
/// the end of the line.
pub(crate) fn split_top_level_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut skip_to = 0;

    for (index, character) in line.char_indices() {
        if index < skip_to {
            continue;
        }
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '$' => {
                let end = reference_length(line, index).map_or(line.len(), |length| index + length);
                current.push_str(&line[index..end]);
                skip_to = end;
            }
            _ if character.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
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

/// Splits the file list of an include directive where GNU Make splits it,
/// leaving the escapes Make only removes once it has expanded the list.
///
/// A blank ends a path when the run of backslashes before it is even, and that
/// run halves here because the blank it belonged to is gone. An odd run keeps
/// the blank inside the path, so both are left as they are for the pass after
/// expansion to read once.
///
/// The comment goes here as well, because Make removes it before it looks at
/// the directive at all, and halves the run of backslashes in front of the `#`
/// as it recognizes it. `line` is therefore the text after the keyword with
/// its comment still on it.
pub(crate) fn split_include_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut backslashes = 0usize;
    let mut skip_to = 0;

    for (index, character) in line.char_indices() {
        if index < skip_to {
            continue;
        }
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        match character {
            '$' => {
                current.extend(std::iter::repeat_n('\\', backslashes));
                let end = reference_length(line, index).map_or(line.len(), |length| index + length);
                current.push_str(&line[index..end]);
                skip_to = end;
            }
            '#' => {
                current.extend(std::iter::repeat_n('\\', backslashes / 2));
                if backslashes % 2 == 0 {
                    // The run quoted itself rather than the `#`, so the
                    // comment starts here and the path ends with it.
                    backslashes = 0;
                    break;
                }
                current.push(character);
            }
            _ if character.is_whitespace() => {
                if backslashes % 2 == 1 {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                    current.push(character);
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                    }
                }
            }
            _ => {
                current.extend(std::iter::repeat_n('\\', backslashes));
                current.push(character);
            }
        }
        backslashes = 0;
    }

    current.extend(std::iter::repeat_n('\\', backslashes));
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Returns UTF-8 byte offsets that are outside Make variable/function
/// expansions and are not backslash-escaped. Every `$` opens a reference,
/// so `$:` and `$#` hold no separator, and nothing after an unterminated
/// reference is at the top level.
fn top_level_indices(line: &str) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut escaped = false;
    let mut skip_to = 0;

    for (index, character) in line.char_indices() {
        if index < skip_to {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '$' => match reference_length(line, index) {
                Some(length) => skip_to = index + length,
                None => break,
            },
            _ => indices.push(index),
        }
    }

    indices
}
