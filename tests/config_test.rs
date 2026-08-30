use rumk::config::Config;
use rumk::diagnostic::Severity;
use rumk::parser::parse;

#[test]
fn an_empty_config_keeps_the_builtin_default_rule_set() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("empty.toml");
    std::fs::write(&path, "").unwrap();

    let config = Config::from_file(&path).unwrap();
    let ids: Vec<_> = config.rules.iter().map(|rule| rule.id()).collect();

    assert_eq!(
        ids,
        [
            "MK001", "MK002", "MK003", "MK004", "MK005", "MK101", "MK201", "MK203", "MK204",
            "MK205", "MK206", "MK207"
        ]
    );
}

#[test]
fn config_applies_options_severity_and_rule_ignores() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rumk.toml");
    std::fs::write(
        &path,
        r#"
[rules]
MK101 = { enabled = true, severity = "error", options = { max = 3 } }
MK201 = { enabled = false }
MK202 = { enabled = true }

[ignore]
paths = ["vendor/**"]
rules = ["MK202"]
"#,
    )
    .unwrap();

    let config = Config::from_file(&path).unwrap();
    let ids: Vec<_> = config.rules.iter().map(|rule| rule.id()).collect();
    assert_eq!(
        ids,
        [
            "MK001", "MK002", "MK003", "MK004", "MK005", "MK101", "MK203", "MK204", "MK205",
            "MK206", "MK207"
        ]
    );
    assert!(config.is_path_ignored(std::path::Path::new("vendor/lib/Makefile")));
    assert!(!config.is_path_ignored(std::path::Path::new("src/Makefile")));

    let makefile = parse("1234\n").unwrap();
    let line_length = config
        .rules
        .iter()
        .find(|rule| rule.id() == "MK101")
        .unwrap();
    let diagnostics = line_length.check(&makefile, "1234\n");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
}

#[test]
fn rumdl_shaped_config_is_canonical() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(".rumk.toml");
    std::fs::write(
        &path,
        r#"
[global]
exclude = ["vendor/**"]
disable = ["MK201"]

[MK101]
line-length = 3
severity = "error"

[MK202]
enabled = true

[per-file-ignores]
"vendor/**" = ["MK001"]
"#,
    )
    .unwrap();

    let config = Config::from_file(&path).unwrap();
    let ids: Vec<_> = config.rules.iter().map(|rule| rule.id()).collect();
    assert_eq!(
        ids,
        [
            "MK001", "MK002", "MK003", "MK004", "MK005", "MK101", "MK202", "MK203", "MK204",
            "MK205", "MK206", "MK207"
        ]
    );
    assert!(config.is_path_ignored(std::path::Path::new("vendor/a.mk")));
    assert!(
        config.is_rule_ignored_for_path(directory.path().join("vendor/a.mk").as_path(), "MK001")
    );
    assert!(config.render(false, false).contains("[per-file-ignores]"));

    let diagnostics = config
        .rules
        .iter()
        .find(|rule| rule.id() == "MK101")
        .unwrap()
        .check(&parse("1234\n").unwrap(), "1234\n");
    assert_eq!(diagnostics[0].severity, Severity::Error);
}

#[test]
fn discovery_walks_up_to_the_project_config() {
    let directory = tempfile::tempdir().unwrap();
    let nested = directory.path().join("a/b");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 88\n",
    )
    .unwrap();

    let discovered = Config::find_from(&nested).unwrap();
    assert_eq!(
        discovered.source_path(),
        Some(directory.path().join(".rumk.toml").as_path())
    );
    assert_eq!(discovered.get("MK101.line-length").as_deref(), Some("88"));
}

#[test]
fn effective_configuration_output_is_valid_and_can_be_reloaded() {
    let defaults = Config::default();
    let rendered = defaults.render(false, false);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rendered.toml");
    std::fs::write(&path, rendered).unwrap();

    assert!(Config::from_file(&path).is_ok());
    assert!(defaults.render(false, true).is_empty());
}

#[test]
fn line_length_uses_character_columns_instead_of_utf8_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rumk.toml");
    std::fs::write(&path, "[MK101]\nline-length = 3\n").unwrap();
    let config = Config::from_file(&path).unwrap();
    let rule = config
        .rules
        .iter()
        .find(|rule| rule.id() == "MK101")
        .unwrap();

    assert!(rule.check(&parse("ééé\n").unwrap(), "ééé\n").is_empty());
    assert_eq!(rule.check(&parse("éééé\n").unwrap(), "éééé\n")[0].column, 4);
}

#[test]
fn config_extends_merges_parent_settings_and_detects_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("parent.toml");
    let child = directory.path().join("child.toml");
    std::fs::write(&parent, "[MK101]\nline-length = 80\n").unwrap();
    std::fs::write(
        &child,
        "extends = \"parent.toml\"\n[MK101]\nseverity = \"error\"\n",
    )
    .unwrap();

    let config = Config::from_file(&child).unwrap();
    assert_eq!(config.get("MK101.line-length").as_deref(), Some("80"));
    assert_eq!(config.get("MK101.severity").as_deref(), Some("error"));

    std::fs::write(&parent, "extends = \"child.toml\"\n").unwrap();
    assert!(Config::from_file(&child)
        .err()
        .unwrap()
        .to_string()
        .contains("Failed to parse config file"));
}

#[test]
fn project_configuration_is_queryable_and_resolves_include_paths_from_the_config() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join(".rumk.toml");
    let makefile = directory.path().join("src/Makefile");
    std::fs::create_dir(directory.path().join("src")).unwrap();
    std::fs::write(
        &config_path,
        concat!(
            "[global]\n",
            "include-paths = [\"mk\"]\n",
            "entry-targets = [\"all\"]\n",
            "predefined-variables = { FROM_CLI = \"yes\" }\n",
            "[MK208]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();

    let config = Config::from_file(&config_path).unwrap();
    let options = config.project_options(&makefile);

    assert_eq!(options.working_directory.as_deref(), makefile.parent());
    assert_eq!(options.include_paths, [directory.path().join("mk")]);
    assert_eq!(
        options.predefined_variables.get("FROM_CLI").map(String::as_str),
        Some("yes")
    );
    assert_eq!(
        config.get("global.entry-targets").as_deref(),
        Some("[\"all\"]")
    );
    assert_eq!(
        config.get("global.predefined-variables").as_deref(),
        Some("{\"FROM_CLI\" = \"yes\"}")
    );
    let rendered = config.render(false, false);
    let rendered_path = directory.path().join("rendered.toml");
    std::fs::write(&rendered_path, rendered).unwrap();
    assert!(Config::from_file(&rendered_path).is_ok());
}
