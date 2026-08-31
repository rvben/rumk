use rumk::{config::Config, rules::RULE_IDS};
use serde_json::Value;
use std::path::Path;

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
