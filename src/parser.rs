use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Makefile {
    pub rules: Vec<Rule>,
    pub variables: HashMap<String, Variable>,
    pub includes: Vec<Include>,
    pub exports: Vec<String>,
    pub phonies: Vec<String>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<String>,
    pub prerequisites: Vec<String>,
    pub recipes: Vec<Recipe>,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub command: String,
    pub silent: bool,
    pub ignore_error: bool,
    pub line: usize,
    pub column: usize,
    pub indentation: String,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub assignment_type: AssignmentType,
    pub line: usize,
    pub column: usize,
    pub export: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentType {
    Simple,      // =
    Recursive,   // :=
    Conditional, // ?=
    Append,      // +=
}

#[derive(Debug, Clone)]
pub struct Include {
    pub path: String,
    pub optional: bool,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub text: String,
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
                includes: Vec::new(),
                exports: Vec::new(),
                phonies: Vec::new(),
                comments: Vec::new(),
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
                self.parse_comment(line)?;
            } else if trimmed.starts_with("include ") || trimmed.starts_with("-include ") {
                self.parse_include(line)?;
            } else if trimmed.starts_with("export ") {
                self.parse_export(line)?;
            } else if trimmed.starts_with(".PHONY:") {
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

    fn parse_comment(&mut self, line: &str) -> Result<()> {
        let column = line.len() - line.trim_start().len() + 1;
        let text = line
            .trim_start()
            .trim_start_matches('#')
            .trim_start()
            .to_string();

        self.makefile.comments.push(Comment {
            text,
            line: self.current_line + 1,
            column,
        });

        self.current_line += 1;
        Ok(())
    }

    fn parse_include(&mut self, line: &str) -> Result<()> {
        let optional = line.trim_start().starts_with("-include");
        let column = line.len() - line.trim_start().len() + 1;

        let path = if optional {
            line.trim_start().trim_start_matches("-include").trim()
        } else {
            line.trim_start().trim_start_matches("include").trim()
        };

        self.makefile.includes.push(Include {
            path: path.to_string(),
            optional,
            line: self.current_line + 1,
            column,
        });

        self.current_line += 1;
        Ok(())
    }

    fn parse_export(&mut self, line: &str) -> Result<()> {
        let export_content = line.trim_start().trim_start_matches("export").trim();

        if export_content.contains('=') {
            self.parse_variable(line)?;
        } else {
            self.makefile.exports.push(export_content.to_string());
            self.current_line += 1;
        }

        Ok(())
    }

    fn parse_phony(&mut self, line: &str) -> Result<()> {
        let targets = line
            .trim_start()
            .trim_start_matches(".PHONY:")
            .trim()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        self.makefile.phonies.extend(targets);
        self.current_line += 1;
        Ok(())
    }

    fn is_variable_assignment(&self, line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }

        trimmed.contains("=") && !trimmed.contains(':')
    }

    fn parse_variable(&mut self, line: &str) -> Result<()> {
        let column = line.len() - line.trim_start().len() + 1;
        let export = line.trim_start().starts_with("export ");
        let content = if export {
            line.trim_start().trim_start_matches("export").trim()
        } else {
            line.trim_start()
        };

        let (assignment_type, sep) = if content.contains(":=") {
            (AssignmentType::Recursive, ":=")
        } else if content.contains("?=") {
            (AssignmentType::Conditional, "?=")
        } else if content.contains("+=") {
            (AssignmentType::Append, "+=")
        } else {
            (AssignmentType::Simple, "=")
        };

        let parts: Vec<&str> = content.splitn(2, sep).collect();
        if parts.len() != 2 {
            bail!(
                "Invalid variable assignment at line {}",
                self.current_line + 1
            );
        }

        let name = parts[0].trim().to_string();
        let mut value = parts[1].trim().to_string();

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
                assignment_type,
                line: self.current_line + 1,
                column,
                export,
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
        let column = line.len() - line.trim_start().len() + 1;

        let colon_pos = line.find(':').unwrap();
        let targets_str = line[..colon_pos].trim();
        let prerequisites_str = line[colon_pos + 1..].trim();

        let targets: Vec<String> = targets_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let prerequisites: Vec<String> = if prerequisites_str.is_empty() {
            Vec::new()
        } else {
            prerequisites_str
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        };

        let mut recipes = Vec::new();
        self.current_line += 1;

        while self.current_line < self.lines.len() {
            let recipe_line = self.lines[self.current_line];

            if recipe_line.starts_with('\t') || recipe_line.starts_with(' ') {
                let mut command = recipe_line.trim_start().to_string();
                let indentation =
                    &recipe_line[..recipe_line.len() - recipe_line.trim_start().len()];

                let mut silent = false;
                let mut ignore_error = false;

                if command.starts_with('@') {
                    silent = true;
                    command = command[1..].to_string();
                }

                if command.starts_with('-') {
                    ignore_error = true;
                    command = command[1..].to_string();
                }

                recipes.push(Recipe {
                    command,
                    silent,
                    ignore_error,
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
            prerequisites,
            recipes,
            line: self.current_line + 1,
            column,
        });

        Ok(())
    }
}
