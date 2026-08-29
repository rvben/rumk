use rumk::config::Config;
use serde_json::Value;
use std::process::Command;

fn rumk() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rumk"))
}

#[test]
fn directory_json_matches_rumdl_flat_diagnostic_shape() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("one.mk"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("two.mk"), "clean:\n    true\n").unwrap();

    let output = rumk()
        .args([
            "check",
            directory.path().to_str().unwrap(),
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = document.as_array().unwrap();

    assert!(!output.status.success());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic["file"].is_string()));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic["rule"].is_string()));
}

#[test]
fn auto_discovered_config_errors_are_reported() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    std::fs::write(directory.path().join(".rumk.toml"), "not valid toml = [").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to parse config file"));
}

#[test]
fn fix_on_a_clean_file_is_a_byte_for_byte_noop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let content = ".PHONY: all\nall:\n\ttrue\n";
    std::fs::write(&path, content).unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), content.as_bytes());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Fixed 0 issues"));
}

#[test]
fn fix_reports_only_issues_remaining_after_the_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, ".PHONY: all\nall:\n    true\n").unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: all\nall:\n\ttrue\n"
    );
    assert!(stdout.contains("Fixed 1 issue"));
    assert!(stdout.contains("[MK001]"));
    assert!(stdout.contains("[fixed]"));
}

#[test]
fn directory_checks_enforce_ignored_paths() {
    let directory = tempfile::tempdir().unwrap();
    let ignored = directory.path().join("vendor");
    std::fs::create_dir(&ignored).unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(ignored.join("bad.mk"), "clean:\n    true\n").unwrap();
    let config = directory.path().join("rumk.toml");
    std::fs::write(&config, "[global]\nexclude = [\"vendor/**\"]\n").unwrap();

    let output = rumk()
        .args([
            "check",
            directory.path().to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = document.as_array().unwrap();

    assert!(output.status.success());
    assert!(diagnostics.is_empty());
}

#[test]
fn fmt_fixes_files_and_uses_formatter_exit_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, ".PHONY: all\nall:\n    true\n").unwrap();

    let output = rumk()
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        ".PHONY: all\nall:\n\ttrue\n"
    );
}

#[test]
fn fmt_check_prints_a_diff_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = ".PHONY: all\nall:\n    true\n";
    std::fs::write(&path, original).unwrap();

    let output = rumk()
        .args(["fmt", "--check", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(String::from_utf8_lossy(&output.stdout).contains("-    true"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("+\ttrue"));
}

#[test]
fn warnings_fail_by_default_and_fail_on_can_relax_the_policy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "clean:\n\ttrue\n").unwrap();

    let default_output = rumk()
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let relaxed_output = rumk()
        .args(["check", "--fail-on", "error", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(default_output.status.code(), Some(1));
    assert_eq!(relaxed_output.status.code(), Some(0));
}

#[test]
fn rule_and_config_commands_provide_rumdl_style_introspection() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 88\n",
    )
    .unwrap();

    let rule_output = rumk().args(["rule", "MK101"]).output().unwrap();
    let config_output = rumk()
        .current_dir(directory.path())
        .args(["config", "get", "MK101.line-length"])
        .output()
        .unwrap();
    let file_output = rumk()
        .current_dir(directory.path())
        .args(["config", "file"])
        .output()
        .unwrap();

    assert!(rule_output.status.success());
    assert!(String::from_utf8_lossy(&rule_output.stdout).contains("MK101"));
    assert_eq!(String::from_utf8_lossy(&config_output.stdout).trim(), "88");
    assert!(String::from_utf8_lossy(&file_output.stdout).contains(".rumk.toml"));
}

#[test]
fn check_without_paths_scans_the_current_directory() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Makefile"), "clean:\n\ttrue\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Makefile:1:1"));
}

#[test]
fn discovery_respects_gitignore_with_a_cli_escape_hatch() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".git")).unwrap();
    std::fs::write(directory.path().join(".gitignore"), "ignored.mk\n").unwrap();
    std::fs::write(directory.path().join("ignored.mk"), "all:\n    true\n").unwrap();

    let respected = rumk()
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .output()
        .unwrap();
    let overridden = rumk()
        .current_dir(directory.path())
        .args([
            "check",
            "--respect-gitignore=false",
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(respected.status.code(), Some(0));
    assert!(serde_json::from_slice::<Value>(&respected.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(overridden.status.code(), Some(1));
}

#[test]
fn cli_rule_selection_and_fix_policy_match_rumdl_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = ".PHONY: all\nall:\n    true\n";
    std::fs::write(&path, original).unwrap();

    let disabled = rumk()
        .args(["check", "--disable", "MK001", path.to_str().unwrap()])
        .output()
        .unwrap();
    let unfixable = rumk()
        .args(["fmt", "--unfixable", "MK001", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(disabled.status.code(), Some(0));
    assert_eq!(unfixable.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn init_creates_a_valid_config_and_refuses_to_overwrite_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(".rumk.toml");

    let created = rumk()
        .args(["init", "--output", path.to_str().unwrap()])
        .output()
        .unwrap();
    let repeated = rumk()
        .args(["init", "--output", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(created.status.code(), Some(0));
    assert!(Config::from_file(&path).is_ok());
    assert_eq!(repeated.status.code(), Some(2));
}

#[test]
fn explicit_files_bypass_include_filters_but_not_excludes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("custom.mk");
    std::fs::write(&path, "all:\n    true\n").unwrap();

    let included = rumk()
        .current_dir(directory.path())
        .args(["check", "--include", "somewhere-else/**", "custom.mk"])
        .output()
        .unwrap();
    let excluded = rumk()
        .current_dir(directory.path())
        .args(["check", "--exclude", "custom.mk", "custom.mk"])
        .output()
        .unwrap();

    assert_eq!(included.status.code(), Some(1));
    assert_eq!(excluded.status.code(), Some(0));
}
