use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::Makefile;
use crate::project::Project;
use crate::rules::{Rule, RuleCategory};

pub struct MissingPhony;

impl Rule for MissingPhony {
    fn id(&self) -> &'static str {
        "MK201"
    }

    fn name(&self) -> &'static str {
        "Non-file targets should be .PHONY"
    }

    fn description(&self) -> &'static str {
        "Targets that don't represent actual files should be declared as .PHONY to ensure \
         they always run and to improve performance."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let common_phony_targets = ["all", "clean", "test", "check", "install", "build", "help"];

        for rule in &makefile.rules {
            for target in &rule.targets {
                if common_phony_targets.contains(&target.as_str())
                    && !makefile.phonies.contains(target)
                {
                    diagnostics.push(Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Target '{target}' should be declared .PHONY"),
                        rule.line,
                        rule.column,
                    ));
                }
            }
        }

        diagnostics
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let common = ["all", "clean", "test", "check", "install", "build", "help"];
        let index = project.analysis();
        project
            .analysis()
            .targets
            .values()
            .filter(|target| common.contains(&target.name.as_str()) && !target.phony)
            .filter_map(|target| {
                let declaration = target
                    .declarations
                    .iter()
                    .find(|declaration| !index.is_conditional_location(declaration.location))?;
                Some(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Target '{}' should be declared .PHONY", target.name),
                        declaration.location.line,
                        declaration.location.column,
                    )
                    .with_source(project.file(declaration.location.source).path.clone()),
                )
            })
            .collect()
    }
}

pub struct HardcodedPath;

impl Rule for HardcodedPath {
    fn id(&self) -> &'static str {
        "MK202"
    }

    fn name(&self) -> &'static str {
        "Avoid hardcoded absolute paths"
    }

    fn description(&self) -> &'static str {
        "Hardcoded absolute paths reduce portability and make the Makefile less flexible. \
         Use variables or relative paths instead."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for variable in &makefile.assignments {
            if contains_absolute_path(&variable.value) {
                diagnostics.push(Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!(
                        "Variable '{}' contains hardcoded absolute path",
                        variable.name
                    ),
                    variable.line,
                    variable.column,
                ));
            }
        }

        for rule in &makefile.rules {
            for recipe in &rule.recipes {
                if contains_absolute_path(&recipe.command) {
                    diagnostics.push(Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        "Recipe contains hardcoded absolute path",
                        recipe.line,
                        recipe.column,
                    ));
                }
            }
        }

        diagnostics
    }
}

fn contains_absolute_path(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        (word.starts_with('/') && word.len() > 1 && !word.starts_with("//"))
            || (word.len() > 2
                && word.chars().nth(1) == Some(':')
                && word.chars().nth(2) == Some('\\'))
    })
}

pub struct RecursiveMake;

impl Rule for RecursiveMake {
    fn id(&self) -> &'static str {
        "MK203"
    }

    fn name(&self) -> &'static str {
        "Use $(MAKE) for recursive invocations"
    }

    fn description(&self) -> &'static str {
        "Recursive Make invocations should use $(MAKE) so jobserver flags, command-line options, and special recursive behavior are preserved."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        makefile
            .rules
            .iter()
            .flat_map(|rule| &rule.recipes)
            .filter(|recipe| invokes_bare_make(&recipe.command))
            .map(|recipe| {
                Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    "Use $(MAKE) instead of invoking make directly",
                    recipe.line,
                    recipe.column,
                )
            })
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ShellToken {
    Word { text: String, quoted: bool },
    Separator,
}

fn invokes_bare_make(command: &str) -> bool {
    let mut command_position = true;
    for token in shell_tokens(command) {
        match token {
            ShellToken::Separator => command_position = true,
            ShellToken::Word { text, quoted } if command_position => {
                if is_environment_assignment(&text)
                    || matches!(text.as_str(), "command" | "exec" | "env" | "sudo" | "time")
                {
                    continue;
                }
                if !quoted && is_make_executable(&text) {
                    return true;
                }
                command_position = matches!(text.as_str(), "if" | "then" | "else" | "do");
            }
            ShellToken::Word { .. } => {}
        }
    }
    false
}

