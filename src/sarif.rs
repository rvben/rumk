//! SARIF 2.1.0 diagnostics for code-scanning consumers.

use rumk::diagnostic::{Diagnostic, Severity};
use rumk::rules;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// A diagnostic paired with its displayed source path, including include files.
pub struct Finding<'a> {
    pub path: &'a str,
    pub diagnostic: &'a Diagnostic,
    /// The exact checked text, without a leading BOM. None for foreign sources.
    pub content: Option<&'a str>,
}

/// Produce deterministic SARIF. Columns count Unicode code points, just as
/// Rumk's diagnostics do. No timestamps or machine-dependent fingerprints enter
/// result identity; upload-sarif can derive fingerprints from checked-out files.
pub fn report(
    findings: &[Finding<'_>],
    working_directory: &Path,
    execution_successful: bool,
) -> Value {
    let all_rules = rules::get_all_rules();
    let indices: BTreeMap<_, _> = all_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| (rule.id(), index))
        .collect();
    let descriptors: Vec<_> = all_rules
        .iter()
        .map(|rule| {
            json!({
                "id": rule.id(),
                "shortDescription": {"text": rule.name()},
                "fullDescription": {"text": rule.description()},
                "helpUri": rules::documentation_url(rule.id()),
                "help": {"text": rule.description()},
                "properties": {"tags": [rule.category().as_str()]}
            })
        })
        .collect();
    let results: Vec<_> = findings
        .iter()
        .map(|finding| {
            let diagnostic = finding.diagnostic;
            let mut region = json!({
                "startLine": diagnostic.line.max(1),
                "startColumn": diagnostic.column.max(1)
            });
            if let Some(end_line) = diagnostic
                .end_line
                .filter(|line| *line >= diagnostic.line.max(1))
            {
                region["endLine"] = json!(end_line);
            }
            if let Some(end_column) = diagnostic.end_column.filter(|column| {
                *column > 0
                    && (diagnostic.end_line.unwrap_or(diagnostic.line) > diagnostic.line
                        || *column > diagnostic.column)
            }) {
                region["endColumn"] = json!(end_column);
            }
            let mut result = json!({
                "ruleId": diagnostic.rule_id,
                "level": match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "note",
                },
                "message": {"text": diagnostic.message},
                "locations": [{"physicalLocation": {
                    "artifactLocation": {"uri": artifact_uri(finding.path)},
                    "region": region
                }}],
                "properties": {"fixable": diagnostic.fixable}
            });
            if let Some(index) = indices.get(diagnostic.rule_id.as_str()) {
                result["ruleIndex"] = json!(index);
            }
            if let Some(fix) = &diagnostic.fix {
                result["properties"]["fixApplicability"] = json!(fix.applicability.as_str());
            }
            if let Some(fix) = exported_fix(finding) {
                result["fixes"] = json!([fix]);
            }
            result
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "rumk",
                "semanticVersion": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/rvben/rumk",
                "rules": descriptors
            }},
            "columnKind": "unicodeCodePoints",
            "invocations": [{
                "executionSuccessful": execution_successful,
                "workingDirectory": {"uri": artifact_uri(&working_directory.to_string_lossy())}
            }],
            "results": results
        }]
    })
}

/// Export only fixes permitted by the run's safety and fixability policy. All
/// edits must validate together against the checked buffer, never reread disk.
fn exported_fix(finding: &Finding<'_>) -> Option<Value> {
    let diagnostic = finding.diagnostic;
    if !diagnostic.fixable {
        return None;
    }
    let content = finding.content?;
    let fix = diagnostic.fix.as_ref()?;
    if !rumk::fix::fix_is_valid(content, fix) {
        return None;
    }
    let replacements: Option<Vec<_>> = fix
        .edits
        .iter()
        .map(|edit| {
            let (start, end) = rumk::fix::edit_byte_range(content, edit)?;
            let (start_line, start_column) = position(content, start);
            let (end_line, end_column) = position(content, end);
            Some(json!({
                "deletedRegion": {
                    "startLine": start_line, "startColumn": start_column,
                    "endLine": end_line, "endColumn": end_column
                },
                "insertedContent": {"text": edit.replacement}
            }))
        })
        .collect();
    Some(json!({
        "description": {"text": fix.description},
        "artifactChanges": [{
            "artifactLocation": {"uri": artifact_uri(finding.path)},
            "replacements": replacements?
        }]
    }))
}

