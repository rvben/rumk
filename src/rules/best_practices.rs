use crate::diagnostic::{Diagnostic, Edit, Fix, Severity};
use crate::parser::Makefile;
use crate::project::Project;
use crate::rules::{Rule, RuleCategory};
use std::collections::BTreeMap;

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

    fn fixable(&self) -> bool {
        true
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let common_phony_targets = ["all", "clean", "test", "check", "install", "build", "help"];

        for rule in &makefile.rules {
            let missing = rule
                .targets
                .iter()
                .filter(|target| {
                    common_phony_targets.contains(&target.as_str())
                        && !makefile.phonies.contains(target)
                })
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                diagnostics.push(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        phony_message(&missing),
                        rule.line,
                        rule.column,
                    )
                    .with_fix(phony_fix(&missing, rule.line, content)),
                );
            }
        }

        diagnostics
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let common = ["all", "clean", "test", "check", "install", "build", "help"];
        let index = project.analysis();
        let mut missing_by_declaration = BTreeMap::new();
        for target in index
            .targets
            .values()
            .filter(|target| common.contains(&target.name.as_str()) && !target.phony)
        {
            if let Some(declaration) = target
                .declarations
                .iter()
                .find(|declaration| index.is_definitely_active(declaration.location))
            {
                missing_by_declaration
                    .entry((
                        declaration.location.source,
                        declaration.location.line,
                        declaration.location.column,
                    ))
                    .or_insert_with(Vec::new)
                    .push(target.name.clone());
            }
        }

        missing_by_declaration
            .into_iter()
            .map(|((source, line, column), targets)| {
                let mut diagnostic = Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    phony_message(&targets),
                    line,
                    column,
                )
                .with_source(project.file(source).path.clone());
                if source == project.root() {
                    diagnostic = diagnostic.with_fix(phony_fix(
                        &targets,
                        line,
                        &project.file(project.root()).content,
                    ));
                }
                diagnostic
            })
            .collect()
    }
}

fn phony_message(targets: &[String]) -> String {
    if targets.len() == 1 {
        format!("Target '{}' should be declared .PHONY", targets[0])
    } else {
        format!(
            "Targets {} should be declared .PHONY",
            targets
                .iter()
                .map(|target| format!("'{target}'"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn phony_fix(targets: &[String], line: usize, content: &str) -> Fix {
    let line_ending = preferred_line_ending(content, line);
    let names = targets.join(" ");
    Fix::new(format!("Declare {names} as .PHONY")).add_edit(Edit::new(
        line,
        1,
        line,
        1,
        format!(".PHONY: {names}{line_ending}"),
    ))
}

fn preferred_line_ending(content: &str, line: usize) -> &'static str {
    match content.split_inclusive('\n').nth(line.saturating_sub(1)) {
        Some(source_line) if source_line.ends_with("\r\n") => "\r\n",
        Some(source_line) if source_line.ends_with('\n') => "\n",
        _ if content.contains("\r\n") => "\r\n",
        _ => "\n",
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

    fn fixable(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        makefile
            .rules
            .iter()
            .flat_map(|rule| &rule.recipes)
            .filter_map(|recipe| {
                let invocations = bare_make_invocations(&recipe.command);
                let first = invocations.first()?;
                let mut diagnostic = Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    "Use $(MAKE) instead of invoking make directly",
                    recipe.line,
                    recipe.column + recipe.command[..first.start].chars().count(),
                );
                if recipe.line == recipe.end_line {
                    let fix = invocations.into_iter().fold(
                        Fix::new("Replace direct Make invocation with $(MAKE)"),
                        |fix, invocation| {
                            let start =
                                recipe.column + recipe.command[..invocation.start].chars().count();
                            let end =
                                recipe.column + recipe.command[..invocation.end].chars().count();
                            fix.add_edit(Edit::new(recipe.line, start, recipe.line, end, "$(MAKE)"))
                        },
                    );
                    diagnostic = diagnostic.with_fix(fix);
                }
                Some(diagnostic)
            })
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ShellToken {
    Word {
        text: String,
        quoted: bool,
        start: usize,
        end: usize,
    },
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Invocation {
    start: usize,
    end: usize,
}

fn bare_make_invocations(command: &str) -> Vec<Invocation> {
    let mut command_position = true;
    let mut invocations = Vec::new();
    for token in shell_tokens(command) {
        match token {
            ShellToken::Separator => command_position = true,
            ShellToken::Word {
                text,
                quoted,
                start,
                end,
            } if command_position => {
                if is_environment_assignment(&text)
                    || matches!(text.as_str(), "command" | "exec" | "env" | "sudo" | "time")
                {
                    continue;
                }
                if !quoted
                    && (is_make_executable(&text) || is_make_executable(&command[start..end]))
                {
                    invocations.push(Invocation { start, end });
                }
                command_position = matches!(text.as_str(), "if" | "then" | "else" | "do");
            }
            ShellToken::Word { .. } => {}
        }
    }
    invocations
}

fn shell_tokens(command: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = None;
    let mut quote = None;
    let mut quoted = false;
    let mut escaped = false;

    let flush = |tokens: &mut Vec<ShellToken>,
                 current: &mut String,
                 current_start: &mut Option<usize>,
                 quoted: &mut bool,
                 end: usize| {
        if !current.is_empty() {
            tokens.push(ShellToken::Word {
                text: std::mem::take(current),
                quoted: *quoted,
                start: current_start.take().expect("non-empty token has a start"),
                end,
            });
            *quoted = false;
        }
    };

    for (offset, character) in command.char_indices() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            current_start.get_or_insert(offset);
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
            current_start.get_or_insert(offset);
            quote = Some(character);
            quoted = true;
        } else if character.is_whitespace() {
            flush(
                &mut tokens,
                &mut current,
                &mut current_start,
                &mut quoted,
                offset,
            );
            if character == '\n' {
                tokens.push(ShellToken::Separator);
            }
        } else if matches!(character, ';' | '|' | '&') {
            flush(
                &mut tokens,
                &mut current,
                &mut current_start,
                &mut quoted,
                offset,
            );
            if !matches!(tokens.last(), Some(ShellToken::Separator)) {
                tokens.push(ShellToken::Separator);
            }
        } else {
            current_start.get_or_insert(offset);
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    flush(
        &mut tokens,
        &mut current,
        &mut current_start,
        &mut quoted,
        command.len(),
    );
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
        .is_some_and(|name| matches!(name, "make" | "gmake" | "make.exe" | "gmake.exe"))
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
                        && index.is_definitely_active(declaration.location)
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
