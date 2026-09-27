use serde_json::Value;
use std::process::Command;

fn rumk() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rumk"))
}

fn stdout(command: &mut Command) -> String {
    String::from_utf8(command.output().unwrap().stdout).unwrap()
}

/// A Makefile whose project-level finding (MK201 on line 1) sits above a
/// file-level one (MK101 on line 4).
const MIXED: &str = concat!(
    "clean:\n",
    "\trm -f out\n",
    "\n",
    "FLAGS = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
);

#[test]
fn text_diagnostics_are_listed_in_line_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, MIXED).unwrap();

    let output = stdout(rumk().args(["check", "--no-config", path.to_str().unwrap()]));
    let lines: Vec<_> = output
        .lines()
        .filter(|line| line.contains(": [MK"))
        .map(|line| line.split(':').nth(1).unwrap().parse::<usize>().unwrap())
        .collect();

    assert_eq!(lines, [1, 4], "{output}");
}

#[test]
fn json_diagnostics_are_listed_in_line_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, MIXED).unwrap();

    let output = stdout(rumk().args([
        "check",
        "--no-config",
        "--output-format",
        "json",
        path.to_str().unwrap(),
    ]));
    let diagnostics: Vec<Value> = serde_json::from_str(&output).unwrap();
    let rules: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["rule"].as_str().unwrap())
        .collect();

    assert_eq!(rules, ["MK201", "MK101"], "{output}");
}

#[test]
fn an_included_file_is_reported_after_the_file_that_includes_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, format!("include rules.mk\n{MIXED}")).unwrap();
    std::fs::write(directory.path().join("rules.mk"), "x:\n    echo x\n").unwrap();

    let output = stdout(rumk().args(["check", "--no-config", path.to_str().unwrap()]));
    let places: Vec<_> = output
        .lines()
        .filter(|line| line.contains(": [MK"))
        .map(|line| {
            let mut fields = line.split(':');
            let file = fields.next().unwrap();
            let file = std::path::Path::new(file).file_name().unwrap().to_owned();
            (file, fields.next().unwrap().parse::<usize>().unwrap())
        })
        .collect();

    assert_eq!(
        places,
        [
            ("Makefile".into(), 2),
            ("Makefile".into(), 5),
            ("rules.mk".into(), 2)
        ],
        "{output}"
    );
}

#[test]
fn a_file_with_invalid_utf8_keeps_its_diagnostics_in_line_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let mut content = b"# \xff\n".to_vec();
    content.extend_from_slice(MIXED.as_bytes());
    std::fs::write(&path, content).unwrap();

    let output = stdout(rumk().args(["check", "--no-config", path.to_str().unwrap()]));
    let lines: Vec<_> = output
        .lines()
        .filter(|line| line.contains(": [MK"))
        .map(|line| line.split(':').nth(1).unwrap().parse::<usize>().unwrap())
        .collect();

    assert_eq!(lines, [1, 2, 5], "{output}");
}

#[test]
fn github_annotations_name_the_rule() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all:\n    echo hi\n").unwrap();

    let output = stdout(rumk().args([
        "check",
        "--no-config",
        "--output-format",
        "github",
        path.to_str().unwrap(),
    ]));
    let annotation = output
        .lines()
        .find(|line| line.starts_with("::error "))
        .unwrap_or_else(|| panic!("no error annotation in {output}"));

    assert!(
        annotation.starts_with("::error title=rumk (MK001),file="),
        "{annotation}"
    );
    assert!(
        annotation.ends_with(
            ",line=2,endLine=2,col=1,endColumn=5::Recipe must be indented with tab, not spaces"
        ),
        "{annotation}"
    );
}

/// The `(line, column)` to `(end_line, end_column)` range of every finding
/// for `rule` in `file`, in report order.
fn spans(diagnostics: &[Value], file: &str, rule: &str) -> Vec<[u64; 4]> {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic["rule"] == rule && diagnostic["file"].as_str().unwrap().ends_with(file)
        })
        .map(|diagnostic| {
            ["line", "column", "end_line", "end_column"]
                .map(|key| diagnostic[key].as_u64().unwrap())
        })
        .collect()
}

