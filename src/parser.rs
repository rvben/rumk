use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::OnceLock;

use crate::analysis::{ConditionalIndex, SemanticIndex};
use crate::binding::value_expands_later;
use crate::expansion::{contains_reference, find_unterminated_reference, CommentHandling};
use crate::logical::{
    conditional_expression, find_top_level_assignment, find_top_level_rule_separator,
    inline_recipe_separator, split_include_words, split_once_top_level, split_top_level_words,
    strip_top_level_comment, target_assignment, ConditionalKind, IncludeKind, LogicalDocument,
    LogicalKind, LogicalStatement, Reach,
};
use crate::syntax::{RecipePrefix, SyntaxTree};

#[derive(Debug, Clone)]
pub struct Makefile {
    /// Lossless, source-ordered syntax for tools that need exact text or spans.
    pub syntax: SyntaxTree,
    /// Continuation-folded statements with exact source spans.
    pub logical: LogicalDocument,
    pub rules: Vec<Rule>,
    /// All assignments in source order. `variables` remains a last-value lookup.
    pub assignments: Vec<Variable>,
    pub variables: HashMap<String, Variable>,
    pub phonies: Vec<String>,
    pub includes: Vec<Include>,
    pub conditionals: Vec<Conditional>,
    pub definitions: Vec<Definition>,
    pub oneshell: bool,
    /// Constructs GNU Make rejects while reading the file, in source order.
    /// The rest of the model is still built around them.
    pub syntax_errors: Vec<SyntaxError>,
    analysis: OnceLock<SemanticIndex>,
    conditional_analysis: OnceLock<ConditionalIndex>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<String>,
    pub prerequisites: Vec<String>,
    pub order_only_prerequisites: Vec<String>,
    pub double_colon: bool,
    pub grouped: bool,
    pub target_pattern: Option<String>,
    pub target_assignment: Option<Variable>,
    pub recipes: Vec<Recipe>,
    pub line: usize,
    pub end_line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub command: String,
    pub inline: bool,
    pub silent: bool,
    pub ignore_errors: bool,
    pub recursive: bool,
    pub line: usize,
    pub end_line: usize,
    pub column: usize,
    pub indentation: String,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub operator: AssignmentOperator,
    pub modifiers: VariableModifiers,
    pub scope: VariableScope,
    /// Whether GNU Make reads the assignment at all.
    pub reach: Reach,
    pub line: usize,
    pub end_line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableScope {
    Global,
    TargetSpecific(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    pub paths: Vec<String>,
    pub optional: bool,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conditional {
    pub kind: ConditionalKind,
    pub expression: String,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub raw_body: String,
    pub value: String,
    pub operator: AssignmentOperator,
    pub modifiers: VariableModifiers,
    pub reach: Reach,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    Recursive,
    Simple,
    SimplePosix,
    ImmediateRecursive,
    Conditional,
    Append,
    Shell,
}

impl AssignmentOperator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recursive => "=",
            Self::Simple => ":=",
            Self::SimplePosix => "::=",
            Self::ImmediateRecursive => ":::=",
            Self::Conditional => "?=",
            Self::Append => "+=",
            Self::Shell => "!=",
        }
    }

