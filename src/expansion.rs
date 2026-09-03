//! Lexical structure of GNU Make expansions: where a `$(...)` or `${...}`
//! reference ends, which references never end at all, and which function
//! arguments are never expanded.
//!
//! The rules mirror `variable_expand` and `handle_function` in GNU Make.
//! A reference whose body starts with a function name counts nested
//! delimiters of the same kind. Any other reference ends at the first
//! matching delimiter, unless a `$` appears before that delimiter, in which
//! case Make counts nesting as well.

use std::ops::Range;

/// How deep a reference is followed into the references nested inside it.
/// Make itself has no limit, and neither does the text a Makefile may hold,
/// so every walk over nested references stops here rather than recursing as
/// deep as the file asks for.
pub const MAX_EXPANSION_DEPTH: usize = 64;

/// A `$(` or `${` whose closing delimiter never arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnterminatedReference {
    /// Byte offset of the `$` within the scanned text.
    pub offset: usize,
    /// The delimiter Make expected.
    pub closing: char,
    /// The function being called, when Make would report a function call.
    pub function: Option<String>,
}

/// Whether `#` starts a comment in the scanned text. Recipes and define
/// bodies keep `#` verbatim; every other line ends at the first unescaped
/// `#` outside a reference, as GNU Make 4.3 and later read it. Earlier
/// versions end the line at any `#`, so a `#` inside a reference that they
/// reject is accepted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentHandling {
    Strip,
    Keep,
}

/// Finds the first reference in `text` that Make would fail to expand.
/// References nested deeper than [`MAX_EXPANSION_DEPTH`] are left unread, so
/// a text that nests deeper than the stack allows is reported as far as it
/// was read rather than crashing the walk.
pub fn find_unterminated_reference(
    text: &str,
    comments: CommentHandling,
) -> Option<UnterminatedReference> {
    find_unterminated(text, comments, 0)
}

fn find_unterminated(
    text: &str,
    comments: CommentHandling,
    depth: usize,
) -> Option<UnterminatedReference> {
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'#' if comments == CommentHandling::Strip && !is_escaped(bytes, index) => {
                return None;
            }
            b'$' => {
                let &next = bytes.get(index + 1)?;
                match next {
                    b'$' => index += 2,
                    b'(' | b'{' => {
                        let opening = next as char;
                        let body_start = index + 2;
                        let Some(end) = reference_end(text, body_start, opening) else {
                            return Some(UnterminatedReference {
                                offset: index,
                                closing: closing_delimiter(opening),
                                function: function_name(&text[body_start..]).map(str::to_string),
                            });
                        };
                        if depth < MAX_EXPANSION_DEPTH {
                            if let Some(inner) = find_unterminated(
                                &text[body_start..end],
                                CommentHandling::Keep,
                                depth + 1,
                            ) {
                                return Some(UnterminatedReference {
                                    offset: body_start + inner.offset,
                                    ..inner
                                });
                            }
                        }
                        index = end + 1;
                    }
                    // A single-character reference such as `$X` or `$#`.
                    _ => index += 2,
                }
            }
            _ => index += 1,
        }
    }

    None
}

/// Returns the byte offset of the delimiter that closes the reference whose
/// body starts at `body_start`, following Make's expansion rules.
pub fn reference_end(text: &str, body_start: usize, opening: char) -> Option<usize> {
    let closing = closing_delimiter(opening);
    let body = &text[body_start..];
    if function_name(body).is_some() {
        return counted_end(body, opening, closing).map(|end| body_start + end);
    }
    let first = body.find(closing)?;
    if body[..first].contains('$') {
        counted_end(body, opening, closing).map(|end| body_start + end)
    } else {
        Some(body_start + first)
    }
}

/// Whether `text` holds a variable or function reference: a `$` followed by
/// anything but another `$`. `$$` is an escaped dollar sign, and a `$` at
/// the end of the text refers to nothing.
pub fn contains_reference(text: &str) -> bool {
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '$' {
            match characters.next() {
                Some('$') => {}
                Some(_) => return true,
                None => return false,
            }
        }
    }
    false
}

