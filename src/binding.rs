//! When GNU Make expands a variable, and which assignments it sees then.
//!
//! Make binds a variable to the last assignment it has read: `+=` extends
//! the current binding, `?=` binds only a variable that is still unbound,
//! and once an `override` assignment binds a variable, later ordinary
//! assignments to the same variable set are ignored. An assignment in a
//! branch whose condition is decided at run time is a possible binding
//! beside the certain ones. Variables Make defines itself start out bound.
//!
//! A variable is expanded either while Make reads a line (a simply expanded
//! value, a rule line, an `include`, a conditional, an expression line) or
//! when a recipe runs. Reading sees the binding at that line; running sees
//! the final binding, plus the target-specific bindings of the targets the
//! recipe builds. A recursively expanded value hands its own expansion
//! points on to the variables it references.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::analysis::{Location, Reference, ReferenceContext, ReferenceKind};
use crate::builtins::is_defined_at_startup;
use crate::expansion::unexpanded_arguments;
use crate::logical::{
    find_top_level_assignment, find_top_level_rule_separator, inline_recipe_separator,
    strip_top_level_comment, target_assignment, LogicalKind, LogicalStatement, Reach,
};
use crate::parser::{AssignmentOperator, Makefile, Variable, VariableScope};
use crate::syntax::is_modifier;

/// The assignments a variable may be bound to at one point of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Binding {
    /// Indices into `Makefile::assignments`, in source order.
    assignments: Vec<usize>,
    /// Whether one of them certainly took effect.
    certain: bool,
    /// Whether an `override` assignment certainly took effect.
    overridden: bool,
}

impl Binding {
    /// The binding of the global variable `name` just before `line`. A
    /// variable Make defines at startup is bound before the first line, so
    /// `?=` leaves it alone and `+=` extends a recursively expanded value.
    pub(crate) fn before(assignments: &[Variable], name: &str, line: usize) -> Self {
        let mut binding = Self {
            certain: is_defined_at_startup(name),
            ..Self::default()
        };
        for (index, variable) in assignments.iter().enumerate() {
            if variable.name == name
                && variable.scope == VariableScope::Global
                && variable.line < line
            {
                binding.apply(assignments, index);
            }
        }
        binding
    }

    fn apply(&mut self, assignments: &[Variable], index: usize) {
        let variable = &assignments[index];
        let certain = match variable.reach {
            Reach::Never => return,
            Reach::Conditional => false,
            Reach::Always => true,
        };
        if self.overridden && !variable.modifiers.override_ {
            return;
        }
        match variable.operator {
            AssignmentOperator::Append => {}
            AssignmentOperator::Conditional if self.certain => return,
            AssignmentOperator::Conditional => {}
            _ if certain && variable.modifiers.override_ => self.assignments.clear(),
            // An `override` that may have taken effect keeps protecting its
            // value against ordinary assignments to the same variable set.
            _ if certain => self.assignments.retain(|&kept| {
                let kept = &assignments[kept];
                kept.modifiers.override_ && is_global(kept) == is_global(variable)
            }),
            _ => {}
        }
        self.assignments.push(index);
        self.certain |= certain;
        self.overridden |= certain && variable.modifiers.override_;
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        self.assignments.contains(&index)
    }

    /// Whether a target-specific assignment certainly supplies the whole
    /// value, so that a recipe sees neither the global binding nor the
    /// bindings it would inherit from the targets it is built for.
    fn replaced_by_target(&self, assignments: &[Variable]) -> bool {
        self.assignments.first().is_some_and(|&index| {
            let variable = &assignments[index];
            !is_global(variable)
                && variable.reach == Reach::Always
                && variable.operator != AssignmentOperator::Append
        })
    }

    /// Whether the variable is certainly simply expanded, so that `+=`
    /// expands the appended text at once. `!=` and `:::=` expand their
    /// value when read but leave a recursively expanded variable behind.
    pub(crate) fn is_simple(&self, assignments: &[Variable]) -> bool {
        let mut flavors = self
            .assignments
            .iter()
            .map(|&index| assignments[index].operator)
            .filter(|operator| *operator != AssignmentOperator::Append)
            .peekable();
        self.certain
            && flavors.peek().is_some()
            && flavors.all(|operator| {
                matches!(
                    operator,
                    AssignmentOperator::Simple | AssignmentOperator::SimplePosix
                )
            })
    }
}