    /// Whether GNU Make expands the assigned value while reading the file.
    pub(crate) fn expands_immediately(self) -> bool {
        matches!(
            self,
            Self::Simple | Self::SimplePosix | Self::ImmediateRecursive | Self::Shell
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VariableModifiers {
    pub export: bool,
    pub unexport: bool,
    pub override_: bool,
    pub private: bool,
}

/// A construct GNU Make rejects while reading the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    /// A line that is neither a rule, an assignment, nor a directive.
    MissingSeparator { hint: Option<SeparatorHint> },
    /// A recipe-prefixed line that no rule can claim.
    RecipeBeforeTarget,
    /// A `$(` or `${` whose closing delimiter never arrives.
    UnterminatedReference {
        closing: char,
        /// The function Make would report, when the body starts a call.
        function: Option<String>,
        /// The recursively expanded variable holding the reference. Make
        /// only fails once that variable is expanded.
        deferred_in: Option<String>,
    },
    /// An assignment operator with nothing before it.
    EmptyVariableName,
    /// A `define` that reaches the end of the file without its `endef`.
    UnterminatedDefine { name: String },
    /// An `endef` outside any `define`.
    UnexpectedEndef,
    /// A conditional that reaches the end of the file without its `endif`.
    /// Holds the line that opens it, without its comment.
    UnterminatedConditional { conditional: String },
    /// An `else` outside any conditional.
    UnexpectedElse,
    /// An `endif` outside any conditional.
    UnexpectedEndif,
    /// A second `else` for the same conditional.
    SecondElse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeparatorHint {
    /// The line starts with eight spaces where a recipe tab was likely meant.
    SpacesInsteadOfTab,
    /// `ifeq` or `ifneq` written without whitespace before the condition.
    ConditionalWithoutSpace,
}

impl fmt::Display for SyntaxErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator { hint: None } => formatter
                .write_str("Missing separator: line is not a rule, an assignment, or a directive"),
            Self::MissingSeparator {
                hint: Some(SeparatorHint::SpacesInsteadOfTab),
            } => formatter.write_str("Missing separator (did you mean a tab instead of 8 spaces?)"),
            Self::MissingSeparator {
                hint: Some(SeparatorHint::ConditionalWithoutSpace),
            } => formatter
                .write_str("Missing separator: ifeq and ifneq must be followed by whitespace"),
            Self::RecipeBeforeTarget => formatter.write_str("Recipe commences before first target"),
            Self::UnterminatedReference {
                closing,
                function,
                deferred_in,
            } => {
                match function {
                    Some(function) => write!(
                        formatter,
                        "Unterminated call to function '{function}': missing '{closing}'"
                    )?,
                    None => write!(
                        formatter,
                        "Unterminated variable reference: missing '{closing}'"
                    )?,
                }
                if let Some(name) = deferred_in {
                    write!(formatter, " (GNU Make fails when '{name}' is expanded)")?;
                }
                Ok(())
            }
            Self::EmptyVariableName => formatter.write_str("Empty variable name"),
            Self::UnterminatedDefine { name } => {
                write!(formatter, "Missing 'endef' for 'define {name}'")
            }
            Self::UnexpectedEndef => formatter.write_str("'endef' without a matching 'define'"),
            Self::UnterminatedConditional { conditional } => {
                write!(formatter, "Missing 'endif' for '{conditional}'")
            }
            Self::UnexpectedElse => formatter.write_str("'else' without a matching conditional"),
            Self::UnexpectedEndif => formatter.write_str("'endif' without a matching conditional"),
            Self::SecondElse => formatter.write_str("Only one 'else' per conditional"),
        }
    }
}

/// A conditional whose `endif` has not been read yet.
struct OpenConditional {
    /// The line that opens it, without its comment.
    header: String,
    line: usize,
    column: usize,
    /// Whether a plain `else` has closed its last branch.
    else_seen: bool,
}

/// Parses `content` into a [`Makefile`]. Parsing never fails: constructs GNU
/// Make would reject are recorded in `syntax_errors` and skipped.
pub fn parse(content: &str) -> Makefile {
    Parser::new(content).parse()
}

impl Makefile {
    /// Returns conditional structure without building the variable and target indexes.
    pub fn conditional_analysis(&self) -> &ConditionalIndex {
        self.conditional_analysis
            .get_or_init(|| ConditionalIndex::build(self))
    }

    /// Returns the semantic index, building it once on first use.
    pub fn analysis(&self) -> &SemanticIndex {
        self.analysis.get_or_init(|| SemanticIndex::build(self))
    }
}

struct Parser {
    current_statement: usize,
    makefile: Makefile,
    /// The rule that receives recipe lines, once one is pending.
    last_rule: Option<usize>,
    /// Whether Make reads the statement being parsed.
    reach: Reach,
    /// The conditionals around the statement being parsed, outermost first.
    open_conditionals: Vec<OpenConditional>,
}

