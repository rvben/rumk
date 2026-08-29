use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::Makefile;
use crate::rules::{self, Rule, RuleCategory};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_RULES: &[&str] = &["MK001", "MK002", "MK101", "MK201"];
const ALL_RULES: &[&str] = &[
    "MK001", "MK002", "MK101", "MK102", "MK103", "MK201", "MK202",
];

pub struct Config {
    pub rules: Vec<Box<dyn Rule>>,
    ignore_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,

    #[serde(default)]
    pub severity: Option<String>,

    #[serde(default)]
    pub options: HashMap<String, toml::Value>,
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IgnoreConfig {
    #[serde(default)]
    pub paths: Vec<String>,

    #[serde(default)]
    pub rules: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rules: rules::get_default_rules(),
            ignore_paths: Vec::new(),
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let toml_config: TomlConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        toml_config
            .into_config()
            .with_context(|| format!("Invalid config file: {}", path.display()))
    }

    pub fn find_and_load() -> Result<Self> {
        let possible_paths = [
            PathBuf::from(".rumk.toml"),
            PathBuf::from("rumk.toml"),
            PathBuf::from(".config/rumk.toml"),
        ];

        for path in &possible_paths {
            if path.exists() {
                return Self::from_file(path);
            }
        }

        Ok(Self::default())
    }

    pub fn is_path_ignored(&self, path: &Path) -> bool {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);

        self.ignore_paths
            .iter()
            .any(|pattern| glob_matches(pattern, normalized))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    rules: HashMap<String, RuleConfig>,

    #[serde(default)]
    ignore: IgnoreConfig,
}

impl TomlConfig {
    fn into_config(self) -> Result<Config> {
        for rule_id in self.rules.keys() {
            if !ALL_RULES.contains(&rule_id.as_str()) {
                bail!("Unknown rule: {rule_id}");
            }
        }

        let ignored_rules: HashSet<_> = self.ignore.rules.iter().map(String::as_str).collect();
        for rule_id in &ignored_rules {
            if !ALL_RULES.contains(rule_id) {
                bail!("Unknown ignored rule: {rule_id}");
            }
        }

        let mut configured_rules = Vec::new();
        for rule_id in ALL_RULES {
            let rule_config = self.rules.get(*rule_id);
            let enabled = rule_config
                .map(|config| config.enabled)
                .unwrap_or_else(|| DEFAULT_RULES.contains(rule_id));

            if !enabled || ignored_rules.contains(rule_id) {
                continue;
            }

            let rule = build_rule(rule_id, rule_config)?;
            let severity = rule_config
                .and_then(|config| config.severity.as_deref())
                .map(parse_severity)
                .transpose()?;

            if let Some(severity) = severity {
                configured_rules
                    .push(Box::new(SeverityOverride { rule, severity }) as Box<dyn Rule>);
            } else {
                configured_rules.push(rule);
            }
        }

        Ok(Config {
            rules: configured_rules,
            ignore_paths: self.ignore.paths,
        })
    }
}

fn build_rule(rule_id: &str, config: Option<&RuleConfig>) -> Result<Box<dyn Rule>> {
    let options = config.map(|config| &config.options);

    match rule_id {
        "MK001" => {
            ensure_options(rule_id, options, &[])?;
            Ok(Box::new(rules::syntax::TabInRecipe))
        }
        "MK002" => {
            ensure_options(rule_id, options, &[])?;
            Ok(Box::new(rules::syntax::InvalidVariableSyntax))
        }
        "MK101" => {
            ensure_options(rule_id, options, &["max"])?;
            let max_length = integer_option(rule_id, options, "max", 120)?;
            Ok(Box::new(rules::style::LineLength::new(max_length)))
        }
        "MK102" => {
            ensure_options(rule_id, options, &["style"])?;
            let style = naming_style_option(rule_id, options, rules::style::NamingStyle::Upper)?;
            Ok(Box::new(rules::style::VariableNaming::new(style)))
        }
        "MK103" => {
            ensure_options(rule_id, options, &["style"])?;
            let style = naming_style_option(rule_id, options, rules::style::NamingStyle::Lower)?;
            Ok(Box::new(rules::style::TargetNaming::new(style)))
        }
        "MK201" => {
            ensure_options(rule_id, options, &[])?;
            Ok(Box::new(rules::best_practices::MissingPhony))
        }
        "MK202" => {
            ensure_options(rule_id, options, &[])?;
            Ok(Box::new(rules::best_practices::HardcodedPath))
        }
        _ => bail!("Unknown rule: {rule_id}"),
    }
}

