use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Makefile {
    pub rules: Vec<Rule>,
    pub variables: HashMap<String, Variable>,
    pub phonies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<String>,
    pub recipes: Vec<Recipe>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub command: String,
    pub line: usize,
    pub column: usize,
    pub indentation: String,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub line: usize,
    pub column: usize,
}

pub fn parse(content: &str) -> Result<Makefile> {
    let mut parser = Parser::new(content);
    parser.parse()
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    current_line: usize,
    makefile: Makefile,
}

impl<'a> Parser<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            lines: content.lines().collect(),
            current_line: 0,
            makefile: Makefile {
                rules: Vec::new(),
                variables: HashMap::new(),
                phonies: Vec::new(),
            },
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

        let name = content[..separator_position].trim().to_string();
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

        self.makefile.variables.insert(
            name.clone(),
            Variable {
                name,
                value,
                line: assignment_line,
                column,
            },
        );

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
        let targets_str = line[..colon_pos].trim();

        let targets: Vec<String> = targets_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut recipes = Vec::new();
        self.current_line += 1;

        while self.current_line < self.lines.len() {
            let recipe_line = self.lines[self.current_line];

            if recipe_line.starts_with('\t') || recipe_line.starts_with(' ') {
                let mut command = recipe_line.trim_start().to_string();
                let indentation =
                    &recipe_line[..recipe_line.len() - recipe_line.trim_start().len()];

                if command.starts_with('@') {
                    command = command[1..].to_string();
                }

                if command.starts_with('-') {
                    command = command[1..].to_string();
                }

                recipes.push(Recipe {
                    command,
                    line: self.current_line + 1,
                    column: 1,
                    indentation: indentation.to_string(),
                });

                self.current_line += 1;
            } else if recipe_line.trim().is_empty() {
                self.current_line += 1;
            } else {
                break;
            }
        }

        self.makefile.rules.push(Rule {
            targets,
            recipes,
            line: rule_line,
            column,
        });

        Ok(())
    }
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