fn shell_tokens(command: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut quoted = false;
    let mut escaped = false;

    let flush = |tokens: &mut Vec<ShellToken>, current: &mut String, quoted: &mut bool| {
        if !current.is_empty() {
            tokens.push(ShellToken::Word {
                text: std::mem::take(current),
                quoted: *quoted,
            });
            *quoted = false;
        }
    };

    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            quoted = true;
        } else if character.is_whitespace() {
            flush(&mut tokens, &mut current, &mut quoted);
            if character == '\n' {
                tokens.push(ShellToken::Separator);
            }
        } else if matches!(character, ';' | '|' | '&') {
            flush(&mut tokens, &mut current, &mut quoted);
            if !matches!(tokens.last(), Some(ShellToken::Separator)) {
                tokens.push(ShellToken::Separator);
            }
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    flush(&mut tokens, &mut current, &mut quoted);
    tokens
}

fn is_environment_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
            && !name.starts_with(|character: char| character.is_ascii_digit())
    })
}

fn is_make_executable(word: &str) -> bool {
    word.rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| matches!(name, "make" | "gmake"))
}

pub struct DuplicateRecipe;

impl Rule for DuplicateRecipe {
    fn id(&self) -> &'static str {
        "MK204"
    }

    fn name(&self) -> &'static str {
        "Duplicate recipe for target"
    }

    fn description(&self) -> &'static str {
        "A concrete target should not have multiple recipes unless it deliberately uses double-colon rules."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let index = makefile.analysis();
        if !index.structural_issues.is_empty() {
            return Vec::new();
        }
        index
            .targets
            .values()
            .filter(|target| !target.name.contains(['$', '%']))
            .flat_map(|target| {
                let mut recipes = target.declarations.iter().filter(|declaration| {
                    declaration.has_recipe
                        && !declaration.double_colon
                        && !index.is_conditional_line(declaration.location.line)
                });
                recipes.next();
                recipes.map(|declaration| {
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Target '{}' has more than one recipe", target.name),
                        declaration.location.line,
                        declaration.location.column,
                    )
                })
            })
            .collect()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let index = project.analysis();
        if index.has_structural_issues() {
            return Vec::new();
        }
        index
            .targets
            .values()
            .filter(|target| !target.name.contains(['$', '%']))
            .flat_map(|target| {
                let mut recipes = target.declarations.iter().filter(|declaration| {
                    declaration.has_recipe
                        && !declaration.double_colon
                        && !index.is_conditional_location(declaration.location)
                });
                recipes.next();
                recipes.map(|declaration| {
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Target '{}' has more than one recipe", target.name),
                        declaration.location.line,
                        declaration.location.column,
                    )
                    .with_source(project.file(declaration.location.source).path.clone())
                })
            })
            .collect()
    }
}

pub struct DependencyCycle;

impl Rule for DependencyCycle {
    fn id(&self) -> &'static str {
        "MK205"
    }

    fn name(&self) -> &'static str {
        "Circular target dependency"
    }

    fn description(&self) -> &'static str {
        "Explicit target dependencies must not form cycles, which Make otherwise drops at runtime."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let index = makefile.analysis();
        index
            .dependency_cycles()
            .into_iter()
            .filter_map(|cycle| {
                let target = index.target(&cycle[0])?;
                let declaration = target.declarations.first()?;
                let description = if cycle.len() == 1 {
                    format!("'{}' depends on itself", cycle[0])
                } else {
                    format!("targets {} form a cycle", cycle.join(", "))
                };
                Some(Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!("Circular dependency: {description}"),
                    declaration.location.line,
                    declaration.location.column,
                ))
            })
            .collect()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let index = project.analysis();
        index
            .dependency_cycles()
            .into_iter()
            .filter_map(|cycle| {
                let target = index.target(&cycle[0])?;
                let declaration = target.declarations.first()?;
                let description = if cycle.len() == 1 {
                    format!("'{}' depends on itself", cycle[0])
                } else {
                    format!("targets {} form a cycle", cycle.join(", "))
                };
                Some(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Circular dependency: {description}"),
                        declaration.location.line,
                        declaration.location.column,
                    )
                    .with_source(project.file(declaration.location.source).path.clone()),
                )
            })
            .collect()
    }
}
