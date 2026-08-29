use anyhow::{bail, Result};
use std::collections::HashMap;

use crate::syntax::SyntaxTree;

#[derive(Debug, Clone)]
pub struct Makefile {
    /// Lossless, source-ordered syntax for tools that need exact text or spans.
    pub syntax: SyntaxTree,
    pub rules: Vec<Rule>,
    /// All assignments in source order. `variables` remains a last-value lookup.
    pub assignments: Vec<Variable>,
    pub variables: HashMap<String, Variable>,
    pub phonies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<String>,
    pub prerequisites: Vec<String>,
    pub order_only_prerequisites: Vec<String>,
    pub double_colon: bool,
    pub grouped: bool,
    pub recipes: Vec<Recipe>,
    pub line: usize,
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
    pub column: usize,
    pub indentation: String,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub operator: AssignmentOperator,
    pub modifiers: VariableModifiers,
    pub line: usize,
    pub column: usize,
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
    let mut parser = Parser::new(content);
    parser.parse()
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    current_line: usize,
    makefile: Makefile,
    recipe_prefix: char,
}

impl<'a> Parser<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            lines: content.lines().collect(),
            current_line: 0,
            makefile: Makefile {
                syntax: SyntaxTree::parse(content),
                rules: Vec::new(),
                assignments: Vec::new(),
                variables: HashMap::new(),
                phonies: Vec::new(),
            },
            recipe_prefix: '\t',
        }
    }

    fn parse(&mut self) -> Result<Makefile> {
        while self.current_line < self.lines.len() {
            let line = self.lines[self.current_line];
            let trimmed = line.trim_start();

            if trimmed.is_empty() {
                self.current_line += 1;
                continue;
            }

            if trimmed.starts_with('#') {
                self.current_line += 1;
            } else if Self::is_phony_line(line) {
                self.parse_phony(line)?;
            } else if self.is_variable_assignment(line) {
                self.parse_variable(line)?;
            } else if self.is_rule_line(line) {
                self.parse_rule()?;
            } else {
                self.current_line += 1;
            }
        }

        Ok(self.makefile.clone())
    }

    fn parse_phony(&mut self, line: &str) -> Result<()> {
        let targets = line
            .split_once(':')
            .map(|(_, prerequisites)| prerequisites)
            .unwrap_or_default()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        self.makefile.phonies.extend(targets);
        self.current_line += 1;
        Ok(())
    }

    fn is_phony_line(line: &str) -> bool {
        line.split_once(':')
            .is_some_and(|(target, _)| target.trim() == ".PHONY")
    }

    fn is_variable_assignment(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }

        assignment_separator(trimmed).is_some_and(|(position, _)| {
            let name = trimmed[..position].trim();
            !name.is_empty() && !name.contains(':')
        })
    }

    fn parse_variable(&mut self, line: &str) -> Result<()> {
        let column = line[..line.len() - line.trim_start().len()].chars().count() + 1;
        let content = line.trim_start();
        let assignment_line = self.current_line + 1;
        let Some((separator_position, separator)) = assignment_separator(content) else {
            bail!("Invalid variable assignment at line {assignment_line}");
        };

        let (name, modifiers) = parse_variable_name(&content[..separator_position]);
        let mut value = content[separator_position + separator.len()..]
            .trim()
            .to_string();

        while self.current_line + 1 < self.lines.len()
            && self.lines[self.current_line].ends_with('\\')
        {
            value.pop();
            self.current_line += 1;
            value.push_str(self.lines[self.current_line].trim_start());
        }

        let variable = Variable {
            name: name.clone(),
            value: value.clone(),
            operator: AssignmentOperator::from_separator(separator),
            modifiers,
            line: assignment_line,
            column,
        };
        self.makefile.assignments.push(variable.clone());
        self.makefile.variables.insert(name.clone(), variable);

        if name == ".RECIPEPREFIX" {
            self.recipe_prefix = value.chars().next().unwrap_or('\t');
        }

        self.current_line += 1;
        Ok(())
    }

    fn is_rule_line(&self, line: &str) -> bool {
        let trimmed = line.trim();
        !trimmed.is_empty() && trimmed.contains(':') && !trimmed.starts_with('\t')
    }

    fn parse_rule(&mut self) -> Result<()> {
        let line = self.lines[self.current_line];
        let rule_line = self.current_line + 1;
        let column = line[..line.len() - line.trim_start().len()].chars().count() + 1;

        let colon_pos = line.find(':').unwrap();
        let before_colon = line[..colon_pos].trim_end();
        let grouped = before_colon.ends_with('&');
        let targets_str = before_colon
            .strip_suffix('&')
            .unwrap_or(before_colon)
            .trim();
        let double_colon = line[colon_pos + 1..].starts_with(':');
        let separator_end = colon_pos + if double_colon { 2 } else { 1 };

        let targets = split_make_words(targets_str);

        let rule_body = &line[separator_end..];
        let inline_separator = find_unescaped(rule_body, ';');
        let (prerequisite_text, inline_command) = split_once_unescaped(rule_body, ';');
        let prerequisite_text = strip_unescaped_comment(prerequisite_text);
        let (normal_prerequisites, order_only_prerequisites) =
            split_once_unescaped(prerequisite_text, '|');
        let prerequisites = split_make_words(normal_prerequisites);
        let order_only_prerequisites = order_only_prerequisites
            .map(split_make_words)
            .unwrap_or_default();

        let mut recipes = Vec::new();
        if let Some(command) = inline_command {
            if !command.trim().is_empty() {
                let command_column = line[..separator_end].chars().count()
                    + rule_body[..=inline_separator.expect("inline command has a separator")]
                        .chars()
                        .count()
                    + 1;
                recipes.push(parse_recipe(command, rule_line, command_column, "", true));
            }
        }
        self.current_line += 1;

        while self.current_line < self.lines.len() {
            let recipe_line = self.lines[self.current_line];

            if recipe_line.trim_start().starts_with('#') {
                self.current_line += 1;
            } else if recipe_line.starts_with(self.recipe_prefix) || recipe_line.starts_with(' ') {
                let indentation_length = if recipe_line.starts_with(self.recipe_prefix) {
                    self.recipe_prefix.len_utf8()
                } else {
                    recipe_line.len() - recipe_line.trim_start().len()
                };
                let indentation = &recipe_line[..indentation_length];
                let command = &recipe_line[indentation_length..];

                recipes.push(parse_recipe(
                    command,
                    self.current_line + 1,
                    indentation.chars().count() + 1,
                    indentation,
                    false,
                ));

                self.current_line += 1;
            } else if recipe_line.trim().is_empty() {
                self.current_line += 1;
            } else {
                break;
            }
        }

        self.makefile.rules.push(Rule {
            targets,
            prerequisites,
            order_only_prerequisites,
            double_colon,
            grouped,
            recipes,
            line: rule_line,
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

fn parse_recipe(
    source: &str,
    line: usize,
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
        column: command_column,
        indentation: indentation.to_string(),
    }
}

fn strip_unescaped_comment(line: &str) -> &str {
    split_once_unescaped(line, '#').0
}

fn split_once_unescaped(line: &str, separator: char) -> (&str, Option<&str>) {
    if let Some(index) = find_unescaped(line, separator) {
        let after = index + separator.len_utf8();
        (&line[..index], Some(&line[after..]))
    } else {
        (line, None)
    }
}

fn find_unescaped(line: &str, separator: char) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character == separator && !escaped {
            return Some(index);
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    None
}

fn split_make_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
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

fn assignment_separator(line: &str) -> Option<(usize, &'static str)> {
    const SEPARATORS: [&str; 7] = [":::=", "::=", ":=", "?=", "+=", "!=", "="];

    SEPARATORS
        .iter()
        .filter_map(|separator| line.find(separator).map(|position| (position, *separator)))
        .min_by(
            |(left_position, left_separator), (right_position, right_separator)| {
                left_position
                    .cmp(right_position)
                    .then_with(|| right_separator.len().cmp(&left_separator.len()))
            },
        )
}