/// The byte length of the reference introduced by the `$` at `dollar`,
/// closing delimiter included: `$$` and a single-character reference such
/// as `$@` are two bytes, a `$` at the end of the text is one. `None` when
/// a parenthesized or braced reference stays open to the end of the text.
pub fn reference_length(text: &str, dollar: usize) -> Option<usize> {
    match text[dollar + 1..].chars().next() {
        None => Some(1),
        Some(opening @ ('(' | '{')) => {
            reference_end(text, dollar + 2, opening).map(|end| end + 1 - dollar)
        }
        Some(character) => Some(1 + character.len_utf8()),
    }
}

/// Byte ranges of the function arguments Make never expands because a
/// literal argument of the same call decides them: the branch of `$(if ...)`
/// its condition rules out, the arguments of `$(or ...)` after a non-empty
/// one and of `$(and ...)` after an empty one, and the body of a
/// `$(foreach ...)` over an empty list. An argument that holds a reference
/// is decided at run time, so it and everything after it count as expanded.
pub fn unexpanded_arguments(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut skipped = Vec::new();
    let mut index = 0;

    while index + 1 < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        match bytes[index + 1] {
            b'$' => index += 2,
            opening @ (b'(' | b'{') => {
                let body_start = index + 2;
                if let Some(end) = reference_end(text, body_start, opening as char) {
                    if let Some(name) = function_name(&text[body_start..end]) {
                        let after_name = body_start + name.len();
                        let arguments_start = end - text[after_name..end].trim_start().len();
                        skipped.extend(skipped_arguments(
                            name,
                            text,
                            arguments_start..end,
                            opening as char,
                        ));
                    }
                }
                // Calls nested in the body are visited in turn.
                index = body_start;
            }
            _ => index += 2,
        }
    }

    skipped
}

