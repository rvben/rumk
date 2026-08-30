use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::Makefile;
use crate::project::{Project, ProjectOptions};
use crate::rules::{self, Rule, RuleCategory};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const DEFAULT_RULES: &[&str] = &[
    "MK001", "MK002", "MK003", "MK004", "MK005", "MK101", "MK201", "MK203", "MK204", "MK205",
    "MK206", "MK207",
];
const ALL_RULES: &[&str] = rules::RULE_IDS;

pub struct Config {
    pub rules: Vec<Box<dyn Rule>>,
    pub global: GlobalConfig,
    settings: BTreeMap<String, RuleSettings>,
    per_file_ignores: BTreeMap<String, Vec<String>>,
    source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct GlobalConfig {
    pub enable: Option<Vec<String>>,
    pub disable: Vec<String>,
    pub extend_enable: Vec<String>,
    pub extend_disable: Vec<String>,
    pub fixable: Vec<String>,
    pub unfixable: Vec<String>,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub include_paths: Vec<String>,
    pub predefined_variables: BTreeMap<String, String>,
    pub entry_targets: Vec<String>,
    pub respect_gitignore: bool,
    pub dialect: Option<String>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            enable: None,
            disable: Vec::new(),
            extend_enable: Vec::new(),
            extend_disable: Vec::new(),
            fixable: Vec::new(),
            unfixable: Vec::new(),
            exclude: Vec::new(),
            include: Vec::new(),
            include_paths: Vec::new(),
            predefined_variables: BTreeMap::new(),
            entry_targets: Vec::new(),
            respect_gitignore: true,
            dialect: Some("gnu".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
struct RuleSettings {
    enabled: bool,
    severity: Option<Severity>,
    options: BTreeMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_parts(
            GlobalConfig::default(),
            default_settings(),
            BTreeMap::new(),
            None,
        )
        .expect("built-in rule configuration must be valid")
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self> {
        let value = load_config_value(path, &mut Vec::new())
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Self::from_value(value, Some(path.to_path_buf()))
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    pub fn find_and_load() -> Result<Self> {
        Self::find_from(&std::env::current_dir().context("Failed to determine current directory")?)
    }

    pub fn find_from(start: &Path) -> Result<Self> {
        let mut directory = if start.is_file() {
            start.parent().unwrap_or(start).to_path_buf()
        } else {
            start.to_path_buf()
        };

        loop {
            let candidates = [
                directory.join(".rumk.toml"),
                directory.join("rumk.toml"),
                directory.join(".config/rumk.toml"),
            ];
            for candidate in candidates {
                if candidate.is_file() {
                    return Self::from_file(&candidate);
                }
            }

            if directory.join(".git").exists() || !directory.pop() {
                break;
            }
        }

        Ok(Self::default())
    }

    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    pub fn project_options(&self, makefile: &Path) -> ProjectOptions {
        let config_root = self.project_root();
        let include_paths = self
            .global
            .include_paths
            .iter()
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else if let Some(root) = config_root {
                    root.join(path)
                } else {
                    path
                }
            })
            .collect();
        ProjectOptions {
            working_directory: makefile.parent().map(Path::to_path_buf),
            include_paths,
            predefined_variables: self.global.predefined_variables.clone(),
            ..ProjectOptions::default()
        }
    }

    pub fn apply_rule_overrides(
        &mut self,
        enable: Option<&[String]>,
        disable: &[String],
        extend_enable: &[String],
        extend_disable: &[String],
    ) -> Result<()> {
        if let Some(enabled) = enable {
            for settings in self.settings.values_mut() {
                settings.enabled = false;
            }
            set_enabled(&mut self.settings, enabled, true)?;
        }
        set_enabled(&mut self.settings, disable, false)?;
        set_enabled(&mut self.settings, extend_enable, true)?;
        set_enabled(&mut self.settings, extend_disable, false)?;
        self.rebuild_rules()
    }

    pub fn apply_file_overrides(
        &mut self,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        no_exclude: bool,
        respect_gitignore: Option<bool>,
    ) {
        if let Some(include) = include {
            self.global.include = include;
        }
        if no_exclude {
            self.global.exclude.clear();
        } else if let Some(exclude) = exclude {
            self.global.exclude = exclude;
        }
        if let Some(respect_gitignore) = respect_gitignore {
            self.global.respect_gitignore = respect_gitignore;
        }
    }

    pub fn apply_fix_overrides(
        &mut self,
        fixable: Option<Vec<String>>,
        unfixable: Option<Vec<String>>,
    ) -> Result<()> {
        if let Some(fixable) = fixable {
            validate_rule_ids(&fixable)?;
            self.global.fixable = fixable;
        }
        if let Some(unfixable) = unfixable {
            validate_rule_ids(&unfixable)?;
            self.global.unfixable = unfixable;
        }
        Ok(())
    }

    pub fn is_path_ignored(&self, path: &Path) -> bool {
        let normalized = self.relative_to_project(path);
        if !self.global.include.is_empty()
            && !self
                .global
                .include
                .iter()
                .any(|pattern| glob_matches(pattern, &normalized))
        {
            return true;
        }

        self.global
            .exclude
            .iter()
            .any(|pattern| glob_matches(pattern, &normalized))
    }

    pub fn is_path_excluded(&self, path: &Path) -> bool {
        let normalized = self.relative_to_project(path);
        self.global
            .exclude
            .iter()
            .any(|pattern| glob_matches(pattern, &normalized))
    }

    pub fn is_rule_fixable(&self, rule_id: &str) -> bool {
        (self.global.fixable.is_empty() || self.global.fixable.iter().any(|rule| rule == rule_id))
            && !self.global.unfixable.iter().any(|rule| rule == rule_id)
    }

    pub fn is_rule_ignored_for_path(&self, path: &Path, rule_id: &str) -> bool {
        let relative = self.relative_to_project(path);
        self.per_file_ignores.iter().any(|(pattern, rules)| {
            glob_matches(pattern, &relative) && rules.iter().any(|rule| rule == rule_id)
        })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let (section, option) = key.split_once('.')?;
        if section.eq_ignore_ascii_case("global") {
            return match option.replace('_', "-").as_str() {
                "dialect" => self.global.dialect.clone(),
                "respect-gitignore" => Some(self.global.respect_gitignore.to_string()),
                "exclude" => Some(format_string_list(&self.global.exclude)),
                "include" => Some(format_string_list(&self.global.include)),
                "include-paths" => Some(format_string_list(&self.global.include_paths)),
                "predefined-variables" => {
                    Some(format_string_map(&self.global.predefined_variables))
                }
                "entry-targets" => Some(format_string_list(&self.global.entry_targets)),
                "disable" => Some(format_string_list(&self.global.disable)),
                "extend-enable" => Some(format_string_list(&self.global.extend_enable)),
                "extend-disable" => Some(format_string_list(&self.global.extend_disable)),
                "fixable" => Some(format_string_list(&self.global.fixable)),
                "unfixable" => Some(format_string_list(&self.global.unfixable)),
                _ => None,
            };
        }

        let rule_id = section.to_ascii_uppercase();
        let settings = self.settings.get(&rule_id)?;
        match option.replace('_', "-").as_str() {
            "enabled" => Some(settings.enabled.to_string()),
            "severity" => settings.severity.map(severity_name).map(str::to_string),
            "line-length" if rule_id == "MK101" => settings.options.get("max").map(value_string),
            "style" if matches!(rule_id.as_str(), "MK102" | "MK103") => {
                settings.options.get("style").map(value_string)
            }
            _ => None,
        }
    }

    pub fn render(&self, defaults_only: bool, no_defaults: bool) -> String {
        let defaults = default_settings();
        let default_global = GlobalConfig::default();
        let global = if defaults_only {
            &default_global
        } else {
            &self.global
        };
        let mut output = String::new();

        let show_global = !no_defaults || global != &default_global;
        if show_global {
            output.push_str("[global]\n");
            render_global(&mut output, global, no_defaults.then_some(&default_global));
        }

        for rule_id in ALL_RULES {
            let settings = if defaults_only {
                &defaults[*rule_id]
            } else {
                &self.settings[*rule_id]
            };
            if no_defaults && settings == &defaults[*rule_id] {
                continue;
            }

            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&format!("[{rule_id}]\n"));
            output.push_str(&format!("enabled = {}\n", settings.enabled));
            if let Some(severity) = settings.severity {
                output.push_str(&format!("severity = {:?}\n", severity_name(severity)));
            }
            for (key, value) in &settings.options {
                let public_key = if *rule_id == "MK101" && key == "max" {
                    "line-length"
                } else {
                    key
                };
                output.push_str(&format!("{public_key} = {}\n", value));
            }
        }

        if !defaults_only && !self.per_file_ignores.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str("[per-file-ignores]\n");
            for (pattern, rules) in &self.per_file_ignores {
                output.push_str(&format!("{pattern:?} = {}\n", format_string_list(rules)));
            }
        }

        output
    }

