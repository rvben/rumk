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

    assert_eq!(ids, ["MK001", "MK002", "MK101", "MK201"]);
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
    assert_eq!(ids, ["MK001", "MK002", "MK101"]);
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