impl Parser {
    fn new(content: &str) -> Self {
        let syntax = SyntaxTree::parse(content);
        let logical = LogicalDocument::parse(&syntax);
        Self {
            current_statement: 0,
            makefile: Makefile {
                syntax,
                logical,
                rules: Vec::new(),
                assignments: Vec::new(),
                variables: HashMap::new(),
                phonies: Vec::new(),
                includes: Vec::new(),
                conditionals: Vec::new(),
                definitions: Vec::new(),
                oneshell: false,
                syntax_errors: Vec::new(),
                analysis: OnceLock::new(),
                conditional_analysis: OnceLock::new(),
            },
            last_rule: None,
            reach: Reach::Always,
            open_conditionals: Vec::new(),
        }
    }

    fn parse(mut self) -> Makefile {
        while let Some(statement) = self.statement().cloned() {
            self.current_statement += 1;
            self.reach = statement.reach;
            match statement.kind {
                LogicalKind::Assignment => self.parse_global_variable(&statement),
                LogicalKind::Rule => self.parse_rule(&statement),
                LogicalKind::Recipe => self.attach_recipe(&statement),
                LogicalKind::OrphanRecipe => {
                    self.record(SyntaxErrorKind::RecipeBeforeTarget, statement.start_line, 1);
                }
                LogicalKind::Include(kind) => self.parse_include(&statement, kind),
                LogicalKind::Conditional(kind) => self.parse_conditional(&statement, kind),
                LogicalKind::Directive => self.check_statement(&statement),
                LogicalKind::Define => self.parse_definition(&statement),
                LogicalKind::Endef => self.record(
                    SyntaxErrorKind::UnexpectedEndef,
                    statement.start_line,
                    first_non_blank_column(statement.text()),
                ),
                LogicalKind::Unknown => self.parse_unknown(&statement),
                LogicalKind::Blank | LogicalKind::Comment | LogicalKind::DefineBody => {}
            }
        }

        for open in std::mem::take(&mut self.open_conditionals) {
            self.record_in_any_branch(
                SyntaxErrorKind::UnterminatedConditional {
                    conditional: open.header,
                },
                open.line,
                open.column,
            );
        }
        self.makefile
            .syntax_errors
            .sort_by_key(|error| (error.line, error.column));
        self.makefile
    }

    fn statement(&self) -> Option<&LogicalStatement> {
        self.makefile
            .logical
            .statements()
            .get(self.current_statement)
    }

    fn source(&self) -> &str {
        self.makefile.syntax.source()
    }

    fn recipe_prefix_at(&self, line: usize) -> RecipePrefix {
        self.makefile.syntax.recipe_prefix_at(line)
    }

    /// Records a syntax error unless it sits in a conditional branch whose
    /// literal condition is false, which Make never reads.
    fn record(&mut self, kind: SyntaxErrorKind, line: usize, column: usize) {
        if self.reach == Reach::Never {
            return;
        }
        self.record_in_any_branch(kind, line, column);
    }

    /// Records an error Make raises in a branch it skips as well: it keeps
    /// following the nesting of conditionals there to find the `endif`.
    fn record_in_any_branch(&mut self, kind: SyntaxErrorKind, line: usize, column: usize) {
        self.makefile
            .syntax_errors
            .push(SyntaxError { kind, line, column });
    }

    fn record_all(&mut self, errors: impl IntoIterator<Item = SyntaxError>) {
        for error in errors {
            self.record(error.kind, error.line, error.column);
        }
    }

    /// Checks a statement Make expands immediately and in full, with
    /// comments stripped.
    fn check_statement(&mut self, statement: &LogicalStatement) {
        let raw = statement.raw(self.source());
        let error = unterminated_error(
            raw,
            0..raw.len(),
            statement.start_line,
            CommentHandling::Strip,
            None,
        );
        self.record_all(error);
    }

    fn parse_global_variable(&mut self, statement: &LogicalStatement) {
        let Some(variable) = parse_variable(
            statement.text(),
            statement.start_line,
            statement.end_line,
            VariableScope::Global,
            self.reach,
        ) else {
            self.record(
                SyntaxErrorKind::EmptyVariableName,
                statement.start_line,
                first_non_blank_column(statement.text()),
            );
            return;
        };

        let deferred_in = value_expands_later(&self.makefile.assignments, &variable)
            .then(|| variable.name.clone());
        self.check_assignment(statement, deferred_in.as_deref());

        self.makefile
            .variables
            .insert(variable.name.clone(), variable.clone());
        self.makefile.assignments.push(variable);
    }