    fn from_value(value: toml::Value, source_path: Option<PathBuf>) -> Result<Self> {
        let table = value
            .as_table()
            .context("Configuration root must be a TOML table")?;
        let mut global: GlobalConfig = table
            .get("global")
            .cloned()
            .map(toml::Value::try_into)
            .transpose()?
            .unwrap_or_default();
        validate_rule_ids(&global.disable)?;
        validate_rule_ids(&global.extend_enable)?;
        validate_rule_ids(&global.extend_disable)?;
        validate_rule_ids(&global.fixable)?;
        validate_rule_ids(&global.unfixable)?;
        if let Some(enable) = &global.enable {
            validate_rule_ids(enable)?;
        }

        let mut settings = default_settings();
        apply_global_selection(&mut settings, &global)?;

        if let Some(legacy_rules) = table.get("rules") {
            parse_legacy_rules(legacy_rules, &mut settings)?;
        }
        if let Some(legacy_ignore) = table.get("ignore") {
            parse_legacy_ignore(legacy_ignore, &mut global, &mut settings)?;
        }

        let per_file_ignores = table
            .get("per-file-ignores")
            .map(parse_per_file_ignores)
            .transpose()?
            .unwrap_or_default();

        for (key, value) in table {
            if matches!(
                key.as_str(),
                "global" | "rules" | "ignore" | "per-file-ignores"
            ) {
                continue;
            }
            let rule_id = key.to_ascii_uppercase();
            if !ALL_RULES.contains(&rule_id.as_str()) {
                bail!("Unknown configuration section: {key}");
            }
            parse_rule_section(&rule_id, value, &mut settings)?;
        }

        Self::from_parts(global, settings, per_file_ignores, source_path)
    }

