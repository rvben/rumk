use crate::diagnostic::Diagnostic;
use crate::parser::Makefile;
use anyhow::{bail, Result};

pub mod best_practices;
pub mod style;
pub mod syntax;

pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn category(&self) -> RuleCategory;
    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Syntax,
    Style,
    BestPractices,
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
        Box::new(style::LineLength::new(120)),
        Box::new(style::VariableNaming::new(style::NamingStyle::Upper)),
        Box::new(style::TargetNaming::new(style::NamingStyle::Lower)),
        Box::new(best_practices::MissingPhony),
        Box::new(best_practices::HardcodedPath),
    ]
}

pub fn get_default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(syntax::TabInRecipe),
        Box::new(syntax::InvalidVariableSyntax),
        Box::new(style::LineLength::new(120)),
        Box::new(best_practices::MissingPhony),
    ]
}