fn skipped_arguments(
    function: &str,
    text: &str,
    arguments: Range<usize>,
    opening: char,
) -> Vec<Range<usize>> {
    let literal = |argument: &Range<usize>| -> Option<bool> {
        let text = text[argument.clone()].trim();
        (!contains_reference(text)).then_some(text.is_empty())
    };
    match function {
        "if" => {
            let arguments = split_arguments(text, arguments, opening, 3);
            let Some(empty) = literal(&arguments[0]) else {
                return Vec::new();
            };
            let skipped = if empty { 1 } else { 2 };
            arguments.get(skipped).cloned().into_iter().collect()
        }
        "or" | "and" => {
            let arguments = split_arguments(text, arguments, opening, usize::MAX);
            let stops_when_empty = function == "and";
            for (position, argument) in arguments.iter().enumerate() {
                match literal(argument) {
                    None => break,
                    Some(empty) if empty == stops_when_empty => {
                        return arguments[position + 1..].to_vec();
                    }
                    Some(_) => {}
                }
            }
            Vec::new()
        }
        "foreach" => {
            let arguments = split_arguments(text, arguments, opening, 3);
            match arguments.get(1).and_then(literal) {
                Some(true) => arguments.get(2).cloned().into_iter().collect(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// Splits the arguments of a call at the commas outside nested delimiters
/// of the call's own kind, as Make does. Once `maximum` arguments are found,
/// the last one runs to the end of the call.
fn split_arguments(
    text: &str,
    arguments: Range<usize>,
    opening: char,
    maximum: usize,
) -> Vec<Range<usize>> {
    let closing = closing_delimiter(opening);
    let mut split = Vec::new();
    let mut depth = 0usize;
    let mut start = arguments.start;
    for (index, character) in text[arguments.clone()].char_indices() {
        let index = arguments.start + index;
        if character == opening {
            depth += 1;
        } else if character == closing {
            depth = depth.saturating_sub(1);
        } else if character == ',' && depth == 0 && split.len() + 1 < maximum {
            split.push(start..index);
            start = index + 1;
        }
    }
    split.push(start..arguments.end);
    split
}

/// The Make function a reference body invokes: its first word, when that word
/// is a function name followed by whitespace or the end of the body. A body
/// runs to the end of the text it was found in, so the first word is only
/// looked for as far as the longest function name reaches.
pub fn function_name(body: &str) -> Option<&str> {
    let head = body
        .as_bytes()
        .get(..=LONGEST_FUNCTION_NAME)
        .unwrap_or(body.as_bytes());
    let end = head
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(head.len());
    let name = body.get(..end)?;
    is_make_function(name).then_some(name)
}

pub fn closing_delimiter(opening: char) -> char {
    if opening == '(' {
        ')'
    } else {
        '}'
    }
}

/// Every function GNU Make expands, and the only words that can start a
/// reference body Make reads as a call.
const FUNCTION_NAMES: &[&str] = &[
    "subst",
    "patsubst",
    "strip",
    "findstring",
    "filter",
    "filter-out",
    "sort",
    "word",
    "wordlist",
    "words",
    "firstword",
    "lastword",
    "dir",
    "notdir",
    "suffix",
    "basename",
    "addsuffix",
    "addprefix",
    "join",
    "wildcard",
    "realpath",
    "abspath",
    "if",
    "or",
    "and",
    "intcmp",
    "foreach",
    "let",
    "file",
    "call",
    "value",
    "eval",
    "origin",
    "flavor",
    "shell",
    "error",
    "warning",
    "info",
    "guile",
];

/// The length of the longest name in [`FUNCTION_NAMES`], which bounds how far
/// [`function_name`] looks for the end of the first word.
const LONGEST_FUNCTION_NAME: usize = longest(FUNCTION_NAMES);

const fn longest(names: &[&str]) -> usize {
    let mut longest = 0;
    let mut index = 0;
    while index < names.len() {
        if names[index].len() > longest {
            longest = names[index].len();
        }
        index += 1;
    }
    longest
}

pub fn is_make_function(name: &str) -> bool {
    name.len() <= LONGEST_FUNCTION_NAME && FUNCTION_NAMES.contains(&name)
}

fn counted_end(body: &str, opening: char, closing: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in body.char_indices() {
        if character == opening {
            depth += 1;
        } else if character == closing {
            if depth == 0 {
                return Some(index);
            }
            depth -= 1;
        }
    }
    None
}

fn is_escaped(bytes: &[u8], index: usize) -> bool {
    bytes[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unterminated(text: &str) -> Option<(usize, Option<String>)> {
        find_unterminated_reference(text, CommentHandling::Strip)
            .map(|found| (found.offset, found.function))
    }

    fn call(offset: usize, function: &str) -> Option<(usize, Option<String>)> {
        Some((offset, Some(function.to_string())))
    }

    #[test]
    fn plain_references_end_at_the_first_matching_delimiter() {
        assert_eq!(unterminated("$(FOO) $(BAR)"), None);
        assert_eq!(unterminated("$(A (b)"), None);
        assert_eq!(unterminated("$(shell)"), None);
        assert_eq!(unterminated("$(FOO"), Some((0, None)));
        assert_eq!(unterminated("x ${FOO"), Some((2, None)));
        assert_eq!(unterminated("$(FOO}"), Some((0, None)));
    }

    #[test]
    fn function_calls_count_nested_delimiters() {
        assert_eq!(unterminated("$(shell echo \"(\")"), call(0, "shell"));
        assert_eq!(unterminated("$(shell echo \")\")"), None);
        assert_eq!(unterminated("$(if $(A),(x),y)"), None);
        assert_eq!(unterminated("$(info $(FOO)"), call(0, "info"));
    }

    #[test]
    fn reference_length_spans_the_whole_reference() {
        assert_eq!(reference_length("$@ x", 0), Some(2));
        assert_eq!(reference_length("$$ x", 0), Some(2));
        assert_eq!(reference_length("$é x", 0), Some(3));
        assert_eq!(reference_length("x $", 2), Some(1));
        assert_eq!(reference_length("$(FOO) x", 0), Some(6));
        assert_eq!(reference_length("${FOO} x", 0), Some(6));
        assert_eq!(reference_length("$(A (b) x", 0), Some(7));
        assert_eq!(reference_length("$(if x,(a):b) x", 0), Some(13));
        assert_eq!(reference_length("$(A$(B) (x)) y", 0), Some(12));
        assert_eq!(reference_length("$(FOO", 0), None);
        assert_eq!(reference_length("$(if x,(a)", 0), None);
    }

    #[test]
    fn a_dollar_before_the_first_closer_switches_to_counting() {
        assert_eq!(unterminated("$(A$(B) (x)"), Some((0, None)));
        assert_eq!(unterminated("$(A$(B) (x))"), None);
    }

    #[test]
    fn nested_bodies_are_scanned() {
        assert_eq!(unterminated("$(if x,$(FOO,y)"), call(0, "if"));
        assert_eq!(unterminated("$(A $(B)"), Some((0, None)));
        assert_eq!(unterminated("$(if x,${B)"), Some((7, None)));
    }

    #[test]
    fn escaped_and_trailing_dollars_are_literal() {
        assert_eq!(unterminated("echo '$$(' done"), None);
        assert_eq!(unterminated("trailing $"), None);
        assert_eq!(unterminated("$( "), Some((0, None)));
    }

    #[test]
    fn comments_end_the_scan_unless_kept() {
        assert_eq!(unterminated("X # $(unclosed"), None);
        assert_eq!(unterminated("X \\# $(unclosed"), Some((5, None)));
        assert_eq!(
            find_unterminated_reference("echo # $(unclosed", CommentHandling::Keep)
                .map(|found| found.offset),
            Some(7)
        );
        assert_eq!(unterminated("$(shell echo # hi)"), None);
        assert_eq!(unterminated("$# $(unclosed"), Some((3, None)));
    }

    /// The skipped argument ranges of `text` as (start, end) pairs.
    fn skipped(text: &str) -> Vec<(usize, usize)> {
        unexpanded_arguments(text)
            .into_iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    #[test]
    fn a_literal_condition_decides_which_branch_of_if_is_expanded() {
        assert_eq!(skipped("$(if ,$(A),$(B))"), [(6, 10)]);
        assert_eq!(skipped("$(if  ,$(A),$(B))"), [(7, 11)]);
        assert_eq!(skipped("$(if x,$(A),$(B))"), [(12, 16)]);
        assert_eq!(skipped("$(if x,$(A))"), []);
        assert_eq!(skipped("$(if $(C),$(A),$(B))"), []);
        assert_eq!(skipped("$(if ,$(A),(b),c)"), [(6, 10)]);
        assert_eq!(skipped("${if ,$(A),(b)}"), [(6, 10)]);
    }

    #[test]
    fn a_literal_argument_stops_or_and_and() {
        assert_eq!(skipped("$(or ok,$(A),$(B))"), [(8, 12), (13, 17)]);
        assert_eq!(skipped("$(or ,$(A),$(B))"), []);
        assert_eq!(skipped("$(or $(C),$(A))"), []);
        assert_eq!(skipped("$(and ,$(A))"), [(7, 11)]);
        assert_eq!(skipped("$(and x,$(A))"), []);
        assert_eq!(skipped("$(and $(C),$(A))"), []);
    }

    #[test]
    fn a_literal_empty_list_skips_the_body_of_foreach() {
        assert_eq!(skipped("$(foreach v,,$(A))"), [(13, 17)]);
        assert_eq!(skipped("$(foreach v,a b,$(A))"), []);
        assert_eq!(skipped("$(foreach v,$(L),$(A))"), []);
    }

    #[test]
    fn nested_and_escaped_calls_are_handled() {
        assert_eq!(skipped("$(if x,y,$(if ,$(A),z))"), [(9, 22), (15, 19)]);
        assert_eq!(skipped("$$(if ,$(A),b)"), []);
        assert_eq!(skipped("$(if ,$(A),b"), []);
        assert_eq!(skipped("$(subst ,$(A),b)"), []);
    }
}
