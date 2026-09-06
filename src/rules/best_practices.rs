use crate::diagnostic::{Applicability, Diagnostic, Edit, Fix, Severity};
use crate::expansion::{function_name, reference_length};
use crate::logical::{
    find_top_level_char, find_top_level_rule_separator, split_top_level_words,
    strip_top_level_comment, LogicalKind,
};
use crate::parser::{AssignmentOperator, Makefile};
use crate::project::Project;
use crate::rules::{Rule, RuleCategory};
use crate::syntax::SyntaxKind;
use std::collections::{BTreeMap, BTreeSet};

use super::phony::{
    format_continued_declaration, preferred_line_ending, COMMON_PHONY_TARGETS,
    DEFAULT_PHONY_LINE_LENGTH,
};

#[derive(Debug, Clone)]
struct MissingPhonyTarget {
    name: String,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonyPlacement {
    Auto,
    Top,
    Adjacent,
}

pub struct MissingPhony {
    placement: PhonyPlacement,
}

impl MissingPhony {
    pub fn new(placement: PhonyPlacement) -> Self {
        Self { placement }
    }
}

impl Default for MissingPhony {
    fn default() -> Self {
        Self::new(PhonyPlacement::Auto)
    }
}

impl Rule for MissingPhony {
    fn id(&self) -> &'static str {
        "MK201"
    }

    fn name(&self) -> &'static str {
        "Conventional command targets should be .PHONY"
    }

    fn description(&self) -> &'static str {
        "Conventional command targets that don't represent actual files should be declared as \
         .PHONY to ensure they always run and to improve performance."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn fixable(&self) -> bool {
        true
    }

    /// Declaring a target `.PHONY` changes what Make does when a file of that
    /// name exists: the recipe runs where Make would have called the target up
    /// to date. Which targets never stand for a file is a judgement about the
    /// project, which Rumk makes from their names, so the fix waits to be
    /// asked for.
    fn fix_applicability(&self) -> Applicability {
        Applicability::Unsafe
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let missing = missing_phony_targets(makefile);
        let Some(first) = missing.first() else {
            return Vec::new();
        };
        let names = missing_names(&missing);
        vec![Diagnostic::new(
            self.id(),
            Severity::Warning,
            phony_message(&names),
            first.line,
            first.column,
        )
        .with_fix(phony_fix(makefile, &missing, content, self.placement))]
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let index = project.analysis();
        let mut missing_by_source = BTreeMap::new();
        for target in index
            .targets
            .values()
            .filter(|target| COMMON_PHONY_TARGETS.contains(&target.name.as_str()) && !target.phony)
        {
            if let Some(declaration) = target
                .declarations
                .iter()
                .find(|declaration| index.is_definitely_active(declaration.location))
            {
                missing_by_source
                    .entry(declaration.location.source)
                    .or_insert_with(Vec::new)
                    .push(MissingPhonyTarget {
                        name: target.name.clone(),
                        line: declaration.location.line,
                        column: declaration.location.column,
                    });
            }
        }

        for missing in missing_by_source.values_mut() {
            missing.sort_by(|left, right| {
                (left.line, left.column, &left.name).cmp(&(right.line, right.column, &right.name))
            });
        }

        missing_by_source
            .into_iter()
            .filter_map(|(source, missing)| {
                let first = missing.first()?;
                let names = missing_names(&missing);
                let mut diagnostic = Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    phony_message(&names),
                    first.line,
                    first.column,
                )
                .with_source(project.file(source).path.clone());
                if source == project.root() {
                    let file = project.file(project.root());
                    diagnostic = diagnostic.with_fix(phony_fix(
                        &file.makefile,
                        &missing,
                        &file.content,
                        self.placement,
                    ));
                }
                Some(diagnostic)
            })
            .collect()
    }
}

fn missing_phony_targets(makefile: &Makefile) -> Vec<MissingPhonyTarget> {
    let mut seen = BTreeSet::new();
    let mut missing = Vec::new();
    for rule in &makefile.rules {
        for target in &rule.targets {
            if COMMON_PHONY_TARGETS.contains(&target.as_str())
                && !makefile.phonies.contains(target)
                && seen.insert(target.clone())
            {
                missing.push(MissingPhonyTarget {
                    name: target.clone(),
                    line: rule.line,
                    column: rule.column,
                });
            }
        }
    }
    missing
}