/// Global and target-specific assignments form separate variable sets.
fn is_global(variable: &Variable) -> bool {
    variable.scope == VariableScope::Global
}

/// Whether Make passes a variable of this name to a shell when everything
/// is exported. Only a name made of letters, digits and underscores is.
fn is_exportable(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// One `export` or `unexport` event, in the order Make reads them.
enum Export<'a> {
    /// A bare `export` or `unexport`.
    All(bool),
    /// A directive or modifier naming one variable.
    Name(&'a str, bool),
}

/// Whether the value of `variable`, read after `assignments`, is expanded
/// later than the assignment itself. Appending to a simply expanded global
/// variable expands at once; appending to anything else is deferred, and so
/// is every target-specific append, which Make resolves against the
/// target's own variables when the recipe runs.
pub(crate) fn value_expands_later(assignments: &[Variable], variable: &Variable) -> bool {
    match variable.operator {
        AssignmentOperator::Append => match variable.scope {
            VariableScope::Global => {
                !Binding::before(assignments, &variable.name, variable.line).is_simple(assignments)
            }
            VariableScope::TargetSpecific(_) => true,
        },
        operator => !operator.expands_immediately(),
    }
}

/// Where a variable is expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Point {
    /// While Make reads this line.
    Reading(usize),
    /// When the recipe of this rule, an index into `Makefile::rules`, runs.
    Running(usize),
}

/// When a reference is expanded.
enum Timing {
    At(Point),
    /// Together with the value of this assignment, an index into
    /// `Makefile::assignments`.
    WithValue(usize),
}

/// The expansion points of every variable referenced in a Makefile.
pub(crate) struct Expansions<'a> {
    makefile: &'a Makefile,
    points: BTreeMap<String, BTreeSet<Point>>,
}

impl<'a> Expansions<'a> {
    pub(crate) fn build(makefile: &'a Makefile) -> Self {
        let mut expansions = Self {
            makefile,
            points: BTreeMap::new(),
        };
        let locator = Locator::new(makefile);
        let mut carried = Vec::new();
        for reference in &makefile.analysis().references {
            if reference.kind != ReferenceKind::Variable {
                continue;
            }
            match locator.timing(reference) {
                Some(Timing::At(point)) => {
                    expansions
                        .points
                        .entry(reference.name.clone())
                        .or_default()
                        .insert(point);
                }
                Some(Timing::WithValue(holder)) => carried.push((reference.name.clone(), holder)),
                None => {}
            }
        }
        expansions.add_environment_points(&locator);
        expansions.propagate(&carried);
        expansions
    }