fn ensure_options(
    rule_id: &str,
    options: Option<&HashMap<String, toml::Value>>,
    allowed: &[&str],
) -> Result<()> {
    if let Some(option) = options.and_then(|options| {
        options
            .keys()
            .find(|option| !allowed.contains(&option.as_str()))
    }) {
        bail!("Unknown option '{option}' for rule {rule_id}");
    }
    Ok(())
}

fn integer_option(
    rule_id: &str,
    options: Option<&HashMap<String, toml::Value>>,
    name: &str,
    default: usize,
) -> Result<usize> {
    let Some(value) = options.and_then(|options| options.get(name)) else {
        return Ok(default);
    };
    let Some(value) = value.as_integer() else {
        bail!("Option '{name}' for rule {rule_id} must be an integer");
    };
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .with_context(|| format!("Option '{name}' for rule {rule_id} must be positive"))
}

fn naming_style_option(
    rule_id: &str,
    options: Option<&HashMap<String, toml::Value>>,
    default: rules::style::NamingStyle,
) -> Result<rules::style::NamingStyle> {
    let Some(value) = options.and_then(|options| options.get("style")) else {
        return Ok(default);
    };
    let Some(value) = value.as_str() else {
        bail!("Option 'style' for rule {rule_id} must be a string");
    };

    match value.to_ascii_lowercase().as_str() {
        "upper" | "upper_case" => Ok(rules::style::NamingStyle::Upper),
        "lower" | "lower_case" => Ok(rules::style::NamingStyle::Lower),
        _ => bail!("Unsupported naming style '{value}' for rule {rule_id}"),
    }
}

fn parse_severity(value: &str) -> Result<Severity> {
    match value.to_ascii_lowercase().as_str() {
        "error" => Ok(Severity::Error),
        "warning" | "warn" => Ok(Severity::Warning),
        "info" => Ok(Severity::Info),
        _ => bail!("Unsupported severity: {value}"),
    }
}

struct SeverityOverride {
    rule: Box<dyn Rule>,
    severity: Severity,
}

impl Rule for SeverityOverride {
    fn id(&self) -> &'static str {
        self.rule.id()
    }

    fn name(&self) -> &'static str {
        self.rule.name()
    }

    fn description(&self) -> &'static str {
        self.rule.description()
    }

    fn category(&self) -> RuleCategory {
        self.rule.category()
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = self.rule.check(makefile, content);
        for diagnostic in &mut diagnostics {
            diagnostic.severity = self.severity;
        }
        diagnostics
    }
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let pattern = pattern.strip_prefix("./").unwrap_or(&pattern);
    let pattern = pattern.as_bytes();
    let path = path.as_bytes();
    let mut memo = HashMap::new();

    fn matches_from(
        pattern: &[u8],
        path: &[u8],
        pattern_index: usize,
        path_index: usize,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(result) = memo.get(&(pattern_index, path_index)) {
            return *result;
        }

        let result = if pattern_index == pattern.len() {
            path_index == path.len()
        } else if pattern[pattern_index] == b'*' {
            let is_double = pattern.get(pattern_index + 1) == Some(&b'*');
            let next_pattern = pattern_index + if is_double { 2 } else { 1 };
            matches_from(pattern, path, next_pattern, path_index, memo)
                || (path_index < path.len()
                    && (is_double || path[path_index] != b'/')
                    && matches_from(pattern, path, pattern_index, path_index + 1, memo))
        } else if path_index < path.len()
            && ((pattern[pattern_index] == b'?' && path[path_index] != b'/')
                || pattern[pattern_index] == path[path_index])
        {
            matches_from(pattern, path, pattern_index + 1, path_index + 1, memo)
        } else {
            false
        };

        memo.insert((pattern_index, path_index), result);
        result
    }

    matches_from(pattern, path, 0, 0, &mut memo)
}
