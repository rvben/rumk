use std::collections::{BTreeSet, VecDeque};

use crate::analysis::{ReferenceContext, ReferenceKind};
use crate::diagnostic::{Diagnostic, Severity};
use crate::eval::BlockedReason;
use crate::parser::Makefile;
use crate::project::{IncludeResolution, Project};
use crate::project_analysis::ProjectTargetSymbol;
use crate::rules::{Rule, RuleCategory};

pub struct MixedTargetSeparators;

impl Rule for MixedTargetSeparators {
    fn id(&self) -> &'static str {
        "MK004"
    }

    fn name(&self) -> &'static str {
        "Target mixes single- and double-colon rules"
    }

    fn description(&self) -> &'static str {
        "A concrete target cannot use both ':' and '::' declarations in one evaluated Make project."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Syntax
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
            .filter_map(|target| {
                let mut declarations = target
                    .declarations
                    .iter()
                    .filter(|declaration| !index.is_conditional_line(declaration.location.line));
                let first = declarations.next()?;
                let second = declarations
                    .find(|declaration| declaration.double_colon != first.double_colon)?;
                Some(Diagnostic::new(
                    self.id(),
                    Severity::Error,
                    format!(
                        "Target '{}' has both single- and double-colon declarations",
                        target.name
                    ),
                    second.location.line,
                    second.location.column,
                ))
            })
            .collect()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        if project.analysis().has_structural_issues() {
            return Vec::new();
        }
        project
            .analysis()
            .targets
            .values()
            .filter(|target| !target.name.contains(['$', '%']))
            .filter_map(|target| {
                mixed_separator_declaration(project, target).map(|(_, second)| {
                    Diagnostic::new(
                        self.id(),
                        Severity::Error,
                        format!(
                            "Target '{}' has both single- and double-colon declarations",
                            target.name
                        ),
                        second.location.line,
                        second.location.column,
                    )
                    .with_source(project.file(second.location.source).path.clone())
                })
            })
            .collect()
    }
}

fn mixed_separator_declaration<'a>(
    project: &Project,
    target: &'a ProjectTargetSymbol,
) -> Option<(
    &'a crate::project_analysis::ProjectTargetDeclaration,
    &'a crate::project_analysis::ProjectTargetDeclaration,
)> {
    let mut declarations = target.declarations.iter().filter(|declaration| {
        project
            .analysis()
            .is_definitely_active(declaration.location)
    });
    let first = declarations.next()?;
    declarations
        .find(|declaration| declaration.double_colon != first.double_colon)
        .map(|second| (first, second))
}

pub struct MissingInclude;

impl Rule for MissingInclude {
    fn id(&self) -> &'static str {
        "MK206"
    }

    fn name(&self) -> &'static str {
        "Required include cannot be resolved"
    }

    fn description(&self) -> &'static str {
        "Required static includes must resolve from the Make working directory or configured include paths. Optional and generated includes are allowed."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        project
            .edges()
            .iter()
            .filter(|edge| !edge.optional)
            .filter_map(|edge| {
                let include = edge.expanded.as_deref().unwrap_or(&edge.expression);
                let detail = match &edge.resolution {
                    IncludeResolution::Missing { .. }
                        if project.analysis().target(include).is_none() =>
                    {
                        format!("Required include '{include}' was not found")
                    }
                    IncludeResolution::Unreadable { path, message } => format!(
                        "Required include '{}' could not be read at {}: {message}",
                        include,
                        path.display()
                    ),
                    IncludeResolution::Invalid { path, message } => format!(
                        "Required include '{}' is invalid at {}: {message}",
                        include,
                        path.display()
                    ),
                    IncludeResolution::LimitExceeded => format!(
                        "Required include '{}' exceeds the project file limit",
                        include
                    ),
                    _ => return None,
                };
                Some(
                    Diagnostic::new(self.id(), Severity::Warning, detail, edge.line, 1)
                        .with_source(project.file(edge.from).path.clone()),
                )
            })
            .collect()
    }
}

pub struct IncludeCycle;

