use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::analysis::{ReferenceContext, ReferenceKind};
use crate::builtins::is_defined_by_make;
use crate::diagnostic::{Diagnostic, Severity};
use crate::eval::{BlockedReason, EvaluationLocation};
use crate::parser::Makefile;
use crate::paths::display_path;
use crate::project::{IncludeEdge, IncludeResolution, Project, SourceId};
use crate::project_analysis::{ProjectSemanticIndex, ProjectTargetSymbol};
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
                let (severity, detail) = match &edge.resolution {
                    IncludeResolution::Missing { .. }
                        if project.analysis().target(include).is_none() =>
                    {
                        (
                            Severity::Warning,
                            format!("Required include '{include}' was not found"),
                        )
                    }
                    IncludeResolution::Unreadable { path, message } => (
                        Severity::Warning,
                        format!(
                            "Required include '{}' could not be read at {}: {message}",
                            include,
                            display_path(path)
                        ),
                    ),
                    IncludeResolution::LimitExceeded => (
                        Severity::Warning,
                        format!("Required include '{include}' exceeds the project file limit"),
                    ),
                    IncludeResolution::Dynamic => undefined_include(project, edge)?,
                    _ => return None,
                };
                Some(
                    Diagnostic::new(self.id(), severity, detail, edge.line, 1)
                        .with_source(project.file(edge.from).path.clone()),
                )
            })
            .collect()
    }
}

/// What to report about an include whose expression names variables that have
/// no value where Make reads it, and `None` when Make reads it the way the
/// file reads.
///
/// A variable the project gives a value further down makes the include an
/// error however Make ends up reading it: the file itself says which value was
/// meant, and Make uses none of it. A variable the project defines but has
/// taken back, or gives a value only where Make does not go, is reported only
/// where Make then finds no file to read, and as a warning. A variable the
/// project gives no value of its own is not reported at all: a caller supplies
/// it, and so does a definition that only reads the name back and writes it
/// again.
///
/// None of that holds for a project whose root is a file Make does not read on
/// its own, because whatever includes it has already run: a name defined below
/// the include may hold the caller's value there, and one the file never
/// defines certainly does. Such a root is passed over rather than reported.
///
/// Nor does it hold below an include Rumk did not read, such as one named
/// through `$(wildcard ...)`. Make read that file and holds whatever it
/// defines; Rumk does not, so a name that reads as having no value here may
/// have had one all along, and a definition below may be the `?=` that never
/// happens. Such an include is passed over too.
fn undefined_include(project: &Project, edge: &IncludeEdge) -> Option<(Severity, String)> {
    if !reads_on_its_own(project) || edge.follows_an_unread_include {
        return None;
    }
    let undefined = edge.undefined.as_ref()?;
    let index = project.analysis();
    let missing: Vec<&String> = undefined
        .missing
        .iter()
        // A missing include the project also knows how to build is generated
        // rather than absent, which is what MK205 is for.
        .filter(|path| index.target(path).is_none())
        .collect();
    let outcome = if undefined.paths.is_empty() {
        "reads no file at all".to_string()
    } else if missing.is_empty() {
        format!("reads '{}' instead", undefined.paths.join("' '"))
    } else {
        format!(
            "cannot find '{}'",
            missing
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join("' '")
        )
    };
    let defined_later = undefined.variables.iter().find_map(|found| {
        project
            .evaluation()
            .definition_after(&found.name, found.definitions_read)
            .map(|at| (&found.name, at))
    });
    match defined_later {
        Some((name, at)) => Some((
            Severity::Error,
            format!(
                "Required include '{}' expands '{name}' before {} defines it, so Make {outcome}",
                edge.expression,
                definition_site(project, edge.from, at)
            ),
        )),
        None if missing.is_empty() => None,
        // A name the project never defines is one a caller supplies: the
        // environment, the command line, or a parent make, none of which Rumk
        // reads. A fragment written to be included that way reads exactly like
        // this, so it is left to MK208, which is opt-in for that reason.
        None if !undefined
            .variables
            .iter()
            .all(|found| project.evaluation().gives_a_value(&found.name)) =>
        {
            None
        }
        None => Some((
            Severity::Warning,
            format!(
                "Required include '{}' expands '{}', which has no value there, so Make {outcome}",
                edge.expression,
                undefined
                    .variables
                    .iter()
                    .map(|found| found.name.as_str())
                    .collect::<Vec<_>>()
                    .join("' '")
            ),
        )),
    }
}

