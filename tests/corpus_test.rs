use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn corpus(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus")
        .join(case)
}

fn assert_rumk_clean(directory: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(directory)
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        diagnostics,
        Value::Array(Vec::new()),
        "corpus case {} produced diagnostics:\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.status.success(),
        "rumk rejected corpus case {}:\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_gnu_make_accepts(directory: &Path) {
    let output = match Command::new("make")
        .current_dir(directory)
        .args(["--no-builtin-rules", "--dry-run", "verify"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };
    assert!(
        output.status.success(),
        "GNU Make rejected corpus case {}:\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn production_style_corpus_is_clean_and_gnu_compatible() {
    for case in ["c_library", "conditional", "go_service", "monorepo"] {
        let directory = corpus(case);
        assert_rumk_clean(&directory);
        assert_gnu_make_accepts(&directory);
    }
}
