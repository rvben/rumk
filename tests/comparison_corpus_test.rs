//! The same authored fixtures drive the offline test suite and external-tool comparison.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

#[test]
fn comparison_corpus_matches_exact_rules_locations_and_exit_codes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comparison");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).unwrap())
            .unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let directory = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(root.join(name)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), directory.path().join(entry.file_name())).unwrap();
        }
        std::fs::write(
            directory.path().join(".rumk.toml"),
            format!(
                "[global]\ndialect = {}\nenable = {}\n[MK104]\nmax-lines = 2\n[MK215]\nrequired = ['test']\n",
                case["dialect"], case["enable"]
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
            .current_dir(directory.path())
            .args(["check", "--output-format", "json", "Makefile"])
            .output()
            .unwrap();
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
        let actual: Vec<Value> = diagnostics.as_array().unwrap().iter().map(|d| {
            json!({"rule": d["rule"], "line": d["line"], "file": d["file"].as_str().unwrap().replace('\\', "/")})
        }).collect();
        assert_eq!(json!(actual), case["expected"], "{name}");
        assert_eq!(
            output.status.code(),
            Some(if actual.is_empty() { 0 } else { 1 }),
            "{name}"
        );
    }
}
