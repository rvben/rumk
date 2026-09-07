//! Safe, side-effect-free loading of statically knowable Make projects.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

use crate::eval::{
    BlockedReason, EvaluationLocation, Evaluator, Expansion, ReadTooEarly, TraceStep, Truth,
    UndefinedName,
};
use crate::logical::{ConditionalKind, LogicalKind};
use crate::parser::{self, Makefile, Variable, VariableScope};
use crate::project_analysis::ProjectSemanticIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub usize);

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub id: SourceId,
    pub path: PathBuf,
    pub content: String,
    pub makefile: Makefile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncludeResolution {
    Resolved(SourceId),
    Missing { searched: Vec<PathBuf> },
    Dynamic,
    Inactive,
    Unreadable { path: PathBuf, message: String },
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeEdge {
    pub from: SourceId,
    pub expression: String,
    /// The individual path produced by safe expansion, when known.
    pub expanded: Option<String>,
    /// Variable definitions that contributed to expansion, in evaluation order.
    pub trace: Vec<TraceStep>,
    /// Why expansion was deliberately left unresolved.
    pub blocked: BTreeSet<BlockedReason>,
    pub optional: bool,
    pub line: usize,
    pub resolution: IncludeResolution,
    /// What Make reads when the expression only stayed unexpanded because
    /// variables in it have no value there.
    pub undefined: Option<UndefinedExpansion>,
    /// Whether Make had already reached an include Rumk did not read by the
    /// time it reached this one. Whatever that file defines, Make holds here
    /// and Rumk does not, so a name with no value in the files Rumk read may
    /// have one in the file it skipped.
    pub follows_an_unread_include: bool,
}

/// What GNU Make reads for an include whose expression names variables that
/// have no value where it appears. Make expands such a reference to nothing,
/// so the expression still says which files are read, just not the ones the
/// author had in mind when the variable is given its value further down.
///
/// The include graph does not follow these paths: a variable with no value in
/// the files Rumk reads may still have one in the environment, and then Make
/// reads something else entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndefinedExpansion {
    /// The variables that had no value there, in the order they were found,
    /// each with the definitions of it Make had already read. A definition
    /// outside that set is one Make reaches only after this include.
    pub variables: Vec<UndefinedName>,
    /// The files Make reads, which is the expression with those variables
    /// contributing nothing. Empty when the expression expands to nothing,
    /// which Make reads as including no file at all.
    pub paths: Vec<String>,
    /// The paths among them that Make finds no file for.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultGoal {
    Unset,
    Known(String),
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ProjectEvaluation {
    evaluator: Evaluator,
    activity: BTreeMap<(SourceId, usize), Truth>,
    default_goal: DefaultGoal,
    active_phonies: BTreeSet<String>,
    rules: BTreeMap<(SourceId, usize), Vec<EvaluatedRule>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluatedRule {
    pub targets: Vec<String>,
    pub prerequisites: Vec<String>,
    pub order_only_prerequisites: Vec<String>,
}

impl ProjectEvaluation {
    pub fn expand(&self, expression: &str) -> crate::eval::Expansion {
        self.evaluator.expand(expression)
    }

    pub fn activity(&self, source: SourceId, line: usize) -> Truth {
        self.activity
            .get(&(source, line))
            .copied()
            .unwrap_or(Truth::Unknown)
    }

    pub fn default_goal(&self) -> &DefaultGoal {
        &self.default_goal
    }

    pub fn active_phonies(&self) -> &BTreeSet<String> {
        &self.active_phonies
    }

    pub fn rules(&self, source: SourceId, line: usize) -> &[EvaluatedRule] {
        self.rules.get(&(source, line)).map_or(&[], Vec::as_slice)
    }

    /// Where GNU Make defines `name` next, having read `definitions_read`
    /// definitions already, which is the definition an expansion at that point
    /// was read before.
    pub fn definition_after(
        &self,
        name: &str,
        definitions_read: usize,
    ) -> Option<EvaluationLocation> {
        self.evaluator.definition_after(name, definitions_read)
    }

    /// Whether the project gives `name` a value of its own somewhere, as
    /// against a caller supplying it through the environment, the command line
    /// or a parent make.
    pub fn gives_a_value(&self, name: &str) -> bool {
        self.evaluator.gives_a_value(name)
    }

    /// Every name a definition read before the definition that gives it a
    /// value, in the order Make read them.
    pub fn read_too_early(&self) -> Vec<ReadTooEarly> {
        self.evaluator.read_too_early()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeCycle {
    /// The first and last source are the same, making the cycle explicit.
    pub sources: Vec<SourceId>,
    pub edge_line: usize,
}

#[derive(Debug, Clone)]
pub struct Project {
    root: SourceId,
    files: Vec<ProjectFile>,
    edges: Vec<IncludeEdge>,
    cycles: Vec<IncludeCycle>,
    evaluation: ProjectEvaluation,
    analysis: OnceLock<ProjectSemanticIndex>,
    working_directory: PathBuf,
}

impl Project {
    pub fn load(path: &Path, options: &ProjectOptions) -> Result<Self> {
        let content = read_source(path)
            .with_context(|| format!("Failed to read Makefile: {}", path.display()))?;
        Self::load_with_root_content(path, content, options)
    }

    pub fn load_with_root_content(
        path: &Path,
        content: String,
        options: &ProjectOptions,
    ) -> Result<Self> {
        let makefile = parser::parse(&content);
        Self::load_with_root_makefile(path, content, makefile, options)
    }

    pub(crate) fn load_with_root_makefile(
        path: &Path,
        content: String,
        makefile: Makefile,
        options: &ProjectOptions,
    ) -> Result<Self> {
        let path = canonical_or_normalized(path)?;
        let working_directory = options
            .working_directory
            .clone()
            .or_else(|| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let mut loader = Loader::new(options, working_directory.clone());
        let root = loader.insert_file(path, content, makefile);
        loader.paths.insert(loader.files[root.0].path.clone(), root);
        loader.visit(root);
        Ok(Self {
            root,
            files: loader.files,
            edges: loader.edges,
            cycles: loader.cycles,
            evaluation: ProjectEvaluation {
                evaluator: loader.evaluator,
                activity: loader.activity,
                default_goal: loader.default_goal,
                active_phonies: loader.active_phonies,
                rules: loader.rules,
            },
            analysis: OnceLock::new(),
            working_directory,
        })
    }

    pub fn root(&self) -> SourceId {
        self.root
    }

    pub fn files(&self) -> &[ProjectFile] {
        &self.files
    }

    pub fn file(&self, id: SourceId) -> &ProjectFile {
        &self.files[id.0]
    }

    pub fn edges(&self) -> &[IncludeEdge] {
        &self.edges
    }

    /// The directory Make would read a relative path against.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn cycles(&self) -> &[IncludeCycle] {
        &self.cycles
    }

    pub fn evaluation(&self) -> &ProjectEvaluation {
        &self.evaluation
    }

    /// Returns the cross-file semantic index, building it once on first use.
    pub fn analysis(&self) -> &ProjectSemanticIndex {
        self.analysis
            .get_or_init(|| ProjectSemanticIndex::build(self))
    }
}

#[derive(Debug, Clone)]
pub struct ProjectOptions {
    /// Directory GNU Make would run from. Relative includes and include search
    /// paths are interpreted from here. Defaults to the root Makefile's parent.
    pub working_directory: Option<PathBuf>,
    pub include_paths: Vec<PathBuf>,
    /// Values supplied as if on GNU Make's command line. Ordinary assignments
    /// cannot replace them unless they use `override`.
    pub predefined_variables: BTreeMap<String, String>,
    pub max_files: usize,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            working_directory: None,
            include_paths: Vec::new(),
            predefined_variables: BTreeMap::new(),
            max_files: 1024,
        }
    }
}

struct Loader<'a> {
    options: &'a ProjectOptions,
    working_directory: PathBuf,
    files: Vec<ProjectFile>,
    paths: BTreeMap<PathBuf, SourceId>,
    edges: Vec<IncludeEdge>,
    cycles: Vec<IncludeCycle>,
    visiting: Vec<SourceId>,
    evaluator: Evaluator,
    activity: BTreeMap<(SourceId, usize), Truth>,
    default_goal: DefaultGoal,
    active_phonies: BTreeSet<String>,
    rules: BTreeMap<(SourceId, usize), Vec<EvaluatedRule>>,
    /// Whether Make has reached an include Rumk did not read. Set in the order
    /// Make reads, so an include below one of those cannot be judged on what
    /// the files Rumk read say a name is worth.
    unread_include: bool,
}

#[derive(Debug, Clone, Copy)]
struct ConditionalFrame {
    parent: Truth,
    matched: Truth,
    current: Truth,
}

impl<'a> Loader<'a> {
    fn new(options: &'a ProjectOptions, working_directory: PathBuf) -> Self {
        Self {
            options,
            working_directory,
            unread_include: false,
            files: Vec::new(),
            paths: BTreeMap::new(),
            edges: Vec::new(),
            cycles: Vec::new(),
            visiting: Vec::new(),
            evaluator: Evaluator::new(&options.predefined_variables),
            activity: BTreeMap::new(),
            default_goal: DefaultGoal::Unset,
            active_phonies: BTreeSet::new(),
            rules: BTreeMap::new(),
        }
    }

    fn insert_file(&mut self, path: PathBuf, content: String, makefile: Makefile) -> SourceId {
        let id = SourceId(self.files.len());
        self.files.push(ProjectFile {
            id,
            path,
            content,
            makefile,
        });
        id
    }

    fn visit(&mut self, source: SourceId) {
        if self.visiting.contains(&source) {
            return;
        }
        self.visiting.push(source);
        let makefile = self.files[source.0].makefile.clone();
        let mut conditionals: Vec<ConditionalFrame> = Vec::new();

        // A target- or pattern-specific assignment gives the name a value the
        // evaluator never holds, because Make holds it only while that rule
        // runs. It is still the project saying what the name is worth.
        for variable in &makefile.assignments {
            if variable.scope != VariableScope::Global {
                self.evaluator.gives_a_scoped_value(&variable.name);
            }
        }

        for statement in makefile.logical.statements() {
            if let LogicalKind::Conditional(kind) = statement.kind {
                let conditional = makefile
                    .conditionals
                    .iter()
                    .find(|conditional| conditional.line == statement.start_line)
                    .expect("parsed conditional has semantic record");
                self.apply_conditional(kind, &conditional.expression, &mut conditionals);
                self.record_activity(
                    source,
                    statement.start_line,
                    current_activity(&conditionals),
                );
                continue;
            }

            let activity = current_activity(&conditionals);
            self.record_activity(source, statement.start_line, activity);
            match statement.kind {
                LogicalKind::Assignment => {
                    if let Some(variable) = makefile.assignments.iter().find(|variable| {
                        variable.line == statement.start_line
                            && variable.scope == VariableScope::Global
                    }) {
                        self.apply_assignment(source, variable, activity);
                    }
                }
                LogicalKind::Define => {
                    if let Some(definition) = makefile
                        .definitions
                        .iter()
                        .find(|definition| definition.line == statement.start_line)
                    {
                        let variable = Variable {
                            name: definition.name.clone(),
                            value: definition.value.clone(),
                            operator: definition.operator,
                            modifiers: definition.modifiers,
                            scope: VariableScope::Global,
                            reach: definition.reach,
                            line: definition.line,
                            end_line: definition.end_line,
                            column: 1,
                        };
                        self.apply_assignment(source, &variable, activity);
                    }
                }
                LogicalKind::Include(_) => {
                    if let Some(include) = makefile
                        .includes
                        .iter()
                        .find(|include| include.line == statement.start_line)
                    {
                        for expression in &include.paths {
                            self.process_include(
                                source,
                                expression,
                                include.optional,
                                include.line,
                                activity,
                            );
                        }
                    }
                }
                LogicalKind::Rule => {
                    if let Some(rule) = makefile
                        .rules
                        .iter()
                        .find(|rule| rule.line == statement.start_line)
                    {
                        self.consider_rule(source, rule, activity);
                    } else if let Some(prerequisites) = phony_prerequisites(statement.text()) {
                        self.consider_phonies(prerequisites, activity);
                    }
                }
                LogicalKind::Directive => {
                    if let Some(name) = undefine_name(statement.text()) {
                        self.evaluator.undefine(name, activity);
                    }
                }
                _ => {}
            }
        }

        self.visiting.pop();
    }

    fn record_activity(&mut self, source: SourceId, line: usize, activity: Truth) {
        self.activity
            .entry((source, line))
            .and_modify(|existing| *existing = existing.or(activity))
            .or_insert(activity);
    }

    fn apply_assignment(&mut self, source: SourceId, variable: &Variable, activity: Truth) {
        self.evaluator.assign(
            variable,
            EvaluationLocation {
                source,
                line: variable.line,
            },
            activity,
        );
        if variable.name == ".DEFAULT_GOAL" && activity != Truth::False {
            self.default_goal = match self.evaluator.expand("$(.DEFAULT_GOAL)").value {
                Some(value) if value.trim().is_empty() => DefaultGoal::Unset,
                Some(value) if value.split_whitespace().count() == 1 => {
                    DefaultGoal::Known(value.trim().to_string())
                }
                Some(_) | None => DefaultGoal::Unknown,
            };
        }
    }

    fn apply_conditional(
        &self,
        kind: ConditionalKind,
        expression: &str,
        stack: &mut Vec<ConditionalFrame>,
    ) {
        match kind {
            ConditionalKind::Ifdef
            | ConditionalKind::Ifndef
            | ConditionalKind::Ifeq
            | ConditionalKind::Ifneq => {
                let parent = current_activity(stack);
                let matched = self.evaluator.condition(kind, expression);
                stack.push(ConditionalFrame {
                    parent,
                    matched,
                    current: parent.and(matched),
                });
            }
            ConditionalKind::Else => {
                let Some(frame) = stack.last_mut() else {
                    return;
                };
                if let Some((nested_kind, nested_expression)) = parse_else_condition(expression) {
                    let condition = self.evaluator.condition(nested_kind, nested_expression);
                    frame.current = frame.parent.and(frame.matched.negate()).and(condition);
                    frame.matched = frame.matched.or(condition);
                } else {
                    frame.current = frame.parent.and(frame.matched.negate());
                    frame.matched = Truth::True;
                }
            }
            ConditionalKind::Endif => {
                stack.pop();
            }
        }
    }

    fn process_include(
        &mut self,
        source: SourceId,
        expression: &str,
        optional: bool,
        line: usize,
        activity: Truth,
    ) {
        if activity == Truth::False {
            self.edges.push(IncludeEdge {
                from: source,
                expression: expression.to_string(),
                expanded: None,
                trace: Vec::new(),
                blocked: BTreeSet::new(),
                optional,
                line,
                resolution: IncludeResolution::Inactive,
                undefined: None,
                follows_an_unread_include: self.unread_include,
            });
            return;
        }
        let expansion = self.evaluator.expand(expression);
        let Some(value) = expansion
            .value
            .as_deref()
            .filter(|_| activity == Truth::True)
        else {
            // An include Make may not even read says nothing about the files
            // it would have read.
            let undefined = (activity == Truth::True)
                .then(|| self.undefined_expansion(expression))
                .flatten();
            let gap = self.reads_a_file_rumk_does_not(expression, &expansion);
            self.edges.push(IncludeEdge {
                from: source,
                expression: expression.to_string(),
                expanded: None,
                trace: expansion.trace,
                blocked: expansion.blocked,
                optional,
                line,
                resolution: IncludeResolution::Dynamic,
                undefined,
                follows_an_unread_include: self.unread_include,
            });
            self.unread_include |= gap;
            return;
        };
        for expanded in include_paths(value) {
            let (resolution, discovered) = self.resolve_include(&expanded, line);
            self.edges.push(IncludeEdge {
                from: source,
                expression: expression.to_string(),
                expanded: Some(expanded),
                trace: expansion.trace.clone(),
                blocked: expansion.blocked.clone(),
                optional,
                line,
                resolution: resolution.clone(),
                undefined: None,
                follows_an_unread_include: self.unread_include,
            });
            // A file Make reads and Rumk cannot is the same gap as one Rumk
            // never named: whatever it defines, Make holds from here on.
            self.unread_include |= matches!(
                resolution,
                IncludeResolution::Unreadable { .. } | IncludeResolution::LimitExceeded
            );
            if let Some(discovered) = discovered {
                self.visit(discovered);
            }
        }
    }

    /// Whether Make can hold something from an include Rumk did not follow.
    /// Rumk reads none of the files such an include names, so whatever Make
    /// took from it, Make holds below and Rumk does not. Only where Rumk can
    /// name every file Make would read there and finds none of them did Make
    /// certainly take nothing: an include under a condition Make decides while
    /// it reads counts either way, since Make may take the branch.
    fn reads_a_file_rumk_does_not(&self, expression: &str, expansion: &Expansion) -> bool {
        let Some(value) = expansion.value.as_deref() else {
            // An expression left unexpanded by anything but a name without a
            // value names files Rumk cannot, such as a `$(wildcard ...)` or a
            // `$(shell ...)` pattern.
            let Some(undefined) = self.undefined_expansion(expression) else {
                return true;
            };
            return undefined
                .paths
                .iter()
                .any(|path| !undefined.missing.contains(path));
        };
        include_paths(value).iter().any(|path| {
            // A pattern is matched against the working directory, so which
            // files it names is not a question Rumk answers about one path.
            is_dynamic_path(path)
                || include_candidates(&self.working_directory, path, self.options)
                    .iter()
                    .any(|candidate| candidate.is_file())
        })
    }

    /// Which files GNU Make reads for an include expression Rumk could only
    /// not expand because variables in it have no value yet. `None` when
    /// anything else left the expression unexpanded, since Make then reads
    /// files this cannot name.
    fn undefined_expansion(&self, expression: &str) -> Option<UndefinedExpansion> {
        let (value, variables) = self.evaluator.expand_as_undefined(expression)?;
        if variables.is_empty() {
            // Nothing without a value reached the result, so a name without one
            // is not why Rumk could not expand this. A function that asks what
            // a variable is rather than what it holds, such as `flavor`,
            // answers the same either way.
            return None;
        }
        let paths = include_paths(&value);
        let missing = paths
            .iter()
            // Make matches a pattern against the working directory, so which
            // files a pattern names is not a question about one path.
            .filter(|path| !is_dynamic_path(path))
            .filter(|path| {
                !include_candidates(&self.working_directory, path, self.options)
                    .iter()
                    .any(|candidate| candidate.is_file())
            })
            .cloned()
            .collect();
        Some(UndefinedExpansion {
            variables,
            paths,
            missing,
        })
    }

    fn consider_rule(&mut self, source: SourceId, rule: &crate::parser::Rule, activity: Truth) {
        if activity == Truth::True {
            let targets = self.expand_words(&rule.targets);
            let prerequisites = self.expand_words(&rule.prerequisites);
            let order_only = self.expand_words(&rule.order_only_prerequisites);
            if let (Some(targets), Some(prerequisites), Some(order_only_prerequisites)) =
                (targets, prerequisites, order_only)
            {
                self.rules
                    .entry((source, rule.line))
                    .or_default()
                    .push(EvaluatedRule {
                        targets,
                        prerequisites,
                        order_only_prerequisites,
                    });
            }
        }
        if self.default_goal != DefaultGoal::Unset || activity == Truth::False {
            return;
        }
        if activity == Truth::Unknown {
            self.default_goal = DefaultGoal::Unknown;
            return;
        }
        for target in &rule.targets {
            let expansion = self.evaluator.expand(target);
            let Some(value) = expansion.value else {
                self.default_goal = DefaultGoal::Unknown;
                return;
            };
            if let Some(target) = value
                .split_whitespace()
                .find(|target| !target.starts_with('.') && !target.contains('%'))
            {
                self.default_goal = DefaultGoal::Known(target.to_string());
                return;
            }
        }
    }

    fn expand_words(&self, expressions: &[String]) -> Option<Vec<String>> {
        let mut words = Vec::new();
        for expression in expressions {
            let value = self.evaluator.expand(expression).value?;
            words.extend(value.split_whitespace().map(ToOwned::to_owned));
        }
        Some(words)
    }

    fn consider_phonies(&mut self, prerequisites: &str, activity: Truth) {
        if activity != Truth::True {
            return;
        }
        if let Some(value) = self.evaluator.expand(prerequisites).value {
            self.active_phonies
                .extend(value.split_whitespace().map(ToOwned::to_owned));
        }
    }

    fn resolve_include(
        &mut self,
        expression: &str,
        line: usize,
    ) -> (IncludeResolution, Option<SourceId>) {
        if is_dynamic_path(expression) {
            return (IncludeResolution::Dynamic, None);
        }

        let candidates = include_candidates(&self.working_directory, expression, self.options);
        let Some(path) = candidates.iter().find(|candidate| candidate.is_file()) else {
            return (
                IncludeResolution::Missing {
                    searched: candidates,
                },
                None,
            );
        };
        let path = match canonical_or_normalized(path) {
            Ok(path) => path,
            Err(error) => {
                return (
                    IncludeResolution::Unreadable {
                        path: path.clone(),
                        message: error.to_string(),
                    },
                    None,
                );
            }
        };

        if let Some(id) = self.paths.get(&path).copied() {
            let cycle = self.visiting.contains(&id);
            self.record_cycle(id, line);
            return (IncludeResolution::Resolved(id), (!cycle).then_some(id));
        }
        if self.files.len() >= self.options.max_files {
            return (IncludeResolution::LimitExceeded, None);
        }

        let content = match read_source(&path) {
            Ok(content) => content,
            Err(error) => {
                return (
                    IncludeResolution::Unreadable {
                        path,
                        message: error.to_string(),
                    },
                    None,
                );
            }
        };
        let makefile = parser::parse(&content);
        let id = self.insert_file(path.clone(), content, makefile);
        self.paths.insert(path, id);
        (IncludeResolution::Resolved(id), Some(id))
    }

    fn record_cycle(&mut self, to: SourceId, edge_line: usize) {
        let Some(start) = self.visiting.iter().position(|source| *source == to) else {
            return;
        };
        let mut sources = self.visiting[start..].to_vec();
        sources.push(to);
        let key: Vec<_> = sources.iter().map(|source| source.0).collect();
        if self.cycles.iter().any(|cycle| {
            cycle
                .sources
                .iter()
                .map(|source| source.0)
                .collect::<Vec<_>>()
                == key
        }) {
            return;
        }
        self.cycles.push(IncludeCycle { sources, edge_line });
    }
}

/// Reads a Makefile the way GNU Make does, as bytes: a file that is not valid
/// UTF-8 is decoded lossily instead of counting as unreadable, so an include
/// GNU Make follows is analyzed rather than reported as missing, and a leading
/// byte order mark is read past the way Make 4.3 and later read past it.
fn read_source(path: &Path) -> std::io::Result<String> {
    let read = std::fs::read(path)?;
    let (bytes, _) = crate::source::split_byte_order_mark(&read);
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn include_candidates(
    working_directory: &Path,
    expression: &str,
    options: &ProjectOptions,
) -> Vec<PathBuf> {
    let path = Path::new(expression);
    if path.is_absolute() {
        return vec![normalize_path(path)];
    }

    std::iter::once(working_directory.to_path_buf())
        .chain(options.include_paths.iter().map(|directory| {
            if directory.is_absolute() {
                directory.clone()
            } else {
                working_directory.join(directory)
            }
        }))
        .map(|directory| normalize_path(&directory.join(path)))
        .collect()
}

/// The files GNU Make reads for an expanded include list. Make splits the list
/// at whitespace and removes the escapes only here, after expansion: a run of
/// backslashes before a blank halves, and an odd run leaves the blank inside
/// the path instead of ending it.
fn include_paths(value: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut backslashes = 0usize;

    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character.is_whitespace() {
            current.extend(std::iter::repeat_n('\\', backslashes / 2));
            if backslashes % 2 == 1 {
                current.push(character);
            } else if !current.is_empty() {
                paths.push(std::mem::take(&mut current));
            }
            backslashes = 0;
            continue;
        }
        current.extend(std::iter::repeat_n('\\', backslashes));
        backslashes = 0;
        current.push(character);
    }

    current.extend(std::iter::repeat_n('\\', backslashes));
    if !current.is_empty() {
        paths.push(current);
    }
    paths
}

fn is_dynamic_path(expression: &str) -> bool {
    expression.contains(['$', '%', '*', '?', '['])
}

fn current_activity(stack: &[ConditionalFrame]) -> Truth {
    stack.last().map_or(Truth::True, |frame| frame.current)
}

fn parse_else_condition(expression: &str) -> Option<(ConditionalKind, &str)> {
    let expression = expression.trim();
    for (keyword, kind) in [
        ("ifdef", ConditionalKind::Ifdef),
        ("ifndef", ConditionalKind::Ifndef),
        ("ifeq", ConditionalKind::Ifeq),
        ("ifneq", ConditionalKind::Ifneq),
    ] {
        if let Some(rest) = expression.strip_prefix(keyword) {
            return Some((kind, rest.trim()));
        }
    }
    None
}

fn undefine_name(text: &str) -> Option<&str> {
    text.trim_start()
        .strip_prefix("undefine")
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.contains(char::is_whitespace))
}

fn phony_prerequisites(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix(".PHONY")?.trim_start();
    let rest = rest.strip_prefix(':')?;
    Some(rest.split('#').next().unwrap_or(rest).trim())
}

fn canonical_or_normalized(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return dunce::canonicalize(path)
            .with_context(|| format!("Failed to resolve path: {}", path.display()));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to determine current directory")?
            .join(path)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