#[test]
fn every_finding_spans_the_text_it_is_about() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        concat!(
            "include inc.mk\r\n",
            "all:\r\n",
            "    echo hi\r\n",
            "\tcd sub && make -C x\r\n",
            "\trm -rf $BUILD out   \r\n",
            "LONG = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n",
        ),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("inc.mk"),
        format!("FLAGS = {}  \nX=1\n", "é".repeat(120)),
    )
    .unwrap();

    let output = stdout(rumk().args([
        "check",
        "--no-config",
        "--extend-enable",
        "MK105",
        "--output-format",
        "json",
        path.to_str().unwrap(),
    ]));
    let diagnostics: Vec<Value> = serde_json::from_str(&output).unwrap();

    // A token a rule names is spanned exactly: the indentation, the word
    // `make`, the reference.
    assert_eq!(spans(&diagnostics, "Makefile", "MK001"), [[3, 1, 3, 5]]);
    assert_eq!(spans(&diagnostics, "Makefile", "MK203"), [[4, 12, 4, 16]]);
    assert_eq!(spans(&diagnostics, "Makefile", "MK211"), [[5, 9, 5, 15]]);
    assert_eq!(spans(&diagnostics, "inc.mk", "MK105"), [[2, 2, 2, 3]]);
    // Anything else runs to the end of its line, trailing space and line
    // endings left out, in characters, in the root and in an included file.
    assert_eq!(spans(&diagnostics, "Makefile", "MK201"), [[2, 1, 2, 5]]);
    assert_eq!(spans(&diagnostics, "Makefile", "MK101"), [[6, 121, 6, 130]]);
    assert_eq!(spans(&diagnostics, "inc.mk", "MK101"), [[1, 121, 1, 129]]);
    for diagnostic in &diagnostics {
        assert!(
            (
                diagnostic["end_line"].as_u64(),
                diagnostic["end_column"].as_u64()
            ) > (diagnostic["line"].as_u64(), diagnostic["column"].as_u64()),
            "{diagnostic}"
        );
    }
}

#[test]
fn colored_text_has_no_empty_escape_sequences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, MIXED).unwrap();
    let fixable = directory.path().join("fixable.mk");
    std::fs::write(&fixable, ".PHONY: all\nall:\n    echo hi\n").unwrap();

    let output = stdout(rumk().args([
        "check",
        "--no-config",
        "--color",
        "always",
        path.to_str().unwrap(),
        fixable.to_str().unwrap(),
    ]));

    assert!(output.contains(" \x1b[33m[*]\x1b[0m\n"), "{output:?}");
    let empty_style = output.split("\x1b[").any(|sequence| {
        sequence
            .split_once('m')
            .is_some_and(|(code, rest)| code != "0" && rest.is_empty())
    });
    assert!(
        !empty_style,
        "a style opened and closed around nothing: {output:?}"
    );
}

#[test]
fn explain_shows_the_rule_detail_for_any_case() {
    let lower = rumk().args(["explain", "mk201"]).output().unwrap();
    let upper = rumk().args(["explain", "MK201"]).output().unwrap();
    let rule = rumk().args(["rule", "MK201"]).output().unwrap();

    assert!(lower.status.success(), "{lower:?}");
    let text = String::from_utf8(lower.stdout.clone()).unwrap();
    assert!(text.starts_with("MK201 - "), "{text}");
    assert!(text.contains("\nCategory: best-practices\n"), "{text}");
    assert_eq!(lower.stdout, upper.stdout);
    assert_eq!(lower.stdout, rule.stdout);
}

#[test]
fn explain_rejects_an_unknown_rule() {
    let output = rumk().args(["explain", "MK999"]).output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown rule: MK999"));
}

#[test]
fn the_rule_list_says_which_rules_run_by_default_and_how_they_fix() {
    let output = stdout(rumk().arg("rule"));
    let line = |id: &str| {
        output
            .lines()
            .find(|line| line.starts_with(id))
            .unwrap_or_else(|| panic!("{id} missing from {output}"))
            .to_string()
    };

    assert_eq!(
        line("MK001"),
        "MK001  default  fix         Recipe must use tab indentation"
    );
    assert_eq!(
        line("MK102"),
        "MK102  opt-in               Variable naming convention"
    );
    assert_eq!(
        line("MK201"),
        "MK201  default  unsafe fix  Conventional command targets should be .PHONY"
    );
}