    /// Adds the point at which Make expands an exported variable: a recipe
    /// that runs a command is handed the final value of every exported
    /// variable in its environment. A variable is exported by the last
    /// `export` or `unexport` directive or modifier that names it, or, when
    /// a bare `export` that no later bare `unexport` cancels, or
    /// `.EXPORT_ALL_VARIABLES`, is in effect, by having a name a shell
    /// accepts. A name Make computes by expansion is left out, as is a
    /// target-specific export, which reaches only the recipes of its own
    /// targets.
    fn add_environment_points(&mut self, locator: &Locator<'a>) {
        let makefile = self.makefile;
        let mut events: Vec<(usize, Export<'a>)> = Vec::new();
        for statement in makefile.logical.statements() {
            if statement.kind != LogicalKind::Directive || statement.reach == Reach::Never {
                continue;
            }
            let mut words = statement.text().split_whitespace();
            let exporting = match words.next() {
                Some("export") => true,
                Some("unexport") => false,
                _ => continue,
            };
            let mut names = words.skip_while(|word| is_modifier(word)).peekable();
            if names.peek().is_none() {
                events.push((statement.start_line, Export::All(exporting)));
            }
            for name in names.filter(|name| !name.contains('$')) {
                events.push((statement.start_line, Export::Name(name, exporting)));
            }
        }
        for variable in makefile
            .assignments
            .iter()
            .filter(|variable| is_global(variable))
        {
            if variable.reach == Reach::Never {
                continue;
            }
            if variable.modifiers.unexport {
                events.push((variable.line, Export::Name(&variable.name, false)));
            } else if variable.modifiers.export {
                events.push((variable.line, Export::Name(&variable.name, true)));
            }
        }
        events.sort_by_key(|(line, _)| *line);

        let mut export_all = false;
        let mut states: BTreeMap<&str, bool> = BTreeMap::new();
        for (_, event) in events {
            match event {
                Export::All(exporting) => export_all = exporting,
                Export::Name(name, exporting) => {
                    states.insert(name, exporting);
                }
            }
        }
        export_all |= makefile.rules.iter().any(|rule| {
            rule.targets
                .iter()
                .any(|target| target == ".EXPORT_ALL_VARIABLES")
        });
        let mut exported: BTreeSet<&str> = states
            .iter()
            .filter_map(|(name, exporting)| exporting.then_some(*name))
            .collect();
        if export_all {
            exported.extend(
                makefile
                    .assignments
                    .iter()
                    .filter(|variable| is_global(variable) && variable.reach != Reach::Never)
                    .map(|variable| variable.name.as_str())
                    .filter(|name| is_exportable(name) && states.get(name) != Some(&false)),
            );
        }
        if exported.is_empty() {
            return;
        }

        let running: Vec<usize> = makefile
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                rule.recipes.iter().any(|recipe| {
                    !recipe.command.trim().is_empty()
                        && locator
                            .statement_at(recipe.line)
                            .is_some_and(|statement| statement.reach != Reach::Never)
                })
            })
            .map(|(index, _)| index)
            .collect();
        for name in exported {
            let points = self.points.entry(name.to_string()).or_default();
            points.extend(running.iter().map(|rule| Point::Running(*rule)));
        }
    }

    /// Hands the expansion points of each recursively expanded value on to
    /// the variables it references, for the points at which that value is
    /// bound, until nothing changes.
    fn propagate(&mut self, carried: &[(String, usize)]) {
        loop {
            let mut changed = false;
            for (name, holder) in carried {
                let holder_name = &self.makefile.assignments[*holder].name;
                let reached: Vec<Point> = self
                    .points
                    .get(holder_name)
                    .map(|points| {
                        points
                            .iter()
                            .copied()
                            .filter(|point| self.is_bound_at(*holder, *point))
                            .collect()
                    })
                    .unwrap_or_default();
                if reached.is_empty() {
                    continue;
                }
                let points = self.points.entry(name.clone()).or_default();
                for point in reached {
                    changed |= points.insert(point);
                }
            }
            if !changed {
                return;
            }
        }
    }

    /// Whether the value assigned to `name` on `line` is expanded anywhere.
    pub(crate) fn expands_value(&self, name: &str, line: usize) -> bool {
        let Some(index) = self.makefile.assignments.iter().position(|variable| {
            variable.name == name && variable.line <= line && line <= variable.end_line
        }) else {
            return false;
        };
        self.points
            .get(name)
            .is_some_and(|points| points.iter().any(|point| self.is_bound_at(index, *point)))
    }

    /// Whether the assignment at `index` is part of its variable's binding
    /// at `point`. A target-specific assignment is bound only while a recipe
    /// that builds one of its targets, or a prerequisite of one, runs, and
    /// not once the recipe's own targets certainly rebind the variable.
    fn is_bound_at(&self, index: usize, point: Point) -> bool {
        let assignments = &self.makefile.assignments;
        let variable = &assignments[index];
        match (&variable.scope, point) {
            (VariableScope::Global, Point::Reading(line)) => {
                Binding::before(assignments, &variable.name, line).contains(index)
            }
            (VariableScope::TargetSpecific(_), Point::Reading(_)) => false,
            (VariableScope::Global, Point::Running(rule)) => {
                self.recipe_binding(rule, &variable.name).contains(index)
            }
            (VariableScope::TargetSpecific(targets), Point::Running(rule)) => {
                let binding = self.recipe_binding(rule, &variable.name);
                if self.certainly_builds(rule, targets) {
                    binding.contains(index)
                } else {
                    variable.reach != Reach::Never
                        && self.recipe_builds(rule, targets)
                        && !binding.replaced_by_target(assignments)
                }
            }
        }
    }

    /// The binding the recipe of `rule` runs with: the global binding at the
    /// end of the file, rebound by the target-specific assignments that
    /// certainly reach every target of the rule. Those win over a global
    /// `override`, since `override` only protects against later assignments
    /// to the same variable set.
    fn recipe_binding(&self, rule: usize, name: &str) -> Binding {
        let assignments = &self.makefile.assignments;
        let mut binding = Binding::before(assignments, name, usize::MAX);
        binding.overridden = false;
        for (index, variable) in assignments.iter().enumerate() {
            if variable.name != name {
                continue;
            }
            if let VariableScope::TargetSpecific(targets) = &variable.scope {
                if self.certainly_builds(rule, targets) {
                    binding.apply(assignments, index);
                }
            }
        }
        binding
    }

    /// Whether every target of `rule` is named literally among `targets`,
    /// so that their target-specific variables apply whenever the recipe
    /// runs. A pattern only possibly applies: an assignment to the exact
    /// target takes precedence over it regardless of order.
    fn certainly_builds(&self, rule: usize, targets: &[String]) -> bool {
        let rule = &self.makefile.rules[rule];
        !rule.targets.is_empty()
            && rule
                .targets
                .iter()
                .all(|target| !target.contains(['%', '$']) && targets.contains(target))
    }

    /// Whether the recipe of `rule` may run with the target-specific
    /// variables of `targets`: it builds one of them or one of their
    /// prerequisites, or a pattern or computed name on either side leaves
    /// that open.
    fn recipe_builds(&self, rule: usize, targets: &[String]) -> bool {
        let rule = &self.makefile.rules[rule];
        if targets
            .iter()
            .chain(&rule.targets)
            .any(|target| target.contains(['%', '$']))
        {
            return true;
        }
        let index = self.makefile.analysis();
        let mut queue: VecDeque<&str> = targets.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name) {
                continue;
            }
            if rule.targets.iter().any(|target| target == name) {
                return true;
            }
            if let Some(symbol) = index.target(name) {
                queue.extend(
                    symbol
                        .dependencies
                        .iter()
                        .map(|edge| edge.prerequisite.as_str()),
                );
            }
        }
        false
    }
}

