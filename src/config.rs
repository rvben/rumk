use crate::rules::{self, Rule};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    pub rules: Vec<Box<dyn Rule>>,
    pub rule_config: HashMap<String, RuleConfig>,
    pub ignore: IgnoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    pub enabled: bool,
    pub severity: Option<String>,
    pub options: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    pub rules: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rules: rules::get_default_rules(),
            rule_config: HashMap::new(),
            ignore: IgnoreConfig::default(),
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let toml_config: TomlConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(toml_config.into_config())
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    rules: HashMap<String, RuleConfig>,

    #[serde(default)]
    ignore: IgnoreConfig,
}

impl TomlConfig {
    fn into_config(self) -> Config {
        let mut rules = Vec::new();
        let all_rules = rules::get_all_rules();

        for rule in all_rules {
            let rule_id = rule.id();

            if let Some(config) = self.rules.get(rule_id) {
                if config.enabled {
                    rules.push(rule);
                }
            } else {
                rules.push(rule);
            }
        }

        Config {
            rules,
            rule_config: self.rules,
            ignore: self.ignore,
        }
    }
}
