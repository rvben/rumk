use serde_json::Value;
use std::process::Command;

fn run(directory: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn coverage_uses_configuration_without_requiring_rule_enablement() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("settings")).unwrap();
    std::fs::write(
        dir.path().join("settings/inputs.mk"),
        "INPUT = missing.txt\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Makefile"),
        "include inputs.mk\nprobe: $(INPUT)\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("rumk.toml"),
        "[global]\ninclude-paths=['settings']\n",
    )
    .unwrap();
    let (status, report) = run(dir.path(), &["coverage"]);
    assert_eq!(status, 0);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["roots"][0]["rule_enabled"], false);
    assert_eq!(report["roots"][0]["coverage"]["outcomes"]["missing"], 1);
    assert_eq!(report, run(dir.path(), &["coverage"]).1);
    let (_, isolated) = run(dir.path(), &["--no-config", "coverage"]);
    assert_eq!(
        isolated["roots"][0]["coverage"]["root_blockers"]["unresolved_include"],
        1
    );
}

#[test]
fn coverage_exposes_dynamic_blockers_without_running_shell_commands() {
    let dir = tempfile::tempdir().unwrap();
    let source = "INPUT := $(shell touch executed)\nprobe: $(INPUT)\n";
    std::fs::write(dir.path().join("Makefile"), source).unwrap();
    let (status, report) = run(dir.path(), &["--no-config", "coverage"]);
    assert_eq!(status, 0);
    assert_eq!(
        report["roots"][0]["coverage"]["root_blockers"]["shell_function"],
        1
    );
    assert!(!dir.path().join("executed").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Makefile")).unwrap(),
        source
    );
}

#[test]
fn coverage_keeps_good_roots_when_an_input_is_unreadable_or_invalid() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "probe: missing.txt\n").unwrap();
    std::fs::write(dir.path().join("bad.mk"), [255]).unwrap();
    let (status, report) = run(
        dir.path(),
        &["--no-config", "coverage", "absent.mk", "Makefile", "bad.mk"],
    );
    assert_eq!(status, 1);
    assert!(report["roots"][0]["error"].is_string());
    assert_eq!(report["roots"][1]["coverage"]["outcomes"]["missing"], 1);
    assert_eq!(report["roots"][2]["error"], "Makefile is not valid UTF-8");
}
