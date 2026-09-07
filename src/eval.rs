//! Safe, side-effect-free evaluation of the statically knowable Make subset.

use std::collections::{BTreeMap, BTreeSet};

use crate::builtins::is_defined_by_make;
use crate::expansion::{reference_end, reference_length, MAX_EXPANSION_DEPTH};
use crate::logical::ConditionalKind;
use crate::parser::{AssignmentOperator, Variable};
use crate::project::SourceId;

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
    Unknown {
        reason: BlockedReason,
        /// What GNU Make holds here once the names that had no value at the
        /// assignment contribute nothing, for an assignment Rumk expanded
        /// itself. `None` where something else left the value unexpanded, and
        /// Make then holds text this cannot name.
        undefined: Option<UndefinedValue>,
    },
}

/// The value a simple assignment produces where every name it reads that has
/// no value contributes nothing, computed where the assignment appears so it
/// reads the variables Make read there.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UndefinedValue {
    value: String,
    /// The names that had no value, in the order they were found.
    variables: Vec<UndefinedName>,
}

/// A name an expansion read while it had no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndefinedName {
    pub name: String,
    /// How many definitions Make had read where this expansion read the name.
    /// A definition it reads after that point is one it reaches only
    /// afterwards, whatever the name is worth in between. A simple assignment
    /// reads its value where it is written, so that is where this is taken for
    /// the names behind it.
    pub definitions_read: usize,
}

/// A definition Make read, and whether it is one a reading before it should
/// have waited for. A definition that only reads the name back and writes it
/// again passes along whatever the caller supplied, and one that writes nothing
/// leaves the name worth exactly what a reading before it already produced, so
/// neither is a definition anything can be read too early for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Definition {
    name: String,
    at: EvaluationLocation,
    defines_a_value: bool,
    /// Whether Make certainly reads it, rather than reaching it only through a
    /// branch whose condition Make decides while it reads.
    certain: bool,
}

/// A name a definition read where Make had given it no value yet, kept with
/// how much of the project Make had read so a definition further down can be
/// found once the whole reading is known.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EarlyReading {
    name: String,
    definitions_read: usize,
    at: EvaluationLocation,
}

/// A name a definition read before the definition that gives it a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadTooEarly {
    pub name: String,
    /// Where the definition that read the name is.
    pub at: EvaluationLocation,
    /// Where the definition it was read before is.
    pub defined: EvaluationLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VariableState {
    value: StoredValue,
    origin: Option<EvaluationLocation>,
    command_line: bool,
}

/// What one expansion carries: the values it is inside, so a reference back to
/// one of them is not expanded forever, and, while an expression is being read
/// the way Make reads it with the valueless names contributing nothing, the
/// names it found that way.
struct Expanding<'a> {
    stack: Vec<String>,
    undefined: Option<&'a mut Vec<UndefinedName>>,
}

