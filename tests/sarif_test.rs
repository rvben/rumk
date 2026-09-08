use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[test]
fn sarif_cli_matches_json_on_the_comparison_corpus() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comparison");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(corpus.join(case["name"].as_str().unwrap())).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }
        std::fs::write(
            temp.path().join(".rumk.toml"),
            format!(
                "[global]\nenable = {}\n[MK104]\nmax-lines = 2\n[MK215]\nrequired = ['test']\n",
                case["enable"]
            ),
        )
        .unwrap();
        let run = |format: &str| {
            Command::new(env!("CARGO_BIN_EXE_rumk"))
                .current_dir(temp.path())
                .args([
                    "check",
                    case["entry"].as_str().unwrap(),
                    "--output-format",
                    format,
                ])
                .output()
                .unwrap()
        };
        let json = run("json");
        let sarif = run("sarif");
        assert_eq!(sarif.status.code(), json.status.code(), "{case}");
        assert!(sarif.stderr.is_empty());
        assert_eq!(sarif.stdout, run("sarif").stdout);
        let json: Value = serde_json::from_slice(&json.stdout).unwrap();
        let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), json.as_array().unwrap().len(), "{case}");
        for (result, diagnostic) in results.iter().zip(json.as_array().unwrap()) {
            assert_eq!(result["ruleId"], diagnostic["rule"]);
            assert_eq!(result["message"]["text"], diagnostic["message"]);
            let location = &result["locations"][0]["physicalLocation"];
            assert_eq!(location["artifactLocation"]["uri"], diagnostic["file"]);
            assert_eq!(location["region"]["startLine"], diagnostic["line"]);
            assert_eq!(location["region"]["startColumn"], diagnostic["column"]);
        }
    }
}

#[test]
fn sarif_supports_unsaved_buffers_and_special_filenames() {
    use std::io::Write;
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(temp.path())
        .args([
            "check",
            "-",
            "--no-config",
            "--stdin-filename",
            "a #é.mk",
            "--enable",
            "MK105",
            "--output-format",
            "sarif",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"CC=cc\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &value["runs"][0]["results"][0];
    assert_eq!(
        result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "a%20%23%C3%A9.mk"
    );
    assert_eq!(result["properties"]["fixApplicability"], "safe");
    assert!(!temp.path().join("a #é.mk").exists());
}

fn stdin_report(input: &str, rule: &str, extra: &[&str]) -> Value {
    use std::io::Write;
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(temp.path())
        .args([
            "check",
            "-",
            "--no-config",
            "--enable",
            rule,
            "--output-format",
            "sarif",
        ])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A small consumer, deliberately independent of Rumk's fix engine. SARIF text
/// coordinates count Unicode scalar values, excluding an encoding BOM.
fn apply_sarif(input: &str, result: &Value) -> String {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let bom_len = input.len() - text.len();
    let offset = |line: &Value, column: &Value| {
        let start = if line == 1 {
            0
        } else {
            text.match_indices('\n')
                .nth(line.as_u64().unwrap() as usize - 2)
                .unwrap()
                .0
                + 1
        };
        start
            + text[start..]
                .char_indices()
                .nth(column.as_u64().unwrap() as usize - 1)
                .map_or(text.len() - start, |(index, _)| index)
            + bom_len
    };
    let changes = result["fixes"][0]["artifactChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    let mut edits: Vec<_> = changes[0]["replacements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edit| {
            let region = &edit["deletedRegion"];
            (
                offset(&region["startLine"], &region["startColumn"]),
                offset(&region["endLine"], &region["endColumn"]),
                edit["insertedContent"]["text"].as_str().unwrap(),
            )
        })
        .collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut fixed = input.to_string();
    for (start, end, replacement) in edits {
        fixed.replace_range(start..end, replacement);
    }
    fixed
}

#[test]
fn sarif_edits_roundtrip_unicode_bom_crlf_insertions_and_multiple_edits() {
    for (input, rule, extra, expected) in [
        ("\u{feff}É=é😀\r\n", "MK105", vec![], "\u{feff}É = é😀\r\n"),
        ("é:;@@echo 😀", "MK218", vec![], "é:;@echo 😀"),
        (
            "all:\n\tmake -C one && echo 'é😀' && make -C two\n",
            "MK203",
            vec!["--unsafe-fixes"],
            "all:\n\t$(MAKE) -C one && echo 'é😀' && $(MAKE) -C two\n",
        ),
        (
            "\u{feff}all:\r\n\tmake -C one && echo 'é😀' && make -C two\r\n",
            "MK203",
            vec!["--unsafe-fixes"],
            "\u{feff}all:\r\n\t$(MAKE) -C one && echo 'é😀' && $(MAKE) -C two\r\n",
        ),
    ] {
        let value = stdin_report(input, rule, &extra);
        let results = value["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(apply_sarif(input, &results[0]), expected);
    }
    let input = "\u{feff}clean:\r\n\t@echo clean\r\n";
    let value = stdin_report(input, "MK201", &["--unsafe-fixes"]);
    let fixed = apply_sarif(input, &value["runs"][0]["results"][0]);
    assert!(fixed.starts_with('\u{feff}'));
    assert!(fixed.contains(".PHONY: clean\r\n"));
    assert!(fixed.contains("clean:\r\n\t@echo clean\r\n"));
}

#[test]
fn sarif_omits_withheld_fixes_but_retains_safety_metadata() {
    for (input, rule, extra, safety) in [
        ("all:\n\tmake -C sub\n", "MK203", vec![], "unsafe"),
        ("CC=cc\n", "MK105", vec!["--unfixable", "MK105"], "safe"),
    ] {
        let value = stdin_report(input, rule, &extra);
        let result = &value["runs"][0]["results"][0];
        assert!(result.get("fixes").is_none());
        if safety == "unsafe" {
            assert_eq!(result["properties"]["fixApplicability"], safety);
        } else {
            assert!(result["properties"].get("fixApplicability").is_none());
        }
        assert_eq!(result["properties"]["fixable"], false);
    }
}

#[test]
fn sarif_edits_for_included_files_use_the_included_buffer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "include vars.mk\nall:;\n").unwrap();
    let input = "# 😀\nCC=cc\n";
    std::fs::write(dir.path().join("vars.mk"), input).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(dir.path())
        .args([
            "check",
            ".",
            "--no-config",
            "--enable",
            "MK105",
            "--output-format",
            "sarif",
        ])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &value["runs"][0]["results"][0];
    assert_eq!(
        result["fixes"][0]["artifactChanges"][0]["artifactLocation"]["uri"],
        "vars.mk"
    );
    assert_eq!(apply_sarif(input, result), "# 😀\nCC = cc\n");
}
