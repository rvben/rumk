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

            if trimmed.starts_with('.') || trimmed.starts_with('#') {
                // Skip comments and special directives except .PHONY
                if trimmed.starts_with(".PHONY:") {
                    self.parse_phony(line)?;
                } else {
                    self.current_line += 1;
                }
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
            .trim_start()
            .trim_start_matches(".PHONY:")
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
        let content = line.trim_start();

        let sep = if content.contains(":=") {
            ":="
        } else if content.contains("?=") {
            "?="
        } else if content.contains("+=") {
            "+="
        } else {
            "="
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
                line: self.current_line + 1,
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
        let column = line.len() - line.trim_start().len() + 1;

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
            line: self.current_line + 1,
            column,
        });

        Ok(())
    }
}