// SARIF text regions do not count an encoding BOM (section 3.57.1).
fn position(content: &str, offset: usize) -> (usize, usize) {
    let before = &content[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

/// URI-encode filesystem bytes, including '%' and '#' so paths cannot become
/// URI escapes or fragments. Windows separators are normalized only on Windows;
/// a backslash can be an actual filename character on Unix.
fn artifact_uri(path: &str) -> String {
    let normalized = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.into()
    };
    let mut encoded = String::new();
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    if cfg!(windows) && normalized.starts_with("//") {
        format!("file:{encoded}")
    } else if cfg!(windows) && Path::new(path).is_absolute() {
        // The drive colon is URI syntax only in an absolute Windows file URI.
        format!("file:///{}", encoded.replacen("%3A", ":", 1))
    } else if normalized.starts_with('/') {
        format!("file://{encoded}")
    } else {
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumk::diagnostic::{Edit, Fix};

    #[test]
    fn sarif_rejects_invalid_or_overlapping_fix_sets_atomically() {
        for fix in [
            Fix::new("empty"),
            Fix::new("invalid line").add_edit(Edit::new(9, 1, 9, 2, "x")),
            Fix::new("overlap")
                .add_edit(Edit::new(1, 1, 1, 3, "x"))
                .add_edit(Edit::new(1, 2, 1, 4, "y")),
        ] {
            let diagnostic = Diagnostic::new("MK105", Severity::Warning, "Fix", 1, 1).with_fix(fix);
            let finding = Finding {
                path: "Makefile",
                diagnostic: &diagnostic,
                content: Some("CC=cc\n"),
            };
            assert!(exported_fix(&finding).is_none());
        }
        let diagnostic = Diagnostic::new("MK105", Severity::Warning, "Fix", 1, 1)
            .with_fix(Fix::new("valid").add_edit(Edit::new(1, 3, 1, 4, " = ")));
        let finding = Finding {
            path: "other.mk",
            diagnostic: &diagnostic,
            content: None,
        };
        assert!(exported_fix(&finding).is_none());
    }

    #[test]
    fn sarif_encodes_paths_and_links_stable_rule_descriptors() {
        let diagnostic = Diagnostic::new("MK105", Severity::Info, "Spacing near 😀", 2, 8);
        let findings = [Finding {
            path: "a space/é#100%.mk",
            diagnostic: &diagnostic,
            content: None,
        }];
        let cwd = std::env::current_dir().unwrap();
        let value = report(&findings, &cwd, true);
        assert_eq!(value, report(&findings, &cwd, true));
        assert_eq!(value["version"], "2.1.0");
        let run = &value["runs"][0];
        assert_eq!(run["columnKind"], "unicodeCodePoints");
        let result = &run["results"][0];
        assert_eq!(result["level"], "note");
        let location = &result["locations"][0]["physicalLocation"];
        assert_eq!(
            location["artifactLocation"]["uri"],
            "a%20space/%C3%A9%23100%25.mk"
        );
        assert_eq!(location["region"]["startColumn"], 8);
        assert!(location["region"].get("endColumn").is_none());
        let index = result["ruleIndex"].as_u64().unwrap() as usize;
        assert_eq!(run["tool"]["driver"]["rules"][index]["id"], "MK105");
        assert!(run["tool"]["driver"]["rules"][index]["helpUri"]
            .as_str()
            .unwrap()
            .ends_with("/mk105.md"));
        let empty = report(&[], &cwd, false);
        assert_eq!(empty["runs"][0]["results"], serde_json::json!([]));
        assert_eq!(
            empty["runs"][0]["invocations"][0]["executionSuccessful"],
            false
        );
        assert_eq!(empty["runs"][0]["tool"], run["tool"]);
    }
}