    /// Checks both sides of an assignment on the raw statement: the name is
    /// expanded immediately, the value according to the operator.
    fn check_assignment(&mut self, statement: &LogicalStatement, deferred_in: Option<&str>) {
        let raw = statement.raw(self.source());
        let leading = raw.len() - raw.trim_start().len();
        let Some((position, operator)) = find_top_level_assignment(&raw[leading..]) else {
            return;
        };
        let operator_start = leading + position;
        let errors = [
            unterminated_error(
                raw,
                0..operator_start,
                statement.start_line,
                CommentHandling::Strip,
                None,
            ),
            unterminated_error(
                raw,
                operator_start + operator.len()..raw.len(),
                statement.start_line,
                CommentHandling::Strip,
                deferred_in,
            ),
        ];
        self.record_all(errors.into_iter().flatten());
    }

    fn parse_include(&mut self, statement: &LogicalStatement, kind: IncludeKind) {
        self.check_statement(statement);
        let text = statement.text().trim_start();
        let keyword_length = text.find(char::is_whitespace).unwrap_or(text.len());
        let paths = split_include_words(text[keyword_length..].trim());
        self.makefile.includes.push(Include {
            paths,
            optional: kind == IncludeKind::Optional,
            line: statement.start_line,
            end_line: statement.end_line,
        });
    }

    fn parse_conditional(&mut self, statement: &LogicalStatement, kind: ConditionalKind) {
        self.check_statement(statement);
        let expression = conditional_expression(statement.text());
        self.track_conditional(statement, kind, expression);
        self.makefile.conditionals.push(Conditional {
            kind,
            expression: expression.to_string(),
            line: statement.start_line,
            end_line: statement.end_line,
        });
    }

    /// Follows the nesting of conditionals, which Make checks in every
    /// branch. Only a plain `else` uses up the one `else` of a conditional;
    /// an `else if...` opens another branch of the same conditional.
    fn track_conditional(
        &mut self,
        statement: &LogicalStatement,
        kind: ConditionalKind,
        expression: &str,
    ) {
        let line = statement.start_line;
        let column = first_non_blank_column(statement.text());
        match kind {
            ConditionalKind::Ifdef
            | ConditionalKind::Ifndef
            | ConditionalKind::Ifeq
            | ConditionalKind::Ifneq => self.open_conditionals.push(OpenConditional {
                header: strip_top_level_comment(statement.text()).trim().to_string(),
                line,
                column,
                else_seen: false,
            }),
            ConditionalKind::Else => match self.open_conditionals.last_mut() {
                None => self.record_in_any_branch(SyntaxErrorKind::UnexpectedElse, line, column),
                Some(open) if open.else_seen => {
                    self.record_in_any_branch(SyntaxErrorKind::SecondElse, line, column);
                }
                Some(open) => open.else_seen = expression.is_empty(),
            },
            ConditionalKind::Endif => {
                if self.open_conditionals.pop().is_none() {
                    self.record_in_any_branch(SyntaxErrorKind::UnexpectedEndif, line, column);
                }
            }
        }
    }

