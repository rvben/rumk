//! Safe, side-effect-free evaluation of the statically knowable Make subset.

use std::collections::{BTreeMap, BTreeSet};

use crate::logical::ConditionalKind;
use crate::parser::{AssignmentOperator, Variable};
use crate::project::SourceId;

const MAX_EXPANSION_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Truth {
    False,
    Unknown,
    True,
}

impl Truth {
    pub fn negate(self) -> Self {
        match self {
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
            Self::True => Self::False,
        }
    }

    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    pub fn is_true(self) -> bool {
        self == Self::True
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EvaluationLocation {
    pub source: SourceId,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraceStep {
    pub variable: String,
    pub origin: Option<EvaluationLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockedReason {
    UndefinedVariable(String),
    DynamicVariableName(String),
    RecursiveReference(String),
    UnsafeFunction(String),
    UnsupportedFunction(String),
    MalformedExpansion,
    ExpansionLimit,
    IndeterminateAssignment(String),
    ShellAssignment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    pub value: Option<String>,
    pub trace: Vec<TraceStep>,
    pub blocked: BTreeSet<BlockedReason>,
}

impl Expansion {
    pub fn known(value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            trace: Vec::new(),
            blocked: BTreeSet::new(),
        }
    }

    pub fn unknown(reason: BlockedReason) -> Self {
        Self {
            value: None,
            trace: Vec::new(),
            blocked: BTreeSet::from([reason]),
        }
    }

    pub fn as_known(&self) -> Option<&str> {
        self.value.as_deref()
    }

    fn merge_unknown(&mut self, other: Self) {
        self.trace.extend(other.trace);
        self.blocked.extend(other.blocked);
        self.value = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableFlavor {
    Recursive,
    Simple,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoredValue {
    Recursive(String),
    Simple(String),
    Unknown(BlockedReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VariableState {
    value: StoredValue,
    origin: Option<EvaluationLocation>,
    command_line: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Evaluator {
    variables: BTreeMap<String, VariableState>,
}

impl Evaluator {
    pub fn new(predefined: &BTreeMap<String, String>) -> Self {
        Self {
            variables: predefined
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        VariableState {
                            value: StoredValue::Simple(value.clone()),
                            origin: None,
                            command_line: true,
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn expand(&self, input: &str) -> Expansion {
        self.expand_inner(input, 0, &mut Vec::new())
    }

    pub fn assign(&mut self, variable: &Variable, location: EvaluationLocation, activity: Truth) {
        if activity == Truth::False {
            return;
        }
        if activity == Truth::Unknown {
            self.variables.insert(
                variable.name.clone(),
                VariableState {
                    value: StoredValue::Unknown(BlockedReason::IndeterminateAssignment(
                        variable.name.clone(),
                    )),
                    origin: Some(location),
                    command_line: false,
                },
            );
            return;
        }

        let protected = self
            .variables
            .get(&variable.name)
            .is_some_and(|state| state.command_line && !variable.modifiers.override_);
        if protected {
            return;
        }

        match variable.operator {
            AssignmentOperator::Conditional if self.variables.contains_key(&variable.name) => {}
            AssignmentOperator::Append => self.append(variable, location),
            AssignmentOperator::Shell => {
                self.variables.insert(
                    variable.name.clone(),
                    VariableState {
                        value: StoredValue::Unknown(BlockedReason::ShellAssignment(
                            variable.name.clone(),
                        )),
                        origin: Some(location),
                        command_line: false,
                    },
                );
            }
            AssignmentOperator::Simple | AssignmentOperator::SimplePosix => {
                let expansion = self.expand(&variable.value);
                self.store_expansion(&variable.name, expansion, location);
            }
            AssignmentOperator::ImmediateRecursive if variable.value.contains('$') => {
                self.variables.insert(
                    variable.name.clone(),
                    VariableState {
                        value: StoredValue::Unknown(BlockedReason::UnsupportedFunction(
                            ":::=".into(),
                        )),
                        origin: Some(location),
                        command_line: false,
                    },
                );
            }
            AssignmentOperator::Recursive
            | AssignmentOperator::Conditional
            | AssignmentOperator::ImmediateRecursive => {
                self.variables.insert(
                    variable.name.clone(),
                    VariableState {
                        value: StoredValue::Recursive(variable.value.clone()),
                        origin: Some(location),
                        command_line: false,
                    },
                );
            }
        }
    }

    pub fn undefine(&mut self, name: &str, activity: Truth) {
        match activity {
            Truth::False => {}
            Truth::True => {
                self.variables.remove(name);
            }
            Truth::Unknown => {
                self.variables.insert(
                    name.to_string(),
                    VariableState {
                        value: StoredValue::Unknown(BlockedReason::IndeterminateAssignment(
                            name.to_string(),
                        )),
                        origin: None,
                        command_line: false,
                    },
                );
            }
        }
    }

    pub fn condition(&self, kind: ConditionalKind, expression: &str) -> Truth {
        match kind {
            ConditionalKind::Ifdef | ConditionalKind::Ifndef => {
                let name = self.expand(expression.trim());
                let truth = match name.as_known() {
                    Some(name) => self.variable_nonempty(name.trim()),
                    None => Truth::Unknown,
                };
                if kind == ConditionalKind::Ifndef {
                    truth.negate()
                } else {
                    truth
                }
            }
            ConditionalKind::Ifeq | ConditionalKind::Ifneq => {
                let Some((left, right)) = parse_comparison(expression) else {
                    return Truth::Unknown;
                };
                let left = self.expand(left);
                let right = self.expand(right);
                let truth = match (left.as_known(), right.as_known()) {
                    (Some(left), Some(right)) if left == right => Truth::True,
                    (Some(_), Some(_)) => Truth::False,
                    _ => Truth::Unknown,
                };
                if kind == ConditionalKind::Ifneq {
                    truth.negate()
                } else {
                    truth
                }
            }
            ConditionalKind::Else | ConditionalKind::Endif => Truth::Unknown,
        }
    }

    pub fn flavor(&self, name: &str) -> Option<VariableFlavor> {
        self.variables.get(name).map(|state| match state.value {
            StoredValue::Recursive(_) => VariableFlavor::Recursive,
            StoredValue::Simple(_) => VariableFlavor::Simple,
            StoredValue::Unknown(_) => VariableFlavor::Unknown,
        })
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    fn append(&mut self, variable: &Variable, location: EvaluationLocation) {
        let Some(existing) = self.variables.get(&variable.name).cloned() else {
            self.variables.insert(
                variable.name.clone(),
                VariableState {
                    value: StoredValue::Recursive(variable.value.clone()),
                    origin: Some(location),
                    command_line: false,
                },
            );
            return;
        };
        let value = match existing.value {
            StoredValue::Recursive(mut value) => {
                append_with_space(&mut value, &variable.value);
                StoredValue::Recursive(value)
            }
            StoredValue::Simple(mut value) => {
                let expansion = self.expand(&variable.value);
                if let Some(addition) = expansion.value {
                    append_with_space(&mut value, &addition);
                    StoredValue::Simple(value)
                } else {
                    StoredValue::Unknown(
                        expansion
                            .blocked
                            .into_iter()
                            .next()
                            .unwrap_or(BlockedReason::MalformedExpansion),
                    )
                }
            }
            StoredValue::Unknown(reason) => StoredValue::Unknown(reason),
        };
        self.variables.insert(
            variable.name.clone(),
            VariableState {
                value,
                origin: Some(location),
                command_line: false,
            },
        );
    }

    fn store_expansion(&mut self, name: &str, expansion: Expansion, location: EvaluationLocation) {
        let value = expansion.value.map_or_else(
            || {
                StoredValue::Unknown(
                    expansion
                        .blocked
                        .into_iter()
                        .next()
                        .unwrap_or(BlockedReason::MalformedExpansion),
                )
            },
            StoredValue::Simple,
        );
        self.variables.insert(
            name.to_string(),
            VariableState {
                value,
                origin: Some(location),
                command_line: false,
            },
        );
    }

    fn variable_nonempty(&self, name: &str) -> Truth {
        match self.variables.get(name).map(|state| &state.value) {
            Some(StoredValue::Recursive(value) | StoredValue::Simple(value)) => {
                if value.is_empty() {
                    Truth::False
                } else {
                    Truth::True
                }
            }
            Some(StoredValue::Unknown(_)) | None => Truth::Unknown,
        }
    }

    fn expand_inner(&self, input: &str, depth: usize, stack: &mut Vec<String>) -> Expansion {
        if depth >= MAX_EXPANSION_DEPTH {
            return Expansion::unknown(BlockedReason::ExpansionLimit);
        }
        let mut output = String::new();
        let mut result = Expansion::known("");
        let mut characters = input.char_indices().peekable();
        while let Some((index, character)) = characters.next() {
            if character != '$' {
                output.push(character);
                continue;
            }
            let Some((next_index, next)) = characters.next() else {
                output.push('$');
                continue;
            };
            if next == '$' {
                output.push('$');
                continue;
            }
            let expansion = if matches!(next, '(' | '{') {
                let closing = if next == '(' { ')' } else { '}' };
                let body_start = next_index + next.len_utf8();
                let Some(end) = matching_delimiter(input, body_start, closing) else {
                    result.merge_unknown(Expansion::unknown(BlockedReason::MalformedExpansion));
                    break;
                };
                while characters
                    .peek()
                    .is_some_and(|(position, _)| *position <= end)
                {
                    characters.next();
                }
                self.expand_body(&input[body_start..end], depth + 1, stack)
            } else {
                self.expand_variable(&next.to_string(), depth + 1, stack)
            };
            if let Some(value) = &expansion.value {
                output.push_str(value);
                result.trace.extend(expansion.trace);
            } else {
                result.merge_unknown(expansion);
            }
            let _ = index;
        }
        if result.value.is_some() {
            result.value = Some(output);
        }
        result
    }

    fn expand_body(&self, body: &str, depth: usize, stack: &mut Vec<String>) -> Expansion {
        let trimmed = body.trim_start();
        let head_end = trimmed
            .find(|character: char| character.is_whitespace() || character == ',')
            .unwrap_or(trimmed.len());
        let head = &trimmed[..head_end];
        let function_invocation = head_end < trimmed.len();
        if function_invocation && is_unsafe_function(head) {
            return Expansion::unknown(BlockedReason::UnsafeFunction(head.to_string()));
        }
        if function_invocation && is_safe_function(head) {
            let arguments = trimmed[head_end..].trim_start();
            return self.expand_function(head, arguments, depth, stack);
        }
        if function_invocation && is_known_unsupported_function(head) {
            return Expansion::unknown(BlockedReason::UnsupportedFunction(head.to_string()));
        }
        if let Some((variable, pattern, replacement)) = parse_substitution_reference(body) {
            return self.expand_substitution_reference(
                variable,
                pattern,
                replacement,
                depth,
                stack,
            );
        }
        if body.contains('$') {
            return Expansion::unknown(BlockedReason::DynamicVariableName(body.to_string()));
        }
        self.expand_variable(body.trim(), depth, stack)
    }

    fn expand_variable(&self, name: &str, depth: usize, stack: &mut Vec<String>) -> Expansion {
        if stack.iter().any(|active| active == name) {
            return Expansion::unknown(BlockedReason::RecursiveReference(name.to_string()));
        }
        let Some(variable) = self.variables.get(name) else {
            return Expansion::unknown(BlockedReason::UndefinedVariable(name.to_string()));
        };
        let mut result = match &variable.value {
            StoredValue::Simple(value) => Expansion::known(value.clone()),
            StoredValue::Recursive(value) => {
                stack.push(name.to_string());
                let result = self.expand_inner(value, depth, stack);
                stack.pop();
                result
            }
            StoredValue::Unknown(reason) => Expansion::unknown(reason.clone()),
        };
        result.trace.insert(
            0,
            TraceStep {
                variable: name.to_string(),
                origin: variable.origin,
            },
        );
        result
    }

    fn expand_substitution_reference(
        &self,
        variable: &str,
        pattern: &str,
        replacement: &str,
        depth: usize,
        stack: &mut Vec<String>,
    ) -> Expansion {
        let variable = variable.trim();
        if variable.contains('$') {
            return Expansion::unknown(BlockedReason::DynamicVariableName(variable.to_string()));
        }
        let source = self.expand_variable(variable, depth, stack);
        let pattern = self.expand_inner(pattern, depth, stack);
        let replacement = self.expand_inner(replacement, depth, stack);
        let mut combined = Expansion::known("");
        for expansion in [&source, &pattern, &replacement] {
            combined.trace.extend(expansion.trace.clone());
            combined.blocked.extend(expansion.blocked.clone());
            if expansion.value.is_none() {
                combined.value = None;
            }
        }
        let (Some(source), Some(pattern), Some(replacement)) =
            (source.value, pattern.value, replacement.value)
        else {
            return combined;
        };
        let suffix_form = !pattern.contains('%');
        let pattern = if suffix_form {
            format!("%{pattern}")
        } else {
            pattern
        };
        let replacement = if suffix_form && !replacement.contains('%') {
            format!("%{replacement}")
        } else {
            replacement
        };
        combined.value = Some(
            words(&source)
                .map(|word| pattern_replace(&pattern, &replacement, word))
                .collect::<Vec<_>>()
                .join(" "),
        );
        combined
    }

    fn expand_function(
        &self,
        name: &str,
        arguments: &str,
        depth: usize,
        stack: &mut Vec<String>,
    ) -> Expansion {
        if matches!(name, "if" | "or" | "and") {
            return self.expand_lazy_function(name, arguments, depth, stack);
        }
        if name == "value" {
            let variable = arguments.trim();
            return match self.variables.get(variable) {
                Some(VariableState {
                    value: StoredValue::Recursive(value) | StoredValue::Simple(value),
                    origin,
                    ..
                }) => {
                    let mut result = Expansion::known(value.clone());
                    result.trace.push(TraceStep {
                        variable: variable.to_string(),
                        origin: *origin,
                    });
                    result
                }
                Some(VariableState {
                    value: StoredValue::Unknown(reason),
                    ..
                }) => Expansion::unknown(reason.clone()),
                None => Expansion::unknown(BlockedReason::UndefinedVariable(variable.into())),
            };
        }
        if name == "flavor" {
            let value = match self.flavor(arguments.trim()) {
                Some(VariableFlavor::Recursive) => "recursive",
                Some(VariableFlavor::Simple) => "simple",
                Some(VariableFlavor::Unknown) => {
                    return Expansion::unknown(BlockedReason::IndeterminateAssignment(
                        arguments.trim().into(),
                    ));
                }
                None => "undefined",
            };
            return Expansion::known(value);
        }
        if name == "origin" {
            let value = match self.variables.get(arguments.trim()) {
                Some(state) if state.command_line => "command line",
                Some(_) => "file",
                None => "undefined",
            };
            return Expansion::known(value);
        }

        let raw_arguments = split_function_arguments(arguments);
        let mut expanded = Vec::with_capacity(raw_arguments.len());
        let mut combined = Expansion::known("");
        for argument in raw_arguments {
            let result = self.expand_inner(argument, depth, stack);
            if let Some(value) = result.value {
                combined.trace.extend(result.trace);
                expanded.push(value);
            } else {
                combined.merge_unknown(result);
            }
        }
        if combined.value.is_none() {
            return combined;
        }

        let value = match name {
            "strip" => collapse_whitespace(argument(&expanded, 0)),
            "subst" => {
                argument(&expanded, 2).replace(argument(&expanded, 0), argument(&expanded, 1))
            }
            "patsubst" => words(argument(&expanded, 2))
                .map(|word| pattern_replace(argument(&expanded, 0), argument(&expanded, 1), word))
                .collect::<Vec<_>>()
                .join(" "),
            "addprefix" => words(argument(&expanded, 1))
                .map(|word| format!("{}{word}", argument(&expanded, 0)))
                .collect::<Vec<_>>()
                .join(" "),
            "addsuffix" => words(argument(&expanded, 1))
                .map(|word| format!("{word}{}", argument(&expanded, 0)))
                .collect::<Vec<_>>()
                .join(" "),
            "sort" => {
                let sorted = words(argument(&expanded, 0)).collect::<BTreeSet<_>>();
                sorted.into_iter().collect::<Vec<_>>().join(" ")
            }
            "words" => words(argument(&expanded, 0)).count().to_string(),
            "firstword" => words(argument(&expanded, 0))
                .next()
                .unwrap_or_default()
                .to_string(),
            "lastword" => words(argument(&expanded, 0))
                .last()
                .unwrap_or_default()
                .to_string(),
            "word" => {
                let Some(index) = positive_index(argument(&expanded, 0)) else {
                    return Expansion::unknown(BlockedReason::MalformedExpansion);
                };
                words(argument(&expanded, 1))
                    .nth(index - 1)
                    .unwrap_or_default()
                    .to_string()
            }
            "wordlist" => {
                let (Some(start), Some(end)) = (
                    positive_index(argument(&expanded, 0)),
                    positive_index(argument(&expanded, 1)),
                ) else {
                    return Expansion::unknown(BlockedReason::MalformedExpansion);
                };
                if end < start {
                    String::new()
                } else {
                    words(argument(&expanded, 2))
                        .skip(start - 1)
                        .take(end - start + 1)
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            }
            "dir" => words(argument(&expanded, 0))
                .map(directory_part)
                .collect::<Vec<_>>()
                .join(" "),
            "notdir" => words(argument(&expanded, 0))
                .map(file_part)
                .collect::<Vec<_>>()
                .join(" "),
            "suffix" => words(argument(&expanded, 0))
                .filter_map(file_suffix)
                .collect::<Vec<_>>()
                .join(" "),
            "basename" => words(argument(&expanded, 0))
                .map(file_basename)
                .collect::<Vec<_>>()
                .join(" "),
            "join" => join_words(argument(&expanded, 0), argument(&expanded, 1)),
            "findstring" => {
                if argument(&expanded, 1).contains(argument(&expanded, 0)) {
                    argument(&expanded, 0).to_string()
                } else {
                    String::new()
                }
            }
            "filter" | "filter-out" => {
                let keep_matches = name == "filter";
                let patterns = words(argument(&expanded, 0)).collect::<Vec<_>>();
                words(argument(&expanded, 1))
                    .filter(|word| {
                        patterns
                            .iter()
                            .any(|pattern| pattern_matches(pattern, word))
                            == keep_matches
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            _ => {
                return Expansion::unknown(BlockedReason::UnsupportedFunction(name.to_string()));
            }
        };
        combined.value = Some(value);
        combined
    }

    fn expand_lazy_function(
        &self,
        name: &str,
        arguments: &str,
        depth: usize,
        stack: &mut Vec<String>,
    ) -> Expansion {
        let arguments = split_function_arguments(arguments);
        match name {
            "if" => {
                let condition =
                    self.expand_inner(arguments.first().copied().unwrap_or(""), depth, stack);
                let Some(value) = condition.value.as_deref() else {
                    return condition;
                };
                let selected = if value.trim().is_empty() {
                    arguments.get(2).copied().unwrap_or("")
                } else {
                    arguments.get(1).copied().unwrap_or("")
                };
                let mut result = self.expand_inner(selected, depth, stack);
                result.trace.splice(0..0, condition.trace);
                result.blocked.extend(condition.blocked);
                result
            }
            "or" => {
                let mut combined = Expansion::known("");
                for argument in arguments {
                    let expansion = self.expand_inner(argument, depth, stack);
                    combined.trace.extend(expansion.trace.clone());
                    combined.blocked.extend(expansion.blocked.clone());
                    let Some(value) = expansion.value else {
                        combined.value = None;
                        return combined;
                    };
                    if !value.is_empty() {
                        combined.value = Some(value);
                        return combined;
                    }
                }
                combined
            }
            "and" => {
                let mut combined = Expansion::known("");
                for argument in arguments {
                    let expansion = self.expand_inner(argument, depth, stack);
                    combined.trace.extend(expansion.trace.clone());
                    combined.blocked.extend(expansion.blocked.clone());
                    let Some(value) = expansion.value else {
                        combined.value = None;
                        return combined;
                    };
                    combined.value = Some(value.clone());
                    if value.is_empty() {
                        return combined;
                    }
                }
                combined
            }
            _ => Expansion::unknown(BlockedReason::UnsupportedFunction(name.to_string())),
        }
    }
}

fn append_with_space(value: &mut String, addition: &str) {
    if !value.is_empty() && !addition.is_empty() {
        value.push(' ');
    }
    value.push_str(addition);
}

fn argument(arguments: &[String], index: usize) -> &str {
    arguments.get(index).map_or("", String::as_str)
}

fn words(value: &str) -> impl Iterator<Item = &str> {
    value.split_whitespace()
}

fn collapse_whitespace(value: &str) -> String {
    words(value).collect::<Vec<_>>().join(" ")
}

fn positive_index(value: &str) -> Option<usize> {
    value.trim().parse().ok().filter(|index| *index > 0)
}

fn directory_part(value: &str) -> &str {
    value
        .rfind('/')
        .map_or("./", |separator| &value[..=separator])
}

fn file_part(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

fn file_suffix(value: &str) -> Option<&str> {
    let filename_start = value.rfind('/').map_or(0, |separator| separator + 1);
    let suffix = value[filename_start..].rfind('.')? + filename_start;
    Some(&value[suffix..])
}

fn file_basename(value: &str) -> &str {
    let filename_start = value.rfind('/').map_or(0, |separator| separator + 1);
    value[filename_start..]
        .rfind('.')
        .map_or(value, |suffix| &value[..filename_start + suffix])
}

fn join_words(left: &str, right: &str) -> String {
    let left = words(left).collect::<Vec<_>>();
    let right = words(right).collect::<Vec<_>>();
    (0..left.len().max(right.len()))
        .map(|index| {
            format!(
                "{}{}",
                left.get(index).copied().unwrap_or(""),
                right.get(index).copied().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn pattern_replace(pattern: &str, replacement: &str, word: &str) -> String {
    pattern_stem(pattern, word).map_or_else(
        || word.to_string(),
        |stem| replacement.replacen('%', stem, 1),
    )
}

fn pattern_matches(pattern: &str, word: &str) -> bool {
    pattern_stem(pattern, word).is_some()
}

fn pattern_stem<'a>(pattern: &str, word: &'a str) -> Option<&'a str> {
    let Some(percent) = pattern.find('%') else {
        return (pattern == word).then_some("");
    };
    let prefix = &pattern[..percent];
    let suffix = &pattern[percent + 1..];
    word.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn is_safe_function(name: &str) -> bool {
    matches!(
        name,
        "strip"
            | "subst"
            | "patsubst"
            | "addprefix"
            | "addsuffix"
            | "sort"
            | "words"
            | "firstword"
            | "lastword"
            | "word"
            | "wordlist"
            | "dir"
            | "notdir"
            | "suffix"
            | "basename"
            | "join"
            | "if"
            | "or"
            | "and"
            | "findstring"
            | "filter"
            | "filter-out"
            | "value"
            | "flavor"
            | "origin"
    )
}

fn is_unsafe_function(name: &str) -> bool {
    matches!(
        name,
        "shell" | "eval" | "file" | "guile" | "error" | "warning" | "info"
    )
}

fn is_known_unsupported_function(name: &str) -> bool {
    matches!(
        name,
        "wildcard" | "realpath" | "abspath" | "intcmp" | "foreach" | "let" | "call"
    )
}

fn parse_substitution_reference(body: &str) -> Option<(&str, &str, &str)> {
    let mut colon = None;
    let mut closers = Vec::new();
    let mut characters = body.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '$' {
            if let Some((_, next)) = characters.peek().copied() {
                if next == '$' {
                    characters.next();
                } else if matches!(next, '(' | '{') {
                    closers.push(if next == '(' { ')' } else { '}' });
                    characters.next();
                }
            }
        } else if closers.last().copied() == Some(character) {
            closers.pop();
        } else if closers.is_empty() {
            if character == ':' && colon.is_none() {
                colon = Some(index);
            } else if character == '=' {
                let colon = colon?;
                return Some((&body[..colon], &body[colon + 1..index], &body[index + 1..]));
            }
        }
    }
    None
}

fn matching_delimiter(text: &str, body_start: usize, initial_closing: char) -> Option<usize> {
    let mut closers = vec![initial_closing];
    let mut characters = text[body_start..].char_indices().peekable();
    while let Some((relative, character)) = characters.next() {
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
            if closers.is_empty() {
                return Some(body_start + relative);
            }
        }
    }
    None
}

fn split_function_arguments(arguments: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut closers = Vec::new();
    let mut characters = arguments.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '$' {
            if let Some((_, next)) = characters.peek().copied() {
                if matches!(next, '(' | '{') {
                    closers.push(if next == '(' { ')' } else { '}' });
                    characters.next();
                }
            }
        } else if closers.last().copied() == Some(character) {
            closers.pop();
        } else if character == ',' && closers.is_empty() {
            result.push(&arguments[start..index]);
            start = index + 1;
        }
    }
    result.push(&arguments[start..]);
    result
}

fn parse_comparison(expression: &str) -> Option<(&str, &str)> {
    let expression = expression.trim();
    if let Some(inner) = expression
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        let arguments = split_function_arguments(inner);
        return (arguments.len() == 2).then(|| (arguments[0].trim(), arguments[1].trim()));
    }
    let (left, rest) = parse_quoted(expression)?;
    let (right, trailing) = parse_quoted(rest.trim_start())?;
    trailing.trim().is_empty().then_some((left, right))
}

fn parse_quoted(value: &str) -> Option<(&str, &str)> {
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let body = &value[quote.len_utf8()..];
    let end = body.find(quote)?;
    Some((&body[..end], &body[end + quote.len_utf8()..]))
}
