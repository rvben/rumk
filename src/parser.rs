use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::analysis::SemanticIndex;
use crate::logical::{
    find_top_level_assignment, find_top_level_char, find_top_level_rule_separator,
    split_top_level_words, ConditionalKind, IncludeKind, LogicalDocument, LogicalKind,
    LogicalStatement,
};
use crate::syntax::SyntaxTree;

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
    analysis: OnceLock<SemanticIndex>,
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VariableModifiers {
    pub export: bool,
    pub unexport: bool,
    pub override_: bool,
    pub private: bool,
}

pub fn parse(content: &str) -> Result<Makefile> {
    Parser::new(content).parse()
}

impl Makefile {
    /// Returns the semantic index, building it once on first use.
    pub fn analysis(&self) -> &SemanticIndex {
        self.analysis.get_or_init(|| SemanticIndex::build(self))
    }
}

struct Parser {
    current_statement: usize,
    makefile: Makefile,
    recipe_prefix: char,
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
                analysis: OnceLock::new(),
            },
            recipe_prefix: '\t',
        }
    }

    fn parse(mut self) -> Result<Makefile> {
        while let Some(statement) = self.statement().cloned() {
            match statement.kind {
                LogicalKind::Assignment => self.parse_global_variable(&statement)?,
                LogicalKind::Rule => self.parse_rule(&statement)?,
                LogicalKind::Include(kind) => self.parse_include(&statement, kind),
                LogicalKind::Conditional(kind) => self.parse_conditional(&statement, kind),
                LogicalKind::Define => self.parse_definition(&statement)?,
                _ => self.current_statement += 1,
            }
        }

        Ok(self.makefile)
    }

    fn statement(&self) -> Option<&LogicalStatement> {
        self.makefile
            .logical
            .statements()
            .get(self.current_statement)
    }

    fn parse_global_variable(&mut self, statement: &LogicalStatement) -> Result<()> {
        let variable = parse_variable(
            statement.text(),
            statement.start_line,
            statement.end_line,
            VariableScope::Global,
        )?;

        if variable.name == ".RECIPEPREFIX" {
            self.recipe_prefix = variable.value.chars().next().unwrap_or('\t');
        }
        self.makefile
            .variables
            .insert(variable.name.clone(), variable.clone());
        self.makefile.assignments.push(variable);
        self.current_statement += 1;
        Ok(())
    }

    fn parse_include(&mut self, statement: &LogicalStatement, kind: IncludeKind) {
        let text = statement.text().trim_start();
        let keyword_length = text.find(char::is_whitespace).unwrap_or(text.len());
        let paths = split_top_level_words(strip_top_level_comment(text[keyword_length..].trim()));
        self.makefile.includes.push(Include {
            paths,
            optional: kind == IncludeKind::Optional,
            line: statement.start_line,
            end_line: statement.end_line,
        });
        self.current_statement += 1;
    }

    fn parse_conditional(&mut self, statement: &LogicalStatement, kind: ConditionalKind) {
        let text = statement.text().trim_start();
        let keyword_length = text.find(char::is_whitespace).unwrap_or(text.len());
        self.makefile.conditionals.push(Conditional {
            kind,
            expression: text[keyword_length..].trim().to_string(),
            line: statement.start_line,
            end_line: statement.end_line,
        });
        self.current_statement += 1;
    }

    fn parse_definition(&mut self, statement: &LogicalStatement) -> Result<()> {
        let (name, operator, modifiers) = parse_definition_header(statement.text())?;
        let mut depth = 1usize;
        let body_start = statement.span.end.offset;
        let mut body_end = body_start;
        let mut end_line = statement.end_line;
        self.current_statement += 1;

        while let Some(next) = self.statement() {
            match next.kind {
                LogicalKind::Define => depth += 1,
                LogicalKind::Endef => {
                    depth -= 1;
                    if depth == 0 {
                        end_line = next.end_line;
                        break;
                    }
                }
                _ => {}
            }
            body_end = next.span.end.offset;
            self.current_statement += 1;
        }

        if depth != 0 {
            bail!(
                "Unterminated define directive at line {}",
                statement.start_line
            );
        }

        let source = self.makefile.syntax.source();
        let raw_body = source[body_start..body_end].to_string();
        let value = raw_body
            .strip_suffix("\r\n")
            .or_else(|| raw_body.strip_suffix('\n'))
            .unwrap_or(&raw_body)
            .to_string();
        let definition = Definition {
            name: name.clone(),
            raw_body,
            value: value.clone(),
            operator,
            modifiers,
            line: statement.start_line,
            end_line,
        };
        self.makefile.definitions.push(definition);
        let variable = Variable {
            name: name.clone(),
            value,
            operator,
            modifiers,
            scope: VariableScope::Global,
            line: statement.start_line,
            end_line,
            column: 1,
        };
        self.makefile.assignments.push(variable.clone());
        self.makefile.variables.insert(name, variable);
        self.current_statement += 1;
        Ok(())
    }

    fn parse_rule(&mut self, statement: &LogicalStatement) -> Result<()> {
        let line = statement.text();
        let leading = line.len() - line.trim_start().len();
        let content = &line[leading..];
        let column = line[..leading].chars().count() + 1;
        let separator = find_top_level_rule_separator(content)
            .ok_or_else(|| anyhow::anyhow!("Invalid rule at line {}", statement.start_line))?;
        let targets = split_top_level_words(content[..separator.position].trim());
        let rule_body = &content[separator.position + separator.length..];

        if targets.iter().any(|target| target == ".ONESHELL") {
            self.makefile.oneshell = true;
        }

        if targets == [".PHONY"] {
            let prerequisites = strip_top_level_comment(rule_body);
            self.makefile
                .phonies
                .extend(split_top_level_words(prerequisites));
            self.current_statement += 1;
            return Ok(());
        }

        let inline_separator = find_top_level_char(rule_body, ';');
        let (prerequisite_text, inline_command) = split_once_top_level(rule_body, ';');
        let prerequisite_text = strip_top_level_comment(prerequisite_text);

        let mut target_pattern = None;
        let mut target_assignment = None;
        let assignment = find_top_level_assignment(prerequisite_text);
        let static_pattern = find_top_level_rule_separator(prerequisite_text);
        let (normal_prerequisites, order_only_prerequisites) = if let Some((position, _)) =
            assignment.filter(|(position, _)| {
                static_pattern.is_none_or(|rule| *position <= rule.position)
            }) {
            let _ = position;
            let assignment_text = prerequisite_text.trim();
            let variable = parse_variable(
                assignment_text,
                statement.start_line,
                statement.end_line,
                VariableScope::TargetSpecific(targets.clone()),
            )?;
            self.makefile.assignments.push(variable.clone());
            target_assignment = Some(variable);
            ("", None)
        } else {
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
        self.current_statement += 1;

        while let Some(recipe_statement) = self.statement().cloned() {
            if matches!(
                recipe_statement.kind,
                LogicalKind::Blank | LogicalKind::Comment
            ) {
                self.current_statement += 1;
                continue;
            }

            let recipe_line = recipe_statement.text();
            let is_recipe = recipe_statement.kind == LogicalKind::Recipe
                || (recipe_line.starts_with(' ') && recipe_statement.kind == LogicalKind::Unknown);
            if is_recipe {
                let indentation_length = if recipe_line.starts_with(self.recipe_prefix) {
                    self.recipe_prefix.len_utf8()
                } else {
                    recipe_line.len() - recipe_line.trim_start().len()
                };
                let indentation = &recipe_line[..indentation_length];
                let command = &recipe_line[indentation_length..];

                recipes.push(parse_recipe(
                    command,
                    recipe_statement.start_line,
                    recipe_statement.end_line,
                    indentation.chars().count() + 1,
                    indentation,
                    false,
                ));
                self.current_statement += 1;
            } else {
                break;
            }
        }

        let end_line = recipes_end_line(&recipes, statement.end_line);
        self.makefile.rules.push(Rule {
            targets,
            prerequisites,
            order_only_prerequisites,
            double_colon: separator.double_colon,
            grouped: separator.grouped,
            target_pattern,
            target_assignment,
            recipes,
            line: statement.start_line,
            end_line,
            column,
        });

        Ok(())
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

fn parse_variable_name(left_hand_side: &str) -> (String, VariableModifiers) {
    let mut modifiers = VariableModifiers::default();
    let mut words = left_hand_side.split_whitespace().peekable();

    while let Some(word) = words.peek().copied() {
        let recognized = match word {
            "export" => {
                modifiers.export = true;
                true
            }
            "unexport" => {
                modifiers.unexport = true;
                true
            }
            "override" => {
                modifiers.override_ = true;
                true
            }
            "private" => {
                modifiers.private = true;
                true
            }
            _ => false,
        };
        if !recognized {
            break;
        }
        words.next();
    }

    (words.collect::<Vec<_>>().join(" "), modifiers)
}

fn parse_variable(
    source: &str,
    line: usize,
    end_line: usize,
    scope: VariableScope,
) -> Result<Variable> {
    let leading = source.len() - source.trim_start().len();
    let content = &source[leading..];
    let Some((separator_position, separator)) = find_top_level_assignment(content) else {
        bail!("Invalid variable assignment at line {line}");
    };
    let (name, modifiers) = parse_variable_name(&content[..separator_position]);
    if name.is_empty() {
        bail!("Variable assignment has no name at line {line}");
    }
    let raw_value = content[separator_position + separator.len()..].trim();
    let value = strip_top_level_comment(raw_value).trim_end().to_string();

    Ok(Variable {
        name,
        value,
        operator: AssignmentOperator::from_separator(separator),
        modifiers,
        scope,
        line,
        end_line,
        column: source[..leading].chars().count() + 1,
    })
}

fn parse_definition_header(
    source: &str,
) -> Result<(String, AssignmentOperator, VariableModifiers)> {
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

    remaining = strip_word(remaining, "define")
        .ok_or_else(|| anyhow::anyhow!("Invalid define directive"))?;
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
        bail!("Define directive has no variable name");
    }

    Ok((name.to_string(), operator, modifiers))
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

fn recipes_end_line(recipes: &[Recipe], fallback: usize) -> usize {
    recipes.last().map_or(fallback, |recipe| recipe.end_line)
}

fn strip_top_level_comment(line: &str) -> &str {
    split_once_top_level(line, '#').0
}

fn split_once_top_level(line: &str, separator: char) -> (&str, Option<&str>) {
    if let Some(index) = find_top_level_char(line, separator) {
        let after = index + separator.len_utf8();
        (&line[..index], Some(&line[after..]))
    } else {
        (line, None)
    }
}