    fn parse_definition(&mut self, statement: &LogicalStatement) {
        self.check_statement(statement);
        let reach = self.reach;
        // Make strips the comment before it reads the name and operator.
        let header = parse_definition_header(strip_top_level_comment(statement.text()));
        if header.is_none() {
            self.record(
                SyntaxErrorKind::EmptyVariableName,
                statement.start_line,
                first_non_blank_column(statement.text()),
            );
        }

        let mut depth = 1usize;
        let body_start = statement.span.end.offset;
        let mut body_end = body_start;
        let mut end_line = statement.end_line;
        let mut terminated = false;

        while let Some(next) = self.statement() {
            match next.kind {
                LogicalKind::Define => depth += 1,
                LogicalKind::Endef => {
                    depth -= 1;
                    if depth == 0 {
                        end_line = next.end_line;
                        terminated = true;
                        self.current_statement += 1;
                        break;
                    }
                }
                _ => {}
            }
            body_end = next.span.end.offset;
            end_line = next.end_line;
            self.current_statement += 1;
        }

        let Some((name, operator, modifiers)) = header else {
            return;
        };
        if !terminated {
            self.record(
                SyntaxErrorKind::UnterminatedDefine { name: name.clone() },
                statement.start_line,
                first_non_blank_column(statement.text()),
            );
        }

        let raw_body = self.source()[body_start..body_end].to_string();
        let value = raw_body
            .strip_suffix("\r\n")
            .or_else(|| raw_body.strip_suffix('\n'))
            .unwrap_or(&raw_body)
            .to_string();
        let variable = Variable {
            name: name.clone(),
            value: value.clone(),
            operator,
            modifiers,
            scope: VariableScope::Global,
            reach,
            line: statement.start_line,
            end_line,
            column: 1,
        };
        let deferred_in =
            value_expands_later(&self.makefile.assignments, &variable).then_some(name.as_str());
        let body_line = statement.end_line + 1;
        let error = unterminated_error(
            &raw_body,
            0..raw_body.len(),
            body_line,
            CommentHandling::Keep,
            deferred_in,
        );
        self.record_all(error);

        self.makefile.definitions.push(Definition {
            name: name.clone(),
            raw_body,
            value,
            operator,
            modifiers,
            reach,
            line: statement.start_line,
            end_line,
        });
        self.makefile.assignments.push(variable.clone());
        self.makefile.variables.insert(name, variable);
    }

    /// Handles a line that is neither a rule, an assignment, nor a directive.
    /// Make expands it first, so an unterminated reference wins; a line whose
    /// expansion could still produce a statement (or nothing) is accepted.
    /// `$$` and a trailing `$` expand to nothing that a statement needs.
    fn parse_unknown(&mut self, statement: &LogicalStatement) {
        let text = statement.text();
        let trimmed = text.trim_start();
        let raw = statement.raw(self.source());
        let unterminated = unterminated_error(
            raw,
            0..raw.len(),
            statement.start_line,
            CommentHandling::Strip,
            None,
        );
        if unterminated.is_some() {
            self.record_all(unterminated);
            return;
        }

        let prefix = self.recipe_prefix_at(statement.start_line);
        let hint = if starts_unspaced_conditional(trimmed) {
            // The line was meant to open a conditional, so the `endif` that
            // closes it is matched rather than reported as stray.
            self.open_conditionals.push(OpenConditional {
                header: strip_top_level_comment(text).trim().to_string(),
                line: statement.start_line,
                column: first_non_blank_column(text),
                else_seen: false,
            });
            Some(SeparatorHint::ConditionalWithoutSpace)
        } else if contains_reference(strip_top_level_comment(trimmed)) {
            return;
        } else if prefix.character == '\t' && text.starts_with("        ") {
            Some(SeparatorHint::SpacesInsteadOfTab)
        } else {
            None
        };
        // Any line is a recipe when it starts with the recipe prefix, so a
        // prefix Rumk could not follow makes the separator unprovable.
        if !prefix.known {
            return;
        }
        self.record(
            SyntaxErrorKind::MissingSeparator { hint },
            statement.start_line,
            1,
        );
    }

    fn attach_recipe(&mut self, statement: &LogicalStatement) {
        let raw = statement.raw(self.source());
        // The line is a recipe only under the prefix Rumk read it with, and
        // Make expands a recipe. A prefix Rumk could not follow leaves a second
        // reading where the line is an ordinary statement, and a reference in
        // an ordinary statement may never be expanded at all, so a broken one
        // here is not something Rumk can hold Make to.
        if self.recipe_prefix_at(statement.start_line).known {
            let error = unterminated_error(
                raw,
                0..raw.len(),
                statement.start_line,
                CommentHandling::Keep,
                None,
            );
            self.record_all(error);
        }

        let Some(index) = self.last_rule else {
            return;
        };
        let recipe_line = statement.text();
        let recipe_prefix = self.recipe_prefix_at(statement.start_line).character;
        let indentation_length = if recipe_line.starts_with(recipe_prefix) {
            recipe_prefix.len_utf8()
        } else {
            recipe_line.len() - recipe_line.trim_start().len()
        };
        let indentation = &recipe_line[..indentation_length];
        let command = &recipe_line[indentation_length..];
        let rule = &mut self.makefile.rules[index];
        rule.recipes.push(parse_recipe(
            command,
            statement.start_line,
            statement.end_line,
            indentation.chars().count() + 1,
            indentation,
            false,
        ));
        rule.end_line = statement.end_line;
    }

