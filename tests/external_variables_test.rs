use rumk::config::Config;
use rumk::project::Project;

fn check(configuration: &str, source: &str) -> (Project, Vec<rumk::diagnostic::Diagnostic>) {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("rumk.toml");
    std::fs::write(&config_path, configuration).unwrap();
    let config = Config::from_file(&config_path).unwrap();
    let path = dir.path().join("Makefile");
    let project =
        Project::load_with_root_content(&path, source.into(), &config.project_options(&path))
            .unwrap();
    let diagnostics = config
        .rules
        .iter()
        .find(|rule| rule.id() == "MK208")
        .unwrap()
        .check_project(&project);
    (project, diagnostics)
}

#[test]
fn external_names_are_exact_case_sensitive_and_do_not_supply_values() {
    let (project, diagnostics) = check(
        "[MK208]\nenabled = true\nexternal-variables = ['TESTS', 'TESTS']\n",
        "FLAGS = $(TESTS) $(TSET) $(tests)\n",
    );
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().any(|d| d.message.contains("'TSET'")));
    assert!(diagnostics.iter().any(|d| d.message.contains("'tests'")));
    assert!(project.evaluation().expand("$(TESTS)").value.is_none());
}

#[test]
fn external_names_do_not_hide_local_read_before_definition() {
    let (_, diagnostics) = check(
        "[MK208]\nenabled = true\nexternal-variables = ['TESTS']\n",
        "FLAGS := $(TESTS)\nTESTS = enabled\n",
    );
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("TESTS"));
    assert_eq!(diagnostics[0].line, 1);
}

#[test]
fn external_names_round_trip_and_legacy_options_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rumk.toml");
    for text in [
        "[MK208]\nenabled=true\nexternal-variables=['TESTS', 'FEATURE-X']\n",
        "[rules.MK208]\nenabled=true\n[rules.MK208.options]\nexternal_variables=['TESTS', 'FEATURE-X']\n",
    ] {
        std::fs::write(&path, text).unwrap();
        let config = Config::from_file(&path).unwrap();
        assert!(config.get("MK208.external-variables").unwrap().contains("TESTS"));
        std::fs::write(&path, config.render(false, false)).unwrap();
        let restored = Config::from_file(&path).unwrap();
        assert_eq!(config.get("MK208.external-variables"), restored.get("MK208.external-variables"));
    }
    for value in [
        "true",
        "[3]",
        "['']",
        "['two words']",
        "['$(TESTS)']",
        "['TEST*']",
        "['X=1']",
    ] {
        std::fs::write(
            &path,
            format!("[MK208]\nenabled=true\nexternal-variables={value}\n"),
        )
        .unwrap();
        assert!(Config::from_file(&path).is_err(), "{value}");
    }
}
