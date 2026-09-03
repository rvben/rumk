use rumk::{config::Config, rules::RULE_IDS};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

#[test]
fn configuration_schema_is_valid_json_and_covers_every_rule() {
    let schema: Value = serde_json::from_str(include_str!("../rumk.schema.json")).unwrap();
    let properties = schema["properties"].as_object().unwrap();

    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    for rule_id in RULE_IDS {
        assert!(
            properties.contains_key(*rule_id),
            "configuration schema is missing {rule_id}"
        );
    }
}

#[test]
fn schema_fixture_is_accepted_by_rumk() {
    Config::from_file(Path::new("tests/fixtures/config/schema-valid.toml")).unwrap();
}

#[test]
fn configuration_schema_rule_id_pattern_covers_exactly_the_known_rules() {
    let schema: Value = serde_json::from_str(include_str!("../rumk.schema.json")).unwrap();
    let pattern = schema["$defs"]["RuleId"]["pattern"].as_str().unwrap();

    for rule_id in RULE_IDS {
        assert!(
            pattern.contains(&rule_id[2..]),
            "schema pattern is missing {rule_id}"
        );
    }
}

/// A configuration key Rumk reads but the schema does not describe is invisible
/// to every editor validating against it, and a key the schema promises but
/// Rumk rejects makes a valid-looking file fail to load. The effective
/// configuration lists every global key Rumk knows, so the two can be compared.
#[test]
fn configuration_schema_describes_every_global_key() {
    let schema: Value = serde_json::from_str(include_str!("../rumk.schema.json")).unwrap();
    let described = schema["$defs"]["GlobalConfig"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .args(["--no-config", "config", "--defaults"])
        .output()
        .unwrap();
    let rendered = String::from_utf8(output.stdout).unwrap();
    let known = rendered
        .lines()
        .skip_while(|line| line.trim() != "[global]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| line.split_once(" = "))
        .map(|(key, _)| key.trim().to_string())
        .collect::<BTreeSet<_>>();

    assert!(
        !known.is_empty(),
        "the effective configuration listed no global keys"
    );
    assert_eq!(known, described);
}