/// Maps references back to the statements that contain them.
struct Locator<'a> {
    makefile: &'a Makefile,
    line_starts: Vec<usize>,
}

impl<'a> Locator<'a> {
    fn new(makefile: &'a Makefile) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            makefile
                .syntax
                .source()
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            makefile,
            line_starts,
        }
    }

    fn timing(&self, reference: &Reference) -> Option<Timing> {
        let line = reference.location.line;
        let statement = self.statement_at(line)?;
        if statement.reach == Reach::Never || self.skipped_by_function(statement, reference) {
            return None;
        }
        match reference.context {
            ReferenceContext::Recipe => self
                .rule_at(line)
                .map(|rule| Timing::At(Point::Running(rule))),
            ReferenceContext::Rule => self.rule_line_timing(reference),
            ReferenceContext::Assignment => self.assignment_timing(reference),
            ReferenceContext::Definition => Some(self.definition_timing(line)),
            ReferenceContext::Include
            | ReferenceContext::Conditional
            | ReferenceContext::Directive
            | ReferenceContext::Other => Some(Timing::At(Point::Reading(line))),
        }
    }

    fn rule_at(&self, line: usize) -> Option<usize> {
        self.makefile
            .rules
            .iter()
            .position(|rule| rule.line <= line && line <= rule.end_line)
    }

    fn statement_at(&self, line: usize) -> Option<&'a LogicalStatement> {
        let statements = self.makefile.logical.statements();
        let index = statements
            .partition_point(|statement| statement.start_line <= line)
            .checked_sub(1)?;
        let statement = &statements[index];
        (line <= statement.end_line).then_some(statement)
    }

    /// Whether the reference sits in a function argument that a literal
    /// argument of the same call keeps Make from ever expanding.
    fn skipped_by_function(&self, statement: &LogicalStatement, reference: &Reference) -> bool {
        let Some(offset) = self.offset_in(statement, reference.location) else {
            return false;
        };
        unexpanded_arguments(statement.raw(self.makefile.syntax.source()))
            .iter()
            .any(|argument| argument.contains(&offset))
    }

    /// The byte offset of `location` within the raw text of `statement`.
    fn offset_in(&self, statement: &LogicalStatement, location: Location) -> Option<usize> {
        let line_start = *self.line_starts.get(location.line.checked_sub(1)?)?;
        let (offset, _) = self.makefile.syntax.source()[line_start..]
            .char_indices()
            .nth(location.column.checked_sub(1)?)?;
        (line_start + offset).checked_sub(statement.span.start.offset)
    }

    /// A rule line expands its targets, prerequisites and the name of a
    /// target-specific variable while read. The value of that variable
    /// follows its operator, and an inline recipe runs with the rule.
    fn rule_line_timing(&self, reference: &Reference) -> Option<Timing> {
        let statement = self.statement_at(reference.location.line)?;
        let raw = statement.raw(self.makefile.syntax.source());
        let offset = self.offset_in(statement, reference.location)?;
        let leading = raw.len() - raw.trim_start().len();
        let separator = find_top_level_rule_separator(&raw[leading..])?;
        let body_start = leading + separator.position + separator.length;
        let semicolon =
            inline_recipe_separator(&raw[body_start..]).map(|position| body_start + position);
        if semicolon.is_some_and(|semicolon| offset > semicolon) {
            let rule = self
                .makefile
                .rules
                .iter()
                .position(|rule| rule.line == statement.start_line)?;
            return Some(Timing::At(Point::Running(rule)));
        }
        let prerequisites = &raw[body_start..semicolon.unwrap_or(raw.len())];
        if let Some((position, operator)) =
            target_assignment(strip_top_level_comment(prerequisites))
        {
            if offset >= body_start + position + operator.len() {
                let assignment = self.makefile.assignments.iter().position(|variable| {
                    variable.line == statement.start_line && variable.scope != VariableScope::Global
                })?;
                return Some(self.value_timing(assignment, statement.start_line));
            }
        }
        Some(Timing::At(Point::Reading(statement.start_line)))
    }

    /// An assignment expands its name while read and its value according
    /// to the operator.
    fn assignment_timing(&self, reference: &Reference) -> Option<Timing> {
        let statement = self.statement_at(reference.location.line)?;
        let raw = statement.raw(self.makefile.syntax.source());
        let offset = self.offset_in(statement, reference.location)?;
        let leading = raw.len() - raw.trim_start().len();
        let reading = Timing::At(Point::Reading(statement.start_line));
        let Some((position, _)) = find_top_level_assignment(&raw[leading..]) else {
            return Some(reading);
        };
        if offset < leading + position {
            return Some(reading);
        }
        let Some(assignment) = self.makefile.assignments.iter().position(|variable| {
            variable.line == statement.start_line && variable.scope == VariableScope::Global
        }) else {
            return Some(reading);
        };
        Some(self.value_timing(assignment, statement.start_line))
    }

    /// A `define` header is expanded while read; the body follows the
    /// operator of the definition.
    fn definition_timing(&self, line: usize) -> Timing {
        let body_of = self.makefile.assignments.iter().position(|variable| {
            variable.scope == VariableScope::Global
                && variable.line < line
                && line <= variable.end_line
        });
        match body_of {
            Some(assignment) => self.value_timing(assignment, line),
            None => Timing::At(Point::Reading(line)),
        }
    }

    fn value_timing(&self, assignment: usize, line: usize) -> Timing {
        let assignments = &self.makefile.assignments;
        if value_expands_later(assignments, &assignments[assignment]) {
            Timing::WithValue(assignment)
        } else {
            Timing::At(Point::Reading(line))
        }
    }
}