    fn parse_rule(&mut self, statement: &LogicalStatement) {
        let line = statement.text();
        let leading = line.len() - line.trim_start().len();
        let content = &line[leading..];
        let column = line[..leading].chars().count() + 1;
        let separator = find_top_level_rule_separator(content)
            .expect("a rule statement contains a rule separator");
        let targets = split_top_level_words(content[..separator.position].trim());
        let rule_body = &content[separator.position + separator.length..];

        // A '.ONESHELL:' in a branch whose literal condition fails is not a
        // setting Make ever holds. One in a branch decided at run time is a
        // setting Make may hold, and a recipe run under it keeps its directory,
        // so the file counts as declaring it.
        if statement.reach != Reach::Never && targets.iter().any(|target| target == ".ONESHELL") {
            self.makefile.oneshell = true;
        }

        if targets == [".PHONY"] {
            self.check_rule_line(statement, None);
            let prerequisites = strip_top_level_comment(rule_body);
            self.makefile
                .phonies
                .extend(split_top_level_words(prerequisites));
            self.last_rule = None;
            return;
        }

        let inline_separator = inline_recipe_separator(rule_body);
        let (prerequisite_text, inline_command) = match inline_separator {
            Some(position) => (&rule_body[..position], Some(&rule_body[position + 1..])),
            None => (rule_body, None),
        };
        let prerequisite_text = strip_top_level_comment(prerequisite_text);

        let mut target_pattern = None;
        let mut assigned_variable = None;
        let (normal_prerequisites, order_only_prerequisites) =
            if target_assignment(prerequisite_text).is_some() {
                match parse_variable(
                    prerequisite_text.trim(),
                    statement.start_line,
                    statement.end_line,
                    VariableScope::TargetSpecific(targets.clone()),
                    self.reach,
                ) {
                    Some(variable) => {
                        self.makefile.assignments.push(variable.clone());
                        assigned_variable = Some(variable);
                    }
                    // Make splits the expanded line, so a reference in the
                    // targets may supply the colon it splits at instead.
                    None if !contains_reference(&content[..separator.position]) => self.record(
                        SyntaxErrorKind::EmptyVariableName,
                        statement.start_line,
                        column,
                    ),
                    None => {}
                }
                ("", None)
            } else {
                let static_pattern = find_top_level_rule_separator(prerequisite_text);
                let prerequisites = if let Some(pattern_separator) = static_pattern {
                    target_pattern = Some(
                        prerequisite_text[..pattern_separator.position]
                            .trim()
                            .to_string(),
                    );
                    &prerequisite_text[pattern_separator.position + pattern_separator.length..]
                } else {
                    prerequisite_text
                };
                split_once_top_level(prerequisites, '|')
            };
        let prerequisites = split_top_level_words(normal_prerequisites);
        let order_only_prerequisites = order_only_prerequisites
            .map(split_top_level_words)
            .unwrap_or_default();

        let deferred_in = assigned_variable
            .as_ref()
            .filter(|variable| value_expands_later(&self.makefile.assignments, variable))
            .map(|variable| variable.name.clone());
        self.check_rule_line(statement, deferred_in.as_deref());

        let mut recipes = Vec::new();
        if let Some(command) = inline_command {
            if !command.trim().is_empty() {
                let command_column = line[..leading].chars().count()
                    + content[..separator.position + separator.length]
                        .chars()
                        .count()
                    + rule_body[..=inline_separator.expect("inline command has a separator")]
                        .chars()
                        .count()
                    + 1;
                recipes.push(parse_recipe(
                    command,
                    statement.start_line,
                    statement.end_line,
                    command_column,
                    "",
                    true,
                ));
            }
        }

        self.makefile.rules.push(Rule {
            targets,
            prerequisites,
            order_only_prerequisites,
            double_colon: separator.double_colon,
            grouped: separator.grouped,
            target_pattern,
            target_assignment: assigned_variable,
            recipes,
            line: statement.start_line,
            end_line: statement.end_line,
            column,
        });
        self.last_rule = Some(self.makefile.rules.len() - 1);
    }