    fn from_parts(
        global: GlobalConfig,
        settings: BTreeMap<String, RuleSettings>,
        per_file_ignores: BTreeMap<String, Vec<String>>,
        source_path: Option<PathBuf>,
    ) -> Result<Self> {
        let mut config = Self {
            rules: Vec::new(),
            global,
            settings,
            per_file_ignores,
            source_path,
        };
        config.rebuild_rules()?;
        Ok(config)
    }

    fn rebuild_rules(&mut self) -> Result<()> {
        self.rules = ALL_RULES
            .iter()
            .filter_map(|rule_id| {
                let settings = &self.settings[*rule_id];
                settings
                    .enabled
                    .then(|| build_rule(rule_id, settings, &self.global))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    fn relative_to_project(&self, path: &Path) -> String {
        let config_root = self.project_root();
        let current_dir = std::env::current_dir().ok();
        let root = config_root.or(current_dir.as_deref());
        let relative = root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        normalize_path(relative)
    }

    fn project_root(&self) -> Option<&Path> {
        self.source_path
            .as_deref()
            .and_then(Path::parent)
            .map(|parent| {
                if parent.file_name().is_some_and(|name| name == ".config") {
                    parent.parent().unwrap_or(parent)
                } else {
                    parent
                }
            })
    }
}

fn load_config_value(path: &Path, stack: &mut Vec<PathBuf>) -> Result<toml::Value> {
    let identity = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&identity) {
        let cycle = stack
            .iter()
            .chain(std::iter::once(&identity))
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        bail!("Configuration extends cycle: {cycle}");
    }
    stack.push(identity);

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let mut value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Invalid TOML in config file: {}", path.display()))?;
    let extends = value
        .as_table_mut()
        .context("Configuration root must be a TOML table")?
        .remove("extends");

    if let Some(extends) = extends {
        let extends = extends.as_str().context("extends must be a string path")?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let extended_path = {
            let candidate = PathBuf::from(extends);
            if candidate.is_absolute() {
                candidate
            } else {
                parent.join(candidate)
            }
        };
        let mut base = load_config_value(&extended_path, stack)
            .with_context(|| format!("Failed to extend configuration from {extends}"))?;
        merge_toml(&mut base, value);
        value = base;
    }

    stack.pop();
    Ok(value)
}

fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    merge_toml(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

impl PartialEq for RuleSettings {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.severity == other.severity
            && self.options == other.options
    }
}

fn default_settings() -> BTreeMap<String, RuleSettings> {
    ALL_RULES
        .iter()
        .map(|rule_id| {
            let mut options = BTreeMap::new();
            match *rule_id {
                "MK101" => {
                    options.insert("max".to_string(), toml::Value::Integer(120));
                }
                "MK102" => {
                    options.insert(
                        "style".to_string(),
                        toml::Value::String("upper-case".into()),
                    );
                }
                "MK103" => {
                    options.insert(
                        "style".to_string(),
                        toml::Value::String("lower-case".into()),
                    );
                }
                _ => {}
            }
            (
                (*rule_id).to_string(),
                RuleSettings {
                    enabled: DEFAULT_RULES.contains(rule_id),
                    severity: None,
                    options,
                },
            )
        })
        .collect()
}

fn apply_global_selection(
    settings: &mut BTreeMap<String, RuleSettings>,
    global: &GlobalConfig,
) -> Result<()> {
    if let Some(enabled) = &global.enable {
        for settings in settings.values_mut() {
            settings.enabled = false;
        }
        set_enabled(settings, enabled, true)?;
    }
    set_enabled(settings, &global.disable, false)?;
    set_enabled(settings, &global.extend_enable, true)?;
    set_enabled(settings, &global.extend_disable, false)
}

fn set_enabled(
    settings: &mut BTreeMap<String, RuleSettings>,
    rule_ids: &[String],
    enabled: bool,
) -> Result<()> {
    for rule_id in rule_ids {
        let canonical = rule_id.to_ascii_uppercase();
        let Some(rule) = settings.get_mut(&canonical) else {
            bail!("Unknown rule: {rule_id}");
        };
        rule.enabled = enabled;
    }
    Ok(())
}

fn validate_rule_ids(rule_ids: &[String]) -> Result<()> {
    for rule_id in rule_ids {
        if !ALL_RULES.contains(&rule_id.to_ascii_uppercase().as_str()) {
            bail!("Unknown rule: {rule_id}");
        }
    }
    Ok(())
}

fn parse_rule_section(
    rule_id: &str,
    value: &toml::Value,
    settings: &mut BTreeMap<String, RuleSettings>,
) -> Result<()> {
    let table = value
        .as_table()
        .with_context(|| format!("Rule {rule_id} must be configured as a table"))?;
    let rule = settings.get_mut(rule_id).expect("validated rule ID");
    if let Some(enabled) = table.get("enabled") {
        rule.enabled = enabled.as_bool().context("enabled must be a boolean")?;
    }
    rule.severity = table
        .get("severity")
        .map(|value| {
            value
                .as_str()
                .context("severity must be a string")
                .and_then(parse_severity)
        })
        .transpose()?;

    for (key, value) in table {
        if matches!(key.as_str(), "enabled" | "severity") {
            continue;
        }
        let key = canonical_option(rule_id, key)?;
        rule.options.insert(key.to_string(), value.clone());
    }
    Ok(())
}

fn parse_legacy_rules(
    value: &toml::Value,
    settings: &mut BTreeMap<String, RuleSettings>,
) -> Result<()> {
    let rules = value.as_table().context("rules must be a TOML table")?;
    for (rule_id, value) in rules {
        let canonical = rule_id.to_ascii_uppercase();
        if !ALL_RULES.contains(&canonical.as_str()) {
            bail!("Unknown rule: {rule_id}");
        }
        let table = value
            .as_table()
            .with_context(|| format!("Rule {rule_id} must be configured as a table"))?;
        let rule = settings.get_mut(&canonical).expect("validated rule ID");
        rule.enabled = table
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        rule.severity = table
            .get("severity")
            .and_then(toml::Value::as_str)
            .map(parse_severity)
            .transpose()?;
        if let Some(options) = table.get("options").and_then(toml::Value::as_table) {
            for (key, value) in options {
                rule.options.insert(
                    canonical_option(&canonical, key)?.to_string(),
                    value.clone(),
                );
            }
        }
    }
    Ok(())
}

fn parse_legacy_ignore(
    value: &toml::Value,
    global: &mut GlobalConfig,
    settings: &mut BTreeMap<String, RuleSettings>,
) -> Result<()> {
    let table = value.as_table().context("ignore must be a TOML table")?;
    if let Some(paths) = table.get("paths") {
        global
            .exclude
            .extend(parse_string_array(paths, "ignore.paths")?);
    }
    if let Some(rules) = table.get("rules") {
        let rule_ids = parse_string_array(rules, "ignore.rules")?;
        set_enabled(settings, &rule_ids, false)?;
    }
    Ok(())
}

fn parse_string_array(value: &toml::Value, name: &str) -> Result<Vec<String>> {
    value
        .as_array()
        .with_context(|| format!("{name} must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .with_context(|| format!("{name} entries must be strings"))
        })
        .collect()
}

fn parse_per_file_ignores(value: &toml::Value) -> Result<BTreeMap<String, Vec<String>>> {
    let table = value
        .as_table()
        .context("per-file-ignores must be a TOML table")?;
    table
        .iter()
        .map(|(pattern, value)| {
            let rules = parse_string_array(value, &format!("per-file-ignores.{pattern}"))?;
            validate_rule_ids(&rules)?;
            Ok((
                pattern.clone(),
                rules
                    .into_iter()
                    .map(|rule| rule.to_ascii_uppercase())
                    .collect(),
            ))
        })
        .collect()
}

fn render_global(output: &mut String, global: &GlobalConfig, defaults: Option<&GlobalConfig>) {
    let show = |different: bool| defaults.is_none() || different;
    if show(global.dialect != defaults.and_then(|value| value.dialect.clone())) {
        if let Some(dialect) = &global.dialect {
            output.push_str(&format!("dialect = {dialect:?}\n"));
        }
    }
    if show(global.respect_gitignore != defaults.is_some_and(|value| value.respect_gitignore)) {
        output.push_str(&format!(
            "respect-gitignore = {}\n",
            global.respect_gitignore
        ));
    }

    let lists = [
        (
            "enable",
            global.enable.as_deref().unwrap_or_default(),
            defaults
                .and_then(|value| value.enable.as_deref())
                .unwrap_or_default(),
        ),
        (
            "disable",
            global.disable.as_slice(),
            defaults
                .map(|value| value.disable.as_slice())
                .unwrap_or_default(),
        ),
        (
            "extend-enable",
            global.extend_enable.as_slice(),
            defaults
                .map(|value| value.extend_enable.as_slice())
                .unwrap_or_default(),
        ),
        (
            "extend-disable",
            global.extend_disable.as_slice(),
            defaults
                .map(|value| value.extend_disable.as_slice())
                .unwrap_or_default(),
        ),
        (
            "fixable",
            global.fixable.as_slice(),
            defaults
                .map(|value| value.fixable.as_slice())
                .unwrap_or_default(),
        ),
        (
            "unfixable",
            global.unfixable.as_slice(),
            defaults
                .map(|value| value.unfixable.as_slice())
                .unwrap_or_default(),
        ),
        (
            "exclude",
            global.exclude.as_slice(),
            defaults
                .map(|value| value.exclude.as_slice())
                .unwrap_or_default(),
        ),
        (
            "include",
            global.include.as_slice(),
            defaults
                .map(|value| value.include.as_slice())
                .unwrap_or_default(),
        ),
        (
            "include-paths",
            global.include_paths.as_slice(),
            defaults
                .map(|value| value.include_paths.as_slice())
                .unwrap_or_default(),
        ),
        (
            "entry-targets",
            global.entry_targets.as_slice(),
            defaults
                .map(|value| value.entry_targets.as_slice())
                .unwrap_or_default(),
        ),
    ];
    for (key, values, default_values) in lists {
        if show(values != default_values) {
            output.push_str(&format!("{key} = {}\n", format_string_list(values)));
        }
    }
    let default_predefined = defaults.map(|value| &value.predefined_variables);
    if show(default_predefined != Some(&global.predefined_variables)) {
        output.push_str(&format!(
            "predefined-variables = {}\n",
            format_string_map(&global.predefined_variables)
        ));
    }
}

fn canonical_option(rule_id: &str, key: &str) -> Result<&'static str> {
    match (rule_id, key.replace('_', "-").as_str()) {
        ("MK101", "line-length" | "max") => Ok("max"),
        ("MK102" | "MK103", "style") => Ok("style"),
        _ => bail!("Unknown option '{key}' for rule {rule_id}"),
    }
}

fn build_rule(
    rule_id: &str,
    settings: &RuleSettings,
    global: &GlobalConfig,
) -> Result<Box<dyn Rule>> {
    let rule: Box<dyn Rule> = match rule_id {
        "MK001" => Box::new(rules::syntax::TabInRecipe),
        "MK002" => Box::new(rules::syntax::InvalidVariableSyntax),
        "MK003" => Box::new(rules::syntax::ConditionalStructure),
        "MK004" => Box::new(rules::project::MixedTargetSeparators),
        "MK005" => Box::new(rules::syntax::SpecialTargetPlacement),
        "MK101" => Box::new(rules::style::LineLength::new(integer_option(
            rule_id, settings, "max", 120,
        )?)),
        "MK102" => Box::new(rules::style::VariableNaming::new(naming_style_option(
            rule_id,
            settings,
            rules::style::NamingStyle::Upper,
        )?)),
        "MK103" => Box::new(rules::style::TargetNaming::new(naming_style_option(
            rule_id,
            settings,
            rules::style::NamingStyle::Lower,
        )?)),
        "MK201" => Box::new(rules::best_practices::MissingPhony),
        "MK202" => Box::new(rules::best_practices::HardcodedPath),
        "MK203" => Box::new(rules::best_practices::RecursiveMake),
        "MK204" => Box::new(rules::best_practices::DuplicateRecipe),
        "MK205" => Box::new(rules::best_practices::DependencyCycle),
        "MK206" => Box::new(rules::project::MissingInclude),
        "MK207" => Box::new(rules::project::IncludeCycle),
        "MK208" => Box::new(rules::project::UndefinedVariableReference::new(
            global.predefined_variables.keys().cloned(),
        )),
        "MK209" => Box::new(rules::project::UnreachableTarget::new(
            global.entry_targets.clone(),
        )),
        _ => bail!("Unknown rule: {rule_id}"),
    };

    Ok(if let Some(severity) = settings.severity {
        Box::new(SeverityOverride { rule, severity })
    } else {
        rule
    })
}

fn integer_option(
    rule_id: &str,
    settings: &RuleSettings,
    name: &str,
    default: usize,
) -> Result<usize> {
    let Some(value) = settings.options.get(name) else {
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
    settings: &RuleSettings,
    default: rules::style::NamingStyle,
) -> Result<rules::style::NamingStyle> {
    let Some(value) = settings.options.get("style") else {
        return Ok(default);
    };
    let Some(value) = value.as_str() else {
        bail!("Option 'style' for rule {rule_id} must be a string");
    };

    match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "upper" | "upper-case" => Ok(rules::style::NamingStyle::Upper),
        "lower" | "lower-case" => Ok(rules::style::NamingStyle::Lower),
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

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn value_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn format_string_list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("{value:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_string_map(values: &BTreeMap<String, String>) -> String {
    format!(
        "{{{}}}",
        values
            .iter()
            .map(|(key, value)| format!("{key:?} = {value:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
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

    fn fixable(&self) -> bool {
        self.rule.fixable()
    }

    fn project_aware(&self) -> bool {
        self.rule.project_aware()
    }

    fn check(&self, makefile: &Makefile, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = self.rule.check(makefile, content);
        for diagnostic in &mut diagnostics {
            diagnostic.severity = self.severity;
        }
        diagnostics
    }

    fn check_project(&self, project: &Project) -> Vec<Diagnostic> {
        let mut diagnostics = self.rule.check_project(project);
        for diagnostic in &mut diagnostics {
            diagnostic.severity = self.severity;
        }
        diagnostics
    }
}

fn normalize_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized
        .strip_prefix("./")
        .unwrap_or(&normalized)
        .to_string()
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