impl Rule for IncludeCycle {
    fn id(&self) -> &'static str {
        "MK207"
    }

    fn name(&self) -> &'static str {
        "Circular Makefile include"
    }

    fn description(&self) -> &'static str {
        "Statically included Makefiles must not form cycles: GNU Make repeatedly reads them, duplicates declarations, and can exhaust file descriptors."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        project
            .cycles()
            .iter()
            .filter_map(|cycle| {
                let source = *cycle.sources.get(cycle.sources.len().checked_sub(2)?)?;
                let names = cycle
                    .sources
                    .iter()
                    .map(|source| project.file(*source).path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                Some(
                    Diagnostic::new(
                        self.id(),
                        Severity::Warning,
                        format!("Circular include: {names}"),
                        cycle.edge_line,
                        1,
                    )
                    .with_source(project.file(source).path.clone()),
                )
            })
            .collect()
    }
}

pub struct UnresolvedIncludeExpression;

impl Rule for UnresolvedIncludeExpression {
    fn id(&self) -> &'static str {
        "MK210"
    }

    fn name(&self) -> &'static str {
        "Include expression cannot be evaluated safely"
    }

    fn description(&self) -> &'static str {
        "Reports include expressions that remain unresolved during safe project evaluation, with the blocked operation and variable-definition trace."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        project
            .edges()
            .iter()
            .filter(|edge| edge.resolution == IncludeResolution::Dynamic)
            .filter(|edge| !edge.blocked.is_empty())
            .map(|edge| {
                let reasons = edge
                    .blocked
                    .iter()
                    .map(blocked_reason_description)
                    .collect::<Vec<_>>()
                    .join("; ");
                let mut seen = BTreeSet::new();
                let trace = edge
                    .trace
                    .iter()
                    .filter_map(|step| {
                        let description = match step.origin {
                        Some(origin) => format!(
                            "{} at {}:{}",
                            step.variable,
                            project.file(origin.source).path.display(),
                            origin.line
                        ),
                        None => format!("{} from predefined variables", step.variable),
                        };
                        seen.insert(description.clone()).then_some(description)
                    })
                    .collect::<Vec<_>>()
                    .join(" -> ");
                let trace = if trace.is_empty() {
                    String::new()
                } else {
                    format!(" (via {trace})")
                };
                Diagnostic::new(
                    self.id(),
                    Severity::Info,
                    format!(
                        "Include expression '{}' could not be resolved statically: {reasons}{trace}",
                        edge.expression
                    ),
                    edge.line,
                    1,
                )
                .with_source(project.file(edge.from).path.clone())
            })
            .collect()
    }
}

fn blocked_reason_description(reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::UndefinedVariable(variable) => {
            format!("variable '{variable}' has no known value")
        }
        BlockedReason::DynamicVariableName(variable) => {
            format!("variable name '{variable}' is dynamic")
        }
        BlockedReason::RecursiveReference(variable) => {
            format!("variable '{variable}' recursively references itself")
        }
        BlockedReason::UnsafeFunction(function) => {
            format!("function '{function}' is intentionally never executed")
        }
        BlockedReason::UnsupportedFunction(function) => {
            format!("function '{function}' is not supported by safe evaluation")
        }
        BlockedReason::MalformedExpansion => "the expansion is malformed".to_string(),
        BlockedReason::ExpansionLimit => "the expansion depth limit was reached".to_string(),
        BlockedReason::IndeterminateAssignment(variable) => {
            format!("assignment to '{variable}' is conditionally indeterminate")
        }
        BlockedReason::ShellAssignment(variable) => {
            format!("shell assignment to '{variable}' is intentionally never executed")
        }
    }
}

#[derive(Default)]
pub struct UndefinedVariableReference {
    predefined: BTreeSet<String>,
}

impl UndefinedVariableReference {
    pub fn new(predefined: impl IntoIterator<Item = String>) -> Self {
        Self {
            predefined: predefined.into_iter().collect(),
        }
    }
}

