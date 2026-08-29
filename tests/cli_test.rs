use serde_json::Value;
use std::process::Command;

fn rumk() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rumk"))
}

#[test]
fn directory_json_is_one_document_with_file_paths() {
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
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let files = document["files"].as_array().unwrap();

    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|file| file["path"].is_string()));
    assert!(files.iter().all(|file| file["diagnostics"].is_array()));
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

    assert!(!output.status.success());
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
    assert!(!stdout.contains("[MK001]"));
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
    std::fs::write(&config, "[ignore]\npaths = [\"vendor/**\"]\n").unwrap();

    let output = rumk()
        .args([
            "check",
            directory.path().to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let files = document["files"].as_array().unwrap();

    assert!(output.status.success());
    assert_eq!(files.len(), 1);
    assert!(files[0]["path"].as_str().unwrap().ends_with("Makefile"));
}