    /// Checks the raw text of a rule line. Targets and prerequisites are
    /// expanded immediately with comments stripped; an inline recipe keeps
    /// its `#` and is expanded when the recipe runs, which for Make's error
    /// reporting is as good as immediately.
    fn check_rule_line(&mut self, statement: &LogicalStatement, deferred_in: Option<&str>) {
        let raw = statement.raw(self.source());
        let leading = raw.len() - raw.trim_start().len();
        let Some(separator) = find_top_level_rule_separator(&raw[leading..]) else {
            return;
        };
        let targets_end = leading + separator.position;
        let body_start = targets_end + separator.length;
        let body = &raw[body_start..];
        let (prerequisites_end, command_start) = match inline_recipe_separator(body) {
            Some(semicolon) => (body_start + semicolon, Some(body_start + semicolon + 1)),
            None => (raw.len(), None),
        };
        let start_line = statement.start_line;
        let mut errors = vec![unterminated_error(
            raw,
            0..targets_end,
            start_line,
            CommentHandling::Strip,
            None,
        )];
        let prerequisites = &raw[body_start..prerequisites_end];
        match target_assignment(strip_top_level_comment(prerequisites)) {
            Some((position, operator)) => {
                let operator_start = body_start + position;
                errors.push(unterminated_error(
                    raw,
                    body_start..operator_start,
                    start_line,
                    CommentHandling::Strip,
                    None,
                ));
                errors.push(unterminated_error(
                    raw,
                    operator_start + operator.len()..prerequisites_end,
                    start_line,
                    CommentHandling::Strip,
                    deferred_in,
                ));
            }
            None => errors.push(unterminated_error(
                raw,
                body_start..prerequisites_end,
                start_line,
                CommentHandling::Strip,
                None,
            )),
        }
        if let Some(command_start) = command_start {
            errors.push(unterminated_error(
                raw,
                command_start..raw.len(),
                start_line,
                CommentHandling::Keep,
                None,
            ));
        }
        self.record_all(errors.into_iter().flatten());
    }
}

impl AssignmentOperator {
    fn from_separator(separator: &str) -> Self {
        match separator {
            "=" => Self::Recursive,
            ":=" => Self::Simple,
            "::=" => Self::SimplePosix,
            ":::=" => Self::ImmediateRecursive,
            "?=" => Self::Conditional,
            "+=" => Self::Append,
            "!=" => Self::Shell,
            _ => unreachable!("assignment separator is validated before conversion"),
        }
    }
}

/// Records the first reference in `raw[range]` that Make cannot close, with
/// its position mapped back onto the file.
fn unterminated_error(
    raw: &str,
    range: Range<usize>,
    start_line: usize,
    comments: CommentHandling,
    deferred_in: Option<&str>,
) -> Option<SyntaxError> {
    let found = find_unterminated_reference(&raw[range.clone()], comments)?;
    let (line, column) = location_in(raw, range.start + found.offset, start_line);
    Some(SyntaxError {
        kind: SyntaxErrorKind::UnterminatedReference {
            closing: found.closing,
            function: found.function,
            deferred_in: deferred_in.map(str::to_string),
        },
        line,
        column,
    })
}

/// Maps a byte offset inside a statement's raw text to a line and column,
/// given the line on which the raw text starts.
fn location_in(raw: &str, offset: usize, start_line: usize) -> (usize, usize) {
    let before = &raw[..offset];
    let line = start_line + before.matches('\n').count();
    let last_line = before.rsplit_once('\n').map_or(before, |(_, tail)| tail);
    (line, last_line.chars().count() + 1)
}

fn first_non_blank_column(text: &str) -> usize {
    text.chars()
        .take_while(|character| character.is_whitespace())
        .count()
        + 1
}

