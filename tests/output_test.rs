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
        annotation.ends_with(",line=2,col=1::Recipe must be indented with tab, not spaces"),
        "{annotation}"
    );
}