impl Rule for UndefinedVariableReference {
    fn id(&self) -> &'static str {
        "MK208"
    }

    fn name(&self) -> &'static str {
        "Undefined static variable reference"
    }

    fn description(&self) -> &'static str {
        "Static Make variable references in assignments and build-graph declarations should resolve to a project definition, a GNU Make built-in, or a configured predefined variable. Recipes and deferred macro bodies are excluded because they commonly accept external parameters."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let index = project.analysis();
        index
            .references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Variable)
            .filter(|reference| {
                !matches!(
                    reference.context,
                    ReferenceContext::Recipe | ReferenceContext::Definition
                )
            })
            .filter(|reference| index.is_definitely_active(reference.location))
            .filter(|reference| {
                index.variable(&reference.name).is_none_or(|variable| {
                    !variable
                        .definitions
                        .iter()
                        .any(|definition| index.is_definitely_active(definition.location))
                })
            })
            .filter(|reference| !self.predefined.contains(&reference.name))
            .filter(|reference| !GNU_BUILTIN_VARIABLES.contains(&reference.name.as_str()))
            .map(|reference| {
                Diagnostic::new(
                    self.id(),
                    Severity::Warning,
                    format!(
                        "Variable '{}' is referenced but not defined",
                        reference.name
                    ),
                    reference.location.line,
                    reference.location.column,
                )
                .with_source(project.file(reference.location.source).path.clone())
            })
            .collect()
    }
}

const GNU_BUILTIN_VARIABLES: &[&str] = &[
    ".DEFAULT_GOAL",
    ".FEATURES",
    ".INCLUDE_DIRS",
    ".RECIPEPREFIX",
    ".SHELLFLAGS",
    ".VARIABLES",
    "AR",
    "ARFLAGS",
    "AS",
    "CC",
    "CO",
    "COMPILE.c",
    "COMPILE.cpp",
    "COMPILE.p",
    "CPP",
    "CXX",
    "CURDIR",
    "FC",
    "GET",
    "LD",
    "LEX",
    "LINK.c",
    "LINK.cpp",
    "LINK.o",
    "MAKE",
    "MAKECMDGOALS",
    "MAKEFILE_LIST",
    "MAKEFLAGS",
    "MAKELEVEL",
    "MAKE_RESTARTS",
    "MAKE_TERMERR",
    "MAKE_TERMOUT",
    "MAKE_VERSION",
    "MFLAGS",
    "OUTPUT_OPTION",
    "PC",
    "PREPROCESS.S",
    "RM",
    "SHELL",
    "SUFFIXES",
    "VPATH",
    "WEAVE",
    "YACC",
];

#[derive(Default)]
pub struct UnreachableTarget {
    entry_targets: Vec<String>,
}

impl UnreachableTarget {
    pub fn new(entry_targets: Vec<String>) -> Self {
        Self { entry_targets }
    }
}

impl Rule for UnreachableTarget {
    fn id(&self) -> &'static str {
        "MK209"
    }

    fn name(&self) -> &'static str {
        "Target is unreachable from configured entries"
    }

    fn description(&self) -> &'static str {
        "Concrete targets should be reachable from explicitly configured entry targets. The rule stays silent without entry-targets because Make targets are also public command-line entry points."
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }

    fn project_aware(&self) -> bool {
        true
    }

    fn check(&self, _makefile: &Makefile, _content: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let index = project.analysis();
        if self.entry_targets.is_empty() {
            return Vec::new();
        }
        let entries = self.entry_targets.clone();
        let mut reachable = BTreeSet::new();
        let mut pending = VecDeque::from(entries);
        while let Some(target) = pending.pop_front() {
            if !reachable.insert(target.clone()) {
                continue;
            }
            if let Some(symbol) = index.target(&target) {
                pending.extend(
                    symbol
                        .dependencies
                        .iter()
                        .filter(|edge| index.is_definitely_active(edge.location))
                        .map(|edge| edge.prerequisite.clone()),
                );
            }
        }

        index
            .targets
            .values()
            .filter(|target| {
                !target.special
                    && !target.name.contains(['$', '%'])
                    && !reachable.contains(&target.name)
            })
            .filter_map(|target| {
                let declaration = target
                    .declarations
                    .iter()
                    .find(|declaration| index.is_definitely_active(declaration.location))?;
                Some(
                    Diagnostic::new(
                        self.id(),
                        Severity::Info,
                        format!(
                            "Target '{}' is unreachable from configured entry targets",
                            target.name
                        ),
                        declaration.location.line,
                        declaration.location.column,
                    )
                    .with_source(project.file(declaration.location.source).path.clone()),
                )
            })
            .collect()
    }
}
