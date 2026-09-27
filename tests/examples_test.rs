use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

fn reported_rules(path: &str) -> BTreeSet<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .args(["check", "--no-config", "--output-format", "json", path])
        .output()
        .unwrap();
    let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic["rule"].as_str().unwrap().to_string())
        .collect()
}

/// Each `# MKnnn:` comment in the example promises a finding, so the example
/// stays a truthful tour of the default rules.
#[test]
fn the_bad_example_reports_exactly_the_rules_its_comments_name() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/bad.mk");
    let promised: BTreeSet<_> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("# "))
        .filter_map(|line| line.split_once(':'))
        .map(|(code, _)| code)
        .filter(|code| code.len() == 5 && code.starts_with("MK"))
        .map(str::to_string)
        .collect();

    assert!(!promised.is_empty());
    assert_eq!(reported_rules(path), promised);
}

#[test]
fn the_good_example_is_clean() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/good.mk");

    assert_eq!(reported_rules(path), BTreeSet::new());
}
