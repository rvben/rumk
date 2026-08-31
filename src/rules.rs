use crate::diagnostic::Diagnostic;
use crate::parser::Makefile;
use crate::project::Project;
use anyhow::{bail, Result};

pub mod best_practices;
mod phony;
pub mod project;
pub mod style;
pub mod syntax;

pub const RULE_IDS: &[&str] = &[
    "MK001", "MK002", "MK003", "MK004", "MK005", "MK101", "MK102", "MK103", "MK201", "MK202",
    "MK203", "MK204", "MK205", "MK206", "MK207", "MK208", "MK209", "MK210",
];

pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn category(&self) -> RuleCategory;
    fn fixable(&self) -> bool {
        false
    }
    fn project_aware(&self) -> bool {
        false
    }
    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic>;
    fn check_project(&self, _project: &Project) -> Vec<Diagnostic> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Syntax,
    Style,
    BestPractices,
}

impl RuleCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Style => "style",
            Self::BestPractices => "best-practices",
        }
    }
}

pub fn documentation_url(rule_id: &str) -> String {
    format!(
        "https://github.com/rvben/rumk/blob/main/docs/{}.md",
        rule_id.to_ascii_lowercase()
    )
}

pub fn get_rule_explanation(rule_id: &str) -> Result<String> {
    let all_rules = get_all_rules();

    for rule in all_rules {
        if rule.id() == rule_id {
            return Ok(format!(
                "Rule: {}\nCategory: {:?}\nDescription: {}\n\n{}",
                rule.id(),
                rule.category(),
                rule.name(),
                rule.description()
            ));
        }
    }

    bail!("Unknown rule: {}", rule_id)
}

pub fn get_all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(syntax::TabInRecipe),
        Box::new(syntax::InvalidVariableSyntax),
        Box::new(syntax::ConditionalStructure),
        Box::new(project::MixedTargetSeparators),
        Box::new(syntax::SpecialTargetPlacement),
        Box::new(style::LineLength::new(120)),
        Box::new(style::VariableNaming::new(style::NamingStyle::Upper)),
        Box::new(style::TargetNaming::new(style::NamingStyle::Lower)),
        Box::new(best_practices::MissingPhony::default()),
        Box::new(best_practices::HardcodedPath),
        Box::new(best_practices::RecursiveMake),
        Box::new(best_practices::DuplicateRecipe),
        Box::new(best_practices::DependencyCycle),
        Box::new(project::MissingInclude),
        Box::new(project::IncludeCycle),
        Box::new(project::UndefinedVariableReference::default()),
        Box::new(project::UnreachableTarget::default()),
        Box::new(project::UnresolvedIncludeExpression),
    ]
}

pub fn get_default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(syntax::TabInRecipe),
        Box::new(syntax::InvalidVariableSyntax),
        Box::new(syntax::ConditionalStructure),
        Box::new(project::MixedTargetSeparators),
        Box::new(syntax::SpecialTargetPlacement),
        Box::new(style::LineLength::new(120)),
        Box::new(best_practices::MissingPhony::default()),
        Box::new(best_practices::RecursiveMake),
        Box::new(best_practices::DuplicateRecipe),
        Box::new(best_practices::DependencyCycle),
        Box::new(project::MissingInclude),
        Box::new(project::IncludeCycle),
    ]
}
