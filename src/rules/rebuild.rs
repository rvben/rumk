//! Incremental-build pitfalls. Intentional force targets remain an opt-in policy.

use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::Makefile;
use crate::project::Project;
use crate::rules::{Rule, RuleCategory};
use std::collections::BTreeSet;

pub struct PhonyPrerequisite;

impl Rule for PhonyPrerequisite {
    fn id(&self) -> &'static str {
        "MK217"
    }
    fn name(&self) -> &'static str {
        "Phony prerequisite forces a target to rebuild"
    }
    fn description(&self) -> &'static str {
        "An ordinary phony prerequisite forces a non-phony target's recipe to run on every invocation. Use an order-only prerequisite for setup, a real file for freshness, or suppress this opt-in rule for intentional force targets."
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::BestPractices
    }
    fn project_aware(&self) -> bool {
        true
    }
    fn check(&self, _: &Makefile, _: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        // An unread include or dynamic declaration can mark the consumer phony.
        if !super::policy::complete_target_graph(project) {
            return Vec::new();
        }
        let index = project.analysis();
        if index.targets.contains_key(".SECONDEXPANSION") {
            return Vec::new();
        }
        let mut seen = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for target in index.targets.values() {
            if target.phony
                || target.special
                || target.name.contains('%')
                // Double-colon declarations have independent recipes and
                // prerequisite lists; the merged target index loses that link.
                || target.declarations.iter().any(|decl| decl.double_colon)
                || !target
                    .declarations
                    .iter()
                    .any(|decl| decl.has_recipe && index.is_definitely_active(decl.location))
            {
                continue;
            }
            for edge in &target.dependencies {
                if edge.order_only
                    || !index.is_definitely_active(edge.location)
                    || !index
                        .targets
                        .get(&edge.prerequisite)
                        .is_some_and(|input| input.phony)
                    || !seen.insert((
                        target.name.as_str(),
                        edge.prerequisite.as_str(),
                        edge.location,
                    ))
                {
                    continue;
                }
                diagnostics.push(Diagnostic::new(self.id(), Severity::Warning,
                    format!("Phony prerequisite '{}' forces non-phony target '{}' to rebuild; use an order-only prerequisite if this is only setup", edge.prerequisite, target.name),
                    edge.location.line, edge.location.column)
                    .with_source(project.file(edge.location.source).path.clone()));
            }
        }
        diagnostics
    }
}