/// Whether the file the project is read from is one Make picks up by itself,
/// which is the only kind that carries no caller. Every other name is a
/// fragment as far as Rumk can tell, whether or not anything includes it.
fn reads_on_its_own(project: &Project) -> bool {
    project
        .file(project.root())
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "Makefile" | "makefile" | "GNUmakefile"))
}

/// Names where a definition is, as read from `from`: a line number for the
/// file being reported on, and a path for any other file.
fn definition_site(
    project: &Project,
    from: crate::project::SourceId,
    at: EvaluationLocation,
) -> String {
    if at.source == from {
        format!("line {}", at.line)
    } else {
        format!(
            "{}:{}",
            display_path(&project.file(at.source).path),
            at.line
        )
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
                    .map(|source| display_path(&project.file(*source).path))
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
                            display_path(&project.file(origin.source).path),
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
        "Static Make variable references in assignments and build-graph declarations should resolve to a project definition, a GNU Make built-in, or a configured predefined variable, and should resolve by the time Make reads them, because a ':=' assignment takes the value the reference has where it is written. Recipes and deferred macro bodies are excluded because they commonly accept external parameters."
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
        let mut diagnostics: Vec<Diagnostic> = index
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
            .filter(|reference| !is_defined_by_make(&reference.name))
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
            .collect();
        diagnostics.extend(read_too_early(self.id(), project));
        diagnostics
    }
}

/// What to report about a definition that read a name before the definition
/// giving it a value. Make expands `:=` where it is written, so the value
/// further down never reaches it, and the reference contributes nothing.
///
/// A definition Make may never read is passed over rather than reported: that a
/// name has no definition Make certainly reads is the other thing MK208 reports,
/// and reporting both would say the same mistake twice. Passing over such a
/// definition rather than dropping the reading is what makes the reading before
/// `ifdef CI` / `EXTRA := -g` / `endif` / `EXTRA := -O2` report the definition
/// on the last line, which Make does certainly read.
fn read_too_early(rule: &'static str, project: &Project) -> Vec<Diagnostic> {
    let index = project.analysis();
    let sites = ReferenceSites::of(index);
    project
        .evaluation()
        .read_too_early()
        .into_iter()
        .map(|reading| {
            let (line, column) = sites.of_name(&reading.name, reading.at);
            Diagnostic::new(
                rule,
                Severity::Warning,
                format!(
                    "Variable '{}' is read before {} defines it",
                    reading.name,
                    definition_site(project, reading.at.source, reading.defined)
                ),
                line,
                column,
            )
            .with_source(project.file(reading.at.source).path.clone())
        })
        .collect()
}

/// Where the references a project makes sit, in reading order, together with
/// how far down the file each definition runs. A definition can carry its
/// references well below the line it starts on, as a `define` body does and as
/// a line continued with a backslash does, so pointing at the reference means
/// looking through the whole definition rather than its first line.
struct ReferenceSites<'a> {
    /// Keyed by source, line and column, so the references inside one
    /// definition come out in the order the file reads them.
    references: BTreeMap<(SourceId, usize, usize), &'a str>,
    /// The last line of the definition starting at each source and line.
    extents: BTreeMap<(SourceId, usize), usize>,
}

impl<'a> ReferenceSites<'a> {
    fn of(index: &'a ProjectSemanticIndex) -> Self {
        Self {
            references: index
                .references
                .iter()
                .map(|reference| {
                    (
                        (
                            reference.location.source,
                            reference.location.line,
                            reference.location.column,
                        ),
                        reference.name.as_str(),
                    )
                })
                .collect(),
            extents: index
                .variables
                .values()
                .flat_map(|symbol| &symbol.definitions)
                .map(|definition| {
                    (
                        (definition.location.source, definition.location.line),
                        definition.end_line,
                    )
                })
                .collect(),
        }
    }

    /// Where the definition starting at `at` reads `name`, and the start of
    /// that definition where the reference is one the index does not carry, as
    /// a reference Rumk reached through another variable is.
    fn of_name(&self, name: &str, at: EvaluationLocation) -> (usize, usize) {
        let last = self
            .extents
            .get(&(at.source, at.line))
            .copied()
            .unwrap_or(at.line);
        self.references
            .range((at.source, at.line, 0)..=(at.source, last, usize::MAX))
            .find(|(_, found)| **found == name)
            .map_or((at.line, 1), |((_, line, column), _)| (*line, *column))
    }
}

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