impl Expanding<'_> {
    fn plain() -> Self {
        Self {
            stack: Vec::new(),
            undefined: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Evaluator {
    variables: BTreeMap<String, VariableState>,
    /// Every definition Make has read, in the order it read them. A file read
    /// twice contributes its definitions twice, because Make reads them again,
    /// and `undefine` takes none of them back: the reading happened whatever
    /// the value is worth afterwards.
    definitions_read: Vec<Definition>,
    /// Where in `definitions_read` each name's own definitions sit, in the
    /// order Make read them. A reading looks only at the definitions of the
    /// name it read, rather than walking every definition below it.
    definitions_by_name: BTreeMap<String, Vec<usize>>,
    /// Every name the project gives a value of its own, whatever the name is
    /// worth afterwards. A definition that only reads the name back and writes
    /// it again leaves the name out: what it wrote came from wherever the
    /// caller put it, not from the project.
    values_given: BTreeSet<String>,
    /// Every reading a definition made of a name Make had given no value yet,
    /// in the order Make read them. Whether a definition arrives too late for
    /// one of these is answerable only once the whole project is read.
    early_readings: Vec<EarlyReading>,
}

impl Evaluator {
    pub fn new(predefined: &BTreeMap<String, String>) -> Self {
        Self {
            definitions_read: Vec::new(),
            definitions_by_name: BTreeMap::new(),
            values_given: BTreeSet::new(),
            early_readings: Vec::new(),
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
        self.expand_inner(input, 0, &mut Expanding::plain())
    }

    pub fn assign(&mut self, variable: &Variable, location: EvaluationLocation, activity: Truth) {
        if activity == Truth::False {
            // Make never reads this definition, but the project still says
            // what the name is worth here, so the value is the project's.
            self.values_given.insert(variable.name.clone());
            return;
        }
        self.definitions_read.push(Definition {
            name: variable.name.clone(),
            at: location,
            defines_a_value: true,
            certain: activity == Truth::True,
        });
        let definition = self.definitions_read.len() - 1;
        self.definitions_by_name
            .entry(variable.name.clone())
            .or_default()
            .push(definition);
        self.store_assignment(variable, location, activity);
        // A definition that only restates the value the name already carried
        // takes that value from wherever the caller put it, so it is not the
        // project giving the name a value.
        let restates = self.restates_the_caller(&variable.name);
        if !restates {
            self.values_given.insert(variable.name.clone());
        }
        // An assignment Make may not read is held indeterminate for that reason
        // alone, so what it writes back is a question only for the ones Make
        // certainly reads.
        let indeterminate =
            activity == Truth::True && self.restates_an_indeterminate_value(&variable.name);
        self.definitions_read[definition].defines_a_value =
            !restates && !self.writes_nothing(&variable.name) && !indeterminate;
        self.record_early_readings(&variable.name, location);
    }

    /// Whether what `name` now holds is nothing at all, so a reading of it
    /// before this definition produced the same text a reading after it does.
    fn writes_nothing(&self, name: &str) -> bool {
        match self.variables.get(name).map(|state| &state.value) {
            Some(StoredValue::Recursive(value) | StoredValue::Simple(value)) => value.is_empty(),
            // What Make holds here is known only where the names with no value
            // are all that kept Rumk from expanding it.
            Some(StoredValue::Unknown { undefined, .. }) => undefined
                .as_ref()
                .is_some_and(|undefined| undefined.value.is_empty()),
            None => true,
        }
    }

    /// Records the names this definition read itself where Make had given them
    /// no value. A name carried in from a variable the definition read was
    /// read where that variable was written, and is recorded there, so only a
    /// name read as many definitions in as Make has now read belongs here.
    fn record_early_readings(&mut self, name: &str, at: EvaluationLocation) {
        let here = self.definitions_read.len();
        let Some(StoredValue::Unknown {
            undefined: Some(undefined),
            ..
        }) = self.variables.get(name).map(|state| &state.value)
        else {
            return;
        };
        let readings: Vec<EarlyReading> = undefined
            .variables
            .iter()
            .filter(|found| found.definitions_read == here)
            .map(|found| EarlyReading {
                name: found.name.clone(),
                definitions_read: here,
                at,
            })
            .collect();
        self.early_readings.extend(readings);
    }

    /// Every name a definition read before a definition Make certainly reaches
    /// gives it a value, in the order Make read them. Answering this needs the
    /// whole reading, so it holds only once the project is loaded.
    pub fn read_too_early(&self) -> Vec<ReadTooEarly> {
        self.early_readings
            .iter()
            .filter_map(|reading| {
                self.certain_definition_after(&reading.name, reading.definitions_read)
                    .map(|defined| ReadTooEarly {
                        name: reading.name.clone(),
                        at: reading.at,
                        defined,
                    })
            })
            .collect()
    }

    fn store_assignment(
        &mut self,
        variable: &Variable,
        location: EvaluationLocation,
        activity: Truth,
    ) {
        if activity == Truth::Unknown {
            self.variables.insert(
                variable.name.clone(),
                VariableState {
                    value: StoredValue::Unknown {
                        reason: BlockedReason::IndeterminateAssignment(variable.name.clone()),
                        undefined: None,
                    },
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
                        value: StoredValue::Unknown {
                            reason: BlockedReason::ShellAssignment(variable.name.clone()),
                            undefined: None,
                        },
                        origin: Some(location),
                        command_line: false,
                    },
                );
            }
            AssignmentOperator::Simple | AssignmentOperator::SimplePosix => {
                let expansion = self.expand(&variable.value);
                self.store_expansion(&variable.name, &variable.value, expansion, location);
            }
            AssignmentOperator::ImmediateRecursive if variable.value.contains('$') => {
                self.variables.insert(
                    variable.name.clone(),
                    VariableState {
                        value: StoredValue::Unknown {
                            reason: BlockedReason::UnsupportedFunction(":::=".into()),
                            undefined: None,
                        },
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

    /// Whether what `name` now holds is a value Rumk could not work out because
    /// a branch Make decides while it reads had already left that same name
    /// indeterminate. `EXT := $(strip $(EXT))` under such a branch may write
    /// back exactly what the caller supplied, so nothing can be said to have
    /// been read too early for it.
    fn restates_an_indeterminate_value(&self, name: &str) -> bool {
        matches!(
            self.variables.get(name).map(|state| &state.value),
            Some(StoredValue::Unknown {
                reason: BlockedReason::IndeterminateAssignment(blocked),
                ..
            }) if blocked == name
        )
    }

    /// Whether what `name` now holds rests on that same name having had no
    /// value where it was written: the definition read the name back, so the
    /// value it wrote is the one the name arrived with.
    fn restates_the_caller(&self, name: &str) -> bool {
        matches!(
            self.variables.get(name).map(|state| &state.value),
            Some(StoredValue::Unknown {
                undefined: Some(undefined),
                ..
            }) if undefined.variables.iter().any(|found| found.name == name)
        )
    }

    /// Whether the project gives `name` a value of its own somewhere, as
    /// against a caller supplying it through the environment, the command line
    /// or a parent make. `undefine` does not take this back: the project still
    /// says what the name is worth.
    pub fn gives_a_value(&self, name: &str) -> bool {
        self.values_given.contains(name)
    }

    /// Records that the project gives `name` a value while one rule runs. Make
    /// holds it nowhere else, so it is a value the project gives and nothing
    /// the evaluator can read.
    pub fn gives_a_scoped_value(&mut self, name: &str) {
        self.values_given.insert(name.to_string());
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
                        value: StoredValue::Unknown {
                            reason: BlockedReason::IndeterminateAssignment(name.to_string()),
                            undefined: None,
                        },
                        origin: None,
                        command_line: false,
                    },
                );
            }
        }
    }

    /// Where GNU Make next gives `name` a value, having read
    /// `definitions_read` definitions already, and `None` where it reads no
    /// further definition worth waiting for. Answering this needs the whole
    /// reading, so it holds only once the project is loaded.
    pub fn definition_after(
        &self,
        name: &str,
        definitions_read: usize,
    ) -> Option<EvaluationLocation> {
        self.find_definition_after(name, definitions_read, |_| true)
    }

    /// Where GNU Make certainly gives `name` a value after `definitions_read`,
    /// passing over the definitions it reaches only through a branch whose
    /// condition it decides while it reads. Those say nothing about whether a
    /// reading before them was too early, because Make may reach none of them.
    pub fn certain_definition_after(
        &self,
        name: &str,
        definitions_read: usize,
    ) -> Option<EvaluationLocation> {
        self.find_definition_after(name, definitions_read, |definition| definition.certain)
    }

    fn find_definition_after(
        &self,
        name: &str,
        definitions_read: usize,
        accept: impl Fn(&Definition) -> bool,
    ) -> Option<EvaluationLocation> {
        let own = self.definitions_by_name.get(name)?;
        // The positions are in the order Make read them, so the ones below this
        // reading are the tail past the first that is not above it.
        let below = own.partition_point(|position| *position < definitions_read);
        own.get(below..)?
            .iter()
            .map(|position| &self.definitions_read[*position])
            .find(|definition| definition.defines_a_value && accept(definition))
            .map(|definition| definition.at)
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
            StoredValue::Unknown { .. } => VariableFlavor::Unknown,
        })
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    /// Expands `input` the way GNU Make expands it when variables it names
    /// have no value here: such a reference contributes nothing to the text
    /// around it. Returns the text Make produces and the variables that had no
    /// value, in the order they were found.
    ///
    /// `None` once anything but a valueless variable keeps the expression from
    /// being expanded, because what Make produces is then genuinely unknown
    /// rather than known to be the text without those references.
    pub fn expand_as_undefined(&self, input: &str) -> Option<(String, Vec<UndefinedName>)> {
        let mut undefined: Vec<UndefinedName> = Vec::new();
        let value = self
            .expand_inner(
                input,
                0,
                &mut Expanding {
                    stack: Vec::new(),
                    undefined: Some(&mut undefined),
                },
            )
            .value?;
        Some((value, undefined))
    }

    /// What an append produces where the names with no value contribute
    /// nothing, for a base whose own such value is already known.
    fn appended_as_undefined(
        &self,
        base: &UndefinedValue,
        addition: &str,
    ) -> Option<UndefinedValue> {
        let (addition, names) = self.expand_as_undefined(addition)?;
        let mut value = base.value.clone();
        append_with_space(&mut value, &addition);
        let mut variables = base.variables.clone();
        for name in names {
            push_undefined(&mut variables, name);
        }
        Some(UndefinedValue { value, variables })
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
                    let base = UndefinedValue {
                        value,
                        variables: Vec::new(),
                    };
                    StoredValue::Unknown {
                        reason: expansion
                            .blocked
                            .into_iter()
                            .next()
                            .unwrap_or(BlockedReason::MalformedExpansion),
                        undefined: self.appended_as_undefined(&base, &variable.value),
                    }
                }
            }
            StoredValue::Unknown { reason, undefined } => StoredValue::Unknown {
                reason,
                undefined: undefined
                    .as_ref()
                    .and_then(|base| self.appended_as_undefined(base, &variable.value)),
            },
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

    fn store_expansion(
        &mut self,
        name: &str,
        source: &str,
        expansion: Expansion,
        location: EvaluationLocation,
    ) {
        let value = match expansion.value {
            Some(value) => StoredValue::Simple(value),
            None => StoredValue::Unknown {
                reason: expansion
                    .blocked
                    .into_iter()
                    .next()
                    .unwrap_or(BlockedReason::MalformedExpansion),
                // Make computes this assignment here, so what it holds where
                // the names with no value contribute nothing is settled here
                // too, whatever they are given further down.
                undefined: self
                    .expand_as_undefined(source)
                    .map(|(value, variables)| UndefinedValue { value, variables }),
            },
        };
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
            Some(StoredValue::Unknown { .. }) | None => Truth::Unknown,
        }
    }

    fn expand_inner(&self, input: &str, depth: usize, context: &mut Expanding<'_>) -> Expansion {
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
                let body_start = next_index + next.len_utf8();
                let Some(end) = reference_end(input, body_start, next) else {
                    result.merge_unknown(Expansion::unknown(BlockedReason::MalformedExpansion));
                    break;
                };
                while characters
                    .peek()
                    .is_some_and(|(position, _)| *position <= end)
                {
                    characters.next();
                }
                self.expand_body(&input[body_start..end], depth + 1, context)
            } else {
                self.expand_variable(&next.to_string(), depth + 1, context)
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

    fn expand_body(&self, body: &str, depth: usize, context: &mut Expanding<'_>) -> Expansion {
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
            return self.expand_function(head, arguments, depth, context);
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
                context,
            );
        }
        if body.contains('$') {
            return Expansion::unknown(BlockedReason::DynamicVariableName(body.to_string()));
        }
        self.expand_variable(body.trim(), depth, context)
    }

    fn expand_variable(&self, name: &str, depth: usize, context: &mut Expanding<'_>) -> Expansion {
        if context.stack.iter().any(|active| active == name) {
            return Expansion::unknown(BlockedReason::RecursiveReference(name.to_string()));
        }
        let Some(variable) = self.variables.get(name) else {
            return self.read_as_undefined(name, context);
        };
        let mut result = match &variable.value {
            StoredValue::Simple(value) => Expansion::known(value.clone()),
            StoredValue::Recursive(value) => {
                context.stack.push(name.to_string());
                let result = self.expand_inner(value, depth, context);
                context.stack.pop();
                result
            }
            // An assignment Rumk could not expand where it was written still
            // holds what Make computed there under this very assumption, so
            // that value is what an expression reading it produces, and the
            // names behind it had no value as they stood where that assignment
            // read them.
            StoredValue::Unknown { reason, undefined } => {
                match (context.undefined.as_deref_mut(), undefined) {
                    (Some(found), Some(stored)) => {
                        for behind in &stored.variables {
                            push_undefined(found, behind.clone());
                        }
                        Expansion::known(stored.value.clone())
                    }
                    _ => Expansion::unknown(reason.clone()),
                }
            }
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

    /// What a reference to a name the project has given no value produces:
    /// nothing, where the expression is being read the way Make reads it, and
    /// an unknown otherwise. Make gives some names a value of its own, which
    /// Rumk does not know, so what those expand to is unknown either way.
    fn read_as_undefined(&self, name: &str, context: &mut Expanding<'_>) -> Expansion {
        let blocked = Expansion::unknown(BlockedReason::UndefinedVariable(name.to_string()));
        let Some(found) = context.undefined.as_deref_mut() else {
            return blocked;
        };
        if is_defined_by_make(name) {
            return blocked;
        }
        // This expansion is the reading, so how much of the project Make had
        // read where the name had no value is settled here.
        push_undefined(
            found,
            UndefinedName {
                name: name.to_string(),
                definitions_read: self.definitions_read.len(),
            },
        );
        Expansion::known("")
    }

    fn expand_substitution_reference(
        &self,
        variable: &str,
        pattern: &str,
        replacement: &str,
        depth: usize,
        context: &mut Expanding<'_>,
    ) -> Expansion {
        let variable = variable.trim();
        if variable.contains('$') {
            return Expansion::unknown(BlockedReason::DynamicVariableName(variable.to_string()));
        }
        let source = self.expand_variable(variable, depth, context);
        let pattern = self.expand_inner(pattern, depth, context);
        let replacement = self.expand_inner(replacement, depth, context);
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
        context: &mut Expanding<'_>,
    ) -> Expansion {
        if matches!(name, "if" | "or" | "and") {
            return self.expand_lazy_function(name, arguments, depth, context);
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
                    value: StoredValue::Unknown { reason, .. },
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
            let result = self.expand_inner(argument, depth, context);
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
                let text = argument(&expanded, 2);
                match argument(&expanded, 0) {
                    // Make has nothing to look for and appends the
                    // replacement once, rather than at every position.
                    "" => format!("{text}{}", argument(&expanded, 1)),
                    from => text.replace(from, argument(&expanded, 1)),
                }
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
        context: &mut Expanding<'_>,
    ) -> Expansion {
        let arguments = split_function_arguments(arguments);
        match name {
            "if" => {
                let condition =
                    self.expand_inner(arguments.first().copied().unwrap_or(""), depth, context);
                let Some(value) = condition.value.as_deref() else {
                    return condition;
                };
                let selected = if value.trim().is_empty() {
                    arguments.get(2).copied().unwrap_or("")
                } else {
                    arguments.get(1).copied().unwrap_or("")
                };
                let mut result = self.expand_inner(selected, depth, context);
                result.trace.splice(0..0, condition.trace);
                result.blocked.extend(condition.blocked);
                result
            }
            "or" => {
                let mut combined = Expansion::known("");
                for argument in arguments {
                    let expansion = self.expand_inner(argument, depth, context);
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
                    let expansion = self.expand_inner(argument, depth, context);
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

/// Records a name that had no value, keeping the earliest reading of it: a
/// definition after that one is a definition the expression was read before,
/// whatever a later reading of the same name had seen by then.
fn push_undefined(names: &mut Vec<UndefinedName>, found: UndefinedName) {
    match names.iter_mut().find(|seen| seen.name == found.name) {
        Some(seen) => seen.definitions_read = seen.definitions_read.min(found.definitions_read),
        None => names.push(found),
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

pub(crate) fn pattern_stem<'a>(pattern: &str, word: &'a str) -> Option<&'a str> {
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

/// Splits function arguments at the commas outside references. A comma
/// inside an unterminated reference separates nothing.
fn split_function_arguments(arguments: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut skip_to = 0;
    for (index, character) in arguments.char_indices() {
        if index < skip_to {
            continue;
        }
        match character {
            '$' => match reference_length(arguments, index) {
                Some(length) => skip_to = index + length,
                None => break,
            },
            ',' => {
                result.push(&arguments[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(&arguments[start..]);
    result
}

/// Splits an `ifeq`/`ifneq` expression into its two operands, accepting the
/// parenthesized and the quoted forms. In the parenthesized form Make keeps
/// the blanks after the opening parenthesis and before the closing one, and
/// drops the blanks around the comma.
pub(crate) fn parse_comparison(expression: &str) -> Option<(&str, &str)> {
    let expression = expression.trim();
    if let Some(inner) = expression
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        let arguments = split_function_arguments(inner);
        return (arguments.len() == 2).then(|| {
            (
                arguments[0].trim_end_matches([' ', '\t']),
                arguments[1].trim_start_matches([' ', '\t']),
            )
        });
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