/// Mirrors the Make hint for `ifeq(` and `ifneq(`: the keyword is followed
/// by something other than whitespace.
fn starts_unspaced_conditional(trimmed: &str) -> bool {
    ["ifeq", "ifneq"].iter().any(|keyword| {
        trimmed
            .strip_prefix(keyword)
            .is_some_and(|rest| !rest.starts_with(|character: char| character.is_whitespace()))
    })
}

/// Splits the left-hand side of an assignment into modifiers and the name.
/// A modifier keyword is only a modifier when a name follows it: `export =
/// 1` assigns a variable called `export`, as GNU Make 4.3 and later read it.
fn parse_variable_name(left_hand_side: &str) -> (String, VariableModifiers) {
    let mut modifiers = VariableModifiers::default();
    let words: Vec<&str> = left_hand_side.split_whitespace().collect();
    let mut first = 0;

    while first + 1 < words.len() {
        match words[first] {
            "export" => modifiers.export = true,
            "unexport" => modifiers.unexport = true,
            "override" => modifiers.override_ = true,
            "private" => modifiers.private = true,
            _ => break,
        }
        first += 1;
    }

    (words[first..].join(" "), modifiers)
}

/// Parses an assignment. Returns `None` when no name precedes the operator.
fn parse_variable(
    source: &str,
    line: usize,
    end_line: usize,
    scope: VariableScope,
    reach: Reach,
) -> Option<Variable> {
    let leading = source.len() - source.trim_start().len();
    let content = &source[leading..];
    let (separator_position, separator) = find_top_level_assignment(content)?;
    let (name, modifiers) = parse_variable_name(&content[..separator_position]);
    if name.is_empty() {
        return None;
    }
    let raw_value = content[separator_position + separator.len()..].trim();
    let value = strip_top_level_comment(raw_value).trim_end().to_string();

    Some(Variable {
        name,
        value,
        operator: AssignmentOperator::from_separator(separator),
        modifiers,
        scope,
        reach,
        line,
        end_line,
        column: source[..leading].chars().count() + 1,
    })
}

/// Parses a `define` header. Returns `None` when no variable name follows
/// the keyword.
fn parse_definition_header(
    source: &str,
) -> Option<(String, AssignmentOperator, VariableModifiers)> {
    let mut remaining = source.trim_start();
    let mut modifiers = VariableModifiers::default();

    loop {
        if let Some(rest) = strip_word(remaining, "export") {
            modifiers.export = true;
            remaining = rest;
        } else if let Some(rest) = strip_word(remaining, "unexport") {
            modifiers.unexport = true;
            remaining = rest;
        } else if let Some(rest) = strip_word(remaining, "override") {
            modifiers.override_ = true;
            remaining = rest;
        } else if let Some(rest) = strip_word(remaining, "private") {
            modifiers.private = true;
            remaining = rest;
        } else {
            break;
        }
    }

    remaining = strip_word(remaining, "define")?;
    let (name, operator) = if let Some((position, separator)) = find_top_level_assignment(remaining)
    {
        (
            remaining[..position].trim(),
            AssignmentOperator::from_separator(separator),
        )
    } else {
        (remaining.trim(), AssignmentOperator::Recursive)
    };
    if name.is_empty() {
        return None;
    }

    Some((name.to_string(), operator, modifiers))
}

fn strip_word<'a>(source: &'a str, word: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(word)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

fn parse_recipe(
    source: &str,
    line: usize,
    end_line: usize,
    column: usize,
    indentation: &str,
    inline: bool,
) -> Recipe {
    let mut command = source.trim_start();
    let mut command_column = column + source[..source.len() - command.len()].chars().count();
    let mut silent = false;
    let mut ignore_errors = false;
    let mut recursive = false;

    loop {
        match command.chars().next() {
            Some('@') => silent = true,
            Some('-') => ignore_errors = true,
            Some('+') => recursive = true,
            _ => break,
        }
        command = &command[1..];
        command_column += 1;
    }

    Recipe {
        command: command.to_string(),
        inline,
        silent,
        ignore_errors,
        recursive,
        line,
        end_line,
        column: command_column,
        indentation: indentation.to_string(),
    }
}