fn missing_names(targets: &[MissingPhonyTarget]) -> Vec<String> {
    targets.iter().map(|target| target.name.clone()).collect()
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

fn phony_fix(
    makefile: &Makefile,
    targets: &[MissingPhonyTarget],
    content: &str,
    placement: PhonyPlacement,
) -> Fix {
    let names = missing_names(targets);
    let description = format!("Declare {} as .PHONY", names.join(" "));
    let declarations = phony_declarations(makefile);

    match placement {
        PhonyPlacement::Adjacent => adjacent_phony_fix(targets, content, description),
        PhonyPlacement::Top => top_phony_fix(makefile, &declarations, &names, content, description),
        PhonyPlacement::Auto => {
            auto_phony_fix(&declarations, targets, &names, content, description)
        }
    }
}

fn adjacent_phony_fix(targets: &[MissingPhonyTarget], content: &str, description: String) -> Fix {
    let by_line = targets.iter().fold(BTreeMap::new(), |mut grouped, target| {
        grouped
            .entry(target.line)
            .or_insert_with(Vec::new)
            .push(target.name.clone());
        grouped
    });
    by_line
        .into_iter()
        .fold(Fix::unsafe_fix(description), |fix, (line, names)| {
            fix.add_edit(Edit::new(
                line,
                1,
                line,
                1,
                format_phony_lines(&names, preferred_line_ending(content, line)),
            ))
        })
}

fn top_phony_fix(
    makefile: &Makefile,
    declarations: &[PhonyDeclaration],
    names: &[String],
    content: &str,
    description: String,
) -> Fix {
    if let Some(declaration) = declarations
        .iter()
        .min_by_key(|declaration| declaration.start_line)
    {
        return extend_phony_declaration(declaration, names, content, description);
    }

    let line = makefile
        .rules
        .iter()
        .map(|rule| rule.line)
        .min()
        .unwrap_or(1);
    let line_ending = preferred_line_ending(content, line);
    Fix::unsafe_fix(description).add_edit(Edit::new(
        line,
        1,
        line,
        1,
        format!(
            "{}{}",
            format_continued_declaration(names, "", line_ending, DEFAULT_PHONY_LINE_LENGTH),
            line_ending
        ),
    ))
}

fn auto_phony_fix(
    declarations: &[PhonyDeclaration],
    targets: &[MissingPhonyTarget],
    names: &[String],
    content: &str,
    description: String,
) -> Fix {
    if declarations.len() > 1
        && declarations
            .iter()
            .all(|declaration| declaration.names.len() <= 1)
    {
        return adjacent_phony_fix(targets, content, description);
    }

    if let Some(declaration) = declarations.iter().max_by_key(|declaration| {
        (
            declaration.names.len(),
            std::cmp::Reverse(declaration.start_line),
        )
    }) {
        return extend_phony_declaration(declaration, names, content, description);
    }

    let line = targets.first().map_or(1, |target| target.line);
    Fix::unsafe_fix(description).add_edit(Edit::new(
        line,
        1,
        line,
        1,
        format_phony_lines(names, preferred_line_ending(content, line)),
    ))
}

fn extend_phony_declaration(
    declaration: &PhonyDeclaration,
    names: &[String],
    content: &str,
    description: String,
) -> Fix {
    if let Some(column) = append_column(content, declaration, names) {
        return Fix::unsafe_fix(description).add_edit(Edit::new(
            declaration.start_line,
            column,
            declaration.start_line,
            column,
            format!(" {}", names.join(" ")),
        ));
    }
    if !declaration.has_comment {
        let mut combined = declaration.names.clone();
        for name in names {
            if !combined.contains(name) {
                combined.push(name.clone());
            }
        }
        if let Some(end_column) = line_end_column(content, declaration.end_line) {
            return Fix::unsafe_fix(description).add_edit(Edit::new(
                declaration.start_line,
                1,
                declaration.end_line,
                end_column,
                format_continued_declaration(
                    &combined,
                    "",
                    preferred_line_ending(content, declaration.start_line),
                    DEFAULT_PHONY_LINE_LENGTH,
                ),
            ));
        }
    }
    Fix::unsafe_fix(description).add_edit(Edit::new(
        declaration.start_line,
        1,
        declaration.start_line,
        1,
        format_phony_lines(
            names,
            preferred_line_ending(content, declaration.start_line),
        ),
    ))
}

#[derive(Debug)]
struct PhonyDeclaration {
    start_line: usize,
    end_line: usize,
    names: Vec<String>,
    has_comment: bool,
}

fn phony_declarations(makefile: &Makefile) -> Vec<PhonyDeclaration> {
    makefile
        .logical
        .statements()
        .iter()
        .filter(|statement| statement.kind == LogicalKind::Rule)
        .filter(|statement| {
            !makefile
                .analysis()
                .is_conditional_line(statement.start_line)
        })
        .filter_map(|statement| {
            let text = statement.text();
            let separator = find_top_level_rule_separator(text)?;
            (text[..separator.position].trim() == ".PHONY").then(|| {
                let body = &text[separator.position + separator.length..];
                let comment = find_top_level_char(body, '#');
                let names = split_top_level_words(comment.map_or(body, |index| &body[..index]));
                PhonyDeclaration {
                    start_line: statement.start_line,
                    end_line: statement.end_line,
                    names,
                    has_comment: comment.is_some(),
                }
            })
        })
        .collect()
}

fn append_column(content: &str, declaration: &PhonyDeclaration, names: &[String]) -> Option<usize> {
    if declaration.start_line != declaration.end_line {
        return None;
    }
    let source_line = content
        .lines()
        .nth(declaration.start_line.checked_sub(1)?)?;
    let comment = find_top_level_char(source_line, '#').unwrap_or(source_line.len());
    let insertion = source_line[..comment].trim_end().len();
    let added = names.iter().map(|name| name.chars().count()).sum::<usize>() + names.len();
    (source_line.chars().count() + added <= DEFAULT_PHONY_LINE_LENGTH)
        .then(|| source_line[..insertion].chars().count() + 1)
}

fn line_end_column(content: &str, line: usize) -> Option<usize> {
    content
        .lines()
        .nth(line.checked_sub(1)?)
        .map(|source_line| source_line.chars().count() + 1)
}

fn format_phony_lines(names: &[String], line_ending: &str) -> String {
    let mut output = String::new();
    let mut line = String::from(".PHONY:");
    for name in names {
        if line.chars().count() + 1 + name.chars().count() > DEFAULT_PHONY_LINE_LENGTH
            && line != ".PHONY:"
        {
            output.push_str(&line);
            output.push_str(line_ending);
            line = String::from(".PHONY:");
        }
        line.push(' ');
        line.push_str(name);
    }
    output.push_str(&line);
    output.push_str(line_ending);
    output
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

    /// `$(MAKE)` is not another spelling of `make`: it carries the command line
    /// options and the jobserver of the running Make into the sub-make, and it
    /// marks the line as recursive, so `make -n` runs it instead of printing
    /// it. That is the point of the rule, and it is a change in what the file
    /// does.
    fn fix_applicability(&self) -> Applicability {
        Applicability::Unsafe
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
                        Fix::unsafe_fix("Replace direct Make invocation with $(MAKE)"),
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

pub struct ShellStyleVariableReference;

impl Rule for ShellStyleVariableReference {
    fn id(&self) -> &'static str {
        "MK211"
    }

    fn name(&self) -> &'static str {
        "Variable reference written the way a shell writes one"
    }

    fn description(&self) -> &'static str {
        "GNU Make reads '$VAR' as the one-character variable '$(V)' followed by the literal text 'AR'. A Make variable whose name is longer than one character needs '$(VAR)' or '${VAR}', and a shell variable in a recipe needs '$$VAR'."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn fixable(&self) -> bool {
        true
    }

    /// Writing the parentheses in makes Make read a variable it was not
    /// reading before, which is the point of the rule and a change in what the
    /// file does.
    fn fix_applicability(&self) -> Applicability {
        Applicability::Unsafe
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let source = makefile.syntax.source();
        let defined: BTreeSet<&str> = makefile
            .assignments
            .iter()
            .map(|variable| variable.name.as_str())
            .collect();
        makefile
            .syntax
            .nodes()
            .iter()
            .filter(|node| {
                !matches!(
                    node.kind,
                    SyntaxKind::Blank | SyntaxKind::Comment | SyntaxKind::Endef
                )
            })
            .flat_map(|node| {
                let content = node.content(source);
                // Make expands a recipe line whole and hands it to the shell,
                // so a '#' there starts nothing; anywhere else it starts a
                // comment, and Make expands nothing after it.
                let expanded = if matches!(node.kind, SyntaxKind::Recipe | SyntaxKind::DefineBody) {
                    content
                } else {
                    strip_top_level_comment(content)
                };
                let line = node.content_span.start.line;
                let start_column = node.content_span.start.column;
                shell_style_references(expanded, &defined)
                    .into_iter()
                    .map(move |reference| {
                        let name = &expanded[reference.start + 1..reference.end];
                        let column = start_column + expanded[..reference.start].chars().count();
                        let diagnostic = Diagnostic::new(
                            self.id(),
                            Severity::Warning,
                            format!(
                                "'${name}' reads the variable '{}' and the literal text '{}'",
                                &name[..1],
                                &name[1..]
                            ),
                            line,
                            column,
                        );
                        // A recipe can mean either the Make variable or the
                        // shell one, written '$$VAR', and the two are not the
                        // same edit, so there is nothing to apply for it.
                        if node.kind == SyntaxKind::Recipe {
                            return diagnostic;
                        }
                        let end = start_column + expanded[..reference.end].chars().count();
                        diagnostic.with_fix(
                            Fix::unsafe_fix(format!("Read '{name}' as one variable"))
                                .add_edit(Edit::new(line, column, line, end, format!("$({name})"))),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// Byte ranges of the `$` references in `text` that name a variable the way a
/// shell names one: a `$` followed by more than one character of a name, which
/// GNU Make reads as the first character alone.
///
/// A first character the file gives a value of its own is left alone: `$(Q)`
/// written `$Qecho` reads exactly as it was meant to, and a file that defines
/// `Q` is a file that means it.
fn shell_style_references(text: &str, defined: &BTreeSet<&str>) -> Vec<std::ops::Range<usize>> {
    let mut references = Vec::new();
    let mut index = 0;
    while let Some(offset) = text[index..].find('$') {
        let dollar = index + offset;
        let Some(length) = reference_length(text, dollar) else {
            break;
        };
        index = dollar + length;
        let Some(first) = text[dollar + 1..].chars().next() else {
            break;
        };
        if length != 1 + first.len_utf8() || !is_name_start(first) {
            continue;
        }
        let end = dollar
            + 1
            + text[dollar + 1..]
                .find(|character: char| !is_name_character(character))
                .unwrap_or(text.len() - dollar - 1);
        if end - dollar < 3 || defined.contains(&text[dollar + 1..dollar + 1 + first.len_utf8()]) {
            continue;
        }
        references.push(dollar..end);
        index = end;
    }
    references
}

fn is_name_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

fn is_name_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

pub struct DirectoryChangeInRecipe;

impl Rule for DirectoryChangeInRecipe {
    fn id(&self) -> &'static str {
        "MK212"
    }

    fn name(&self) -> &'static str {
        "Directory change lost when the recipe line ends"
    }

    fn description(&self) -> &'static str {
        "Make runs every recipe line in its own shell, so a line whose last command is 'cd' leaves the lines after it where Make started. Join the commands on one line with '&&', pass the directory to the command, or declare .ONESHELL."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        if makefile.oneshell {
            return Vec::new();
        }
        makefile
            .rules
            .iter()
            .flat_map(|rule| {
                let last = rule.recipes.len().saturating_sub(1);
                rule.recipes[..last].iter()
            })
            .filter_map(|recipe| {
                let start = trailing_directory_change(&recipe.command)?;
                let (line, column) =
                    position_within(recipe.line, recipe.column, &recipe.command, start);
                Some(Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    "The directory this line changes to is gone when the next line runs",
                    line,
                    column,
                ))
            })
            .collect()
    }
}

/// The byte offset of the `cd` a recipe line ends with, and `None` when the
/// line runs something after it. The shell the line runs in exits at the end
/// of the line, taking the directory with it, so only a `cd` nothing follows
/// is one whose whole effect is lost.
fn trailing_directory_change(command: &str) -> Option<usize> {
    let mut command_position = true;
    let mut last = None;
    for token in shell_tokens(command) {
        match token {
            ShellToken::Separator => command_position = true,
            ShellToken::Word {
                text,
                quoted,
                start,
                ..
            } if command_position => {
                if is_environment_assignment(&text) {
                    continue;
                }
                last = (!quoted && text == "cd").then_some(start);
                command_position = matches!(text.as_str(), "if" | "then" | "else" | "do");
            }
            ShellToken::Word { .. } => {}
        }
    }
    last
}

/// Where `offset` falls in a recipe that starts at `line` and `column`,
/// counting the lines a continued recipe folded together.
fn position_within(line: usize, column: usize, command: &str, offset: usize) -> (usize, usize) {
    let before = &command[..offset];
    match before.rfind('\n') {
        None => (line, column + before.chars().count()),
        Some(newline) => (
            line + before.matches('\n').count(),
            1 + before[newline + 1..].chars().count(),
        ),
    }
}

pub struct ShellInRecursiveVariable;

impl Rule for ShellInRecursiveVariable {
    fn id(&self) -> &'static str {
        "MK213"
    }

    fn name(&self) -> &'static str {
        "$(shell ...) in a variable Make expands every time it is read"
    }

    fn description(&self) -> &'static str {
        "A recursive variable expands its value again every time it is read, so a '$(shell ...)' in one runs the command once per reading. ':=' and '!=' run it once, where the assignment is."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn check(&self, makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        let mut flavors: BTreeMap<&str, AssignmentOperator> = BTreeMap::new();
        let mut diagnostics = Vec::new();
        // A `define` is an assignment whose value is its body, so the bodies
        // are here too, under the operator their header carries.
        for variable in &makefile.assignments {
            let flavor = match variable.operator {
                // '+=' takes the flavor of the variable it appends to, and
                // creates a recursive one where there is nothing to append to.
                AssignmentOperator::Append => *flavors
                    .get(variable.name.as_str())
                    .unwrap_or(&AssignmentOperator::Recursive),
                operator => operator,
            };
            flavors.insert(variable.name.as_str(), flavor);
            if !expands_on_every_reading(flavor) {
                continue;
            }
            if calls_the_shell(&variable.value) && !reads_a_late_value(&variable.value) {
                diagnostics.push(shell_call_diagnostic(
                    self.id(),
                    &variable.name,
                    variable.line,
                    variable.column,
                ));
            }
        }
        diagnostics
    }
}

fn shell_call_diagnostic(rule: &'static str, name: &str, line: usize, column: usize) -> Diagnostic {
    Diagnostic::new(
        rule,
        Severity::Warning,
        format!("'{name}' runs its $(shell ...) again every time it is read"),
        line,
        column,
    )
}

/// Whether an assignment leaves Make expanding the value again at every
/// reading. `:::=` expands the value where it is written and escapes what
/// comes out, so the command behind it runs once.
fn expands_on_every_reading(operator: AssignmentOperator) -> bool {
    matches!(
        operator,
        AssignmentOperator::Recursive | AssignmentOperator::Conditional
    )
}

/// Whether `value` reads something Make knows only later: an argument a
/// `$(call ...)` passes in, or an automatic variable a rule sets while it runs.
/// A value like that has to be expanded at every reading to mean anything, so
/// the shell call inside it is what the value is for.
fn reads_a_late_value(value: &str) -> bool {
    let mut index = 0;
    while let Some(offset) = value[index..].find('$') {
        let dollar = index + offset;
        let Some(length) = reference_length(value, dollar) else {
            break;
        };
        let reference = &value[dollar + 1..dollar + length];
        let name = reference
            .strip_prefix(['(', '{'])
            .map_or(reference, |body| &body[..body.len() - 1]);
        // '$(@D)' and '$(<F)' name the directory and the file of an automatic
        // variable, so the first character is what decides.
        if name.len() <= 2 && name != "$" && name.starts_with(is_late_value) {
            return true;
        }
        index = match value[dollar + 1..].chars().next() {
            None => break,
            // Reading on from inside a reference finds the ones nested in it.
            Some('(' | '{') => dollar + 2,
            Some(character) => dollar + 1 + character.len_utf8(),
        };
    }
    false
}

fn is_late_value(character: char) -> bool {
    character.is_ascii_digit() || matches!(character, '@' | '<' | '^' | '?' | '*' | '+' | '|' | '%')
}

/// Whether `value` holds a `$(shell ...)` call, nested calls included. A `$$`
/// is the dollar sign itself, so `$$(shell ...)` is a command the shell
/// substitutes and not a call Make makes.
fn calls_the_shell(value: &str) -> bool {
    let mut index = 0;
    while let Some(offset) = value[index..].find('$') {
        let dollar = index + offset;
        let body = dollar + 2;
        match value[dollar + 1..].chars().next() {
            Some('(' | '{') if function_name(&value[body..]) == Some("shell") => return true,
            // Reading on from inside the reference finds the calls nested in it.
            Some('(' | '{') => index = body,
            Some('$') => index = dollar + 2,
            Some(character) => index = dollar + 1 + character.len_utf8(),
            None => break,
        }
    }
    false
}
