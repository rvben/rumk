use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}
fn diagnostics(root: &Path, args: &[&str]) -> Vec<Value> {
    let mut arguments = vec!["check", "--output-format", "json"];
    arguments.extend_from_slice(args);
    let output = run(root, &arguments);
    assert_ne!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn nested_configuration_is_consistent_for_tree_file_and_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".rumk.toml",
        "[global]\nenable=['MK101']\n[MK101]\nline-length=8\n",
    );
    write(root, "Makefile", "VARIABLE = long value\n");
    write(
        root,
        "nested/pyproject.toml",
        "[tool.rumk.global]\nenable=['MK101']\n[tool.rumk.MK101]\nline-length=100\n",
    );
    write(root, "nested/Makefile", "VARIABLE = long value\n");
    assert_eq!(diagnostics(root, &["."]).len(), 1);
    assert!(diagnostics(root, &["nested/Makefile"]).is_empty());
    assert!(diagnostics(&root.join("nested"), &["Makefile"]).is_empty());
    assert!(diagnostics(
        root.parent().unwrap(),
        &[root.join("nested/Makefile").to_str().unwrap()]
    )
    .is_empty());
    assert_eq!(diagnostics(root, &[".", "--config", ".rumk.toml"]).len(), 2);
    assert!(diagnostics(root, &[".", "--disable", "MK101"]).is_empty());
    assert!(diagnostics(root, &[".", "--isolated"])
        .iter()
        .all(|d| d["rule"] != "MK101"));
}

#[test]
fn nested_filters_and_gitignore_are_resolved_from_their_config_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join(".git")).unwrap();
    write(root, ".rumk.toml", "[global]\nenable=['MK001']\n");
    write(root, ".gitignore", "ignored.mk\n");
    write(
        root,
        "nested/pyproject.toml",
        "[tool.rumk.global]\nenable=['MK001']\nrespect-gitignore=false\nexclude=['excluded.mk']\n",
    );
    for name in [
        "ignored.mk",
        "nested/ignored.mk",
        "nested/excluded.mk",
        "nested/Makefile",
    ] {
        write(root, name, "all:\n    echo hi\n");
    }
    assert_eq!(diagnostics(root, &["."]).len(), 2);
    assert_eq!(diagnostics(root, &["nested"]).len(), 2);
    assert_eq!(
        diagnostics(root, &["nested", "--respect-gitignore"]).len(),
        1
    );
    assert_eq!(diagnostics(root, &["nested", "--no-exclude"]).len(), 3);
}

#[test]
fn formatting_and_project_rules_use_nested_settings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".rumk.toml", "[global]\nenable=[]\n");
    write(
        root,
        "nested/pyproject.toml",
        "[tool.rumk.global]\nenable=['MK001','MK201']\n",
    );
    write(root, "nested/Makefile", "test:\n    echo hi\n");
    assert!(diagnostics(root, &["."])
        .iter()
        .any(|d| d["rule"] == "MK201"));
    let formatted = run(root, &["fmt", "."]);
    assert!(formatted.status.success(), "{:?}", formatted);
    assert_eq!(
        std::fs::read_to_string(root.join("nested/Makefile")).unwrap(),
        "test:\n\techo hi\n"
    );
    // Formatting must never apply the unsafe phony fix.
    assert!(diagnostics(root, &["."])
        .iter()
        .any(|d| d["rule"] == "MK201"));
}

#[test]
fn configuration_trace_shows_inheritance_and_warns_once_per_selected_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "base.toml", "[global]\nenable=[]\n");
    write(root, ".rumk.toml", "extends='base.toml'\n");
    write(root, "pyproject.toml", "[tool.rumk]\n");
    write(root, "a/Makefile", "X = 1\n");
    write(root, "b/Makefile", "X = 1\n");
    let output = run(root, &["config", "file", "a/Makefile", "--explain"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains(".rumk.toml")
            && text.contains("extends:")
            && text.contains("base.toml")
            && text.contains("shadowed:")
            && text.contains("pyproject.toml"),
        "{text}"
    );
    let output = run(root, &["check", ".", "--output-format", "json"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("[config warning]")
            .count(),
        1
    );
    let output = run(root, &["check", ".", "--silent"]);
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    write(root, "pyproject.toml", "[project]\nname='unrelated'\n");
    assert!(run(root, &["config", "file"]).stderr.is_empty());
}

#[test]
fn init_pyproject_preserves_existing_content_and_refuses_duplicate_or_invalid_tables() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original = "# Keep this comment\r\n[project]\r\nname = 'example' # and this\r\n[tool.ruff]\r\nline-length = 88\r\n";
    write(root, "pyproject.toml", original);
    let output = run(root, &["init", "--pyproject"]);
    assert!(output.status.success(), "{:?}", output);
    let changed = std::fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(changed.starts_with(original));
    assert!(changed.contains("[tool.rumk.global]\r\n"));
    assert!(rumk::config::Config::from_file(&root.join("pyproject.toml")).is_ok());
    assert_eq!(run(root, &["init", "--pyproject"]).status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(root.join("pyproject.toml")).unwrap(),
        changed
    );
    for original in ["[invalid TOML", "[tool]\nrumk = false\n"] {
        write(root, "pyproject.toml", original);
        assert_eq!(run(root, &["init", "--pyproject"]).status.code(), Some(2));
        assert_eq!(
            std::fs::read_to_string(root.join("pyproject.toml")).unwrap(),
            original
        );
    }
}

#[test]
fn invalid_inherited_setting_reports_its_file_and_namespaced_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "pyproject.toml",
        "[tool.rumk.MK101]\nline-length='bad'\n",
    );
    write(root, "nested/.rumk.toml", "extends='../pyproject.toml'\n");
    let output = run(root, &["config", "file", "nested/Makefile"]);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("pyproject.toml") && error.contains("tool.rumk.MK101.line-length"),
        "{error}"
    );
}

#[test]
fn init_pyproject_handles_inline_tool_tables_without_reformatting_other_settings() {
    let dir = tempfile::tempdir().unwrap();
    for original in [
        "tool = { ruff = { line-length = 88 } } # keep\n[project]\nname='example'\n",
        "tool = {}\n",
    ] {
        write(dir.path(), "pyproject.toml", original);
        let output = run(dir.path(), &["init", "--pyproject"]);
        assert!(output.status.success(), "{:?}", output);
        let changed = std::fs::read_to_string(dir.path().join("pyproject.toml")).unwrap();
        assert!(changed.starts_with(original.split('}').next().unwrap()));
        if original.contains("# keep") {
            assert!(changed.ends_with("# keep\n[project]\nname='example'\n"));
        }
        assert!(rumk::config::Config::from_file(&dir.path().join("pyproject.toml")).is_ok());
    }
}

#[test]
fn invalid_nested_configuration_prevents_partial_formatting() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".rumk.toml", "[global]\nenable=['MK001']\n");
    let original = "all:\n    echo hi\n";
    write(dir.path(), "Makefile", original);
    write(
        dir.path(),
        "nested/pyproject.toml",
        "[tool.rumk.MK101]\nline-length='bad'\n",
    );
    write(dir.path(), "nested/Makefile", original);
    assert_eq!(run(dir.path(), &["fmt", "."]).status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Makefile")).unwrap(),
        original
    );
}

#[test]
fn coverage_resolves_nested_include_paths_from_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".rumk.toml", "[global]\nenable=[]\n");
    write(
        dir.path(),
        "nested/pyproject.toml",
        "[tool.rumk.global]\ninclude-paths=['settings']\n",
    );
    write(
        dir.path(),
        "nested/settings/inputs.mk",
        "INPUT = missing.txt\n",
    );
    write(
        dir.path(),
        "nested/Makefile",
        "include inputs.mk\nprobe: $(INPUT)\n",
    );
    let output = run(dir.path(), &["coverage", "nested/Makefile"]);
    assert!(output.status.success(), "{:?}", output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["roots"][0]["coverage"]["outcomes"]["missing"], 1);
}

#[cfg(unix)]
#[test]
fn init_pyproject_preserves_symlinks_permissions_and_dangling_targets() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("project.toml");
    write(dir.path(), "project.toml", "[project]\nname='example'\n");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&target, dir.path().join("pyproject.toml")).unwrap();
    assert!(run(dir.path(), &["init", "--pyproject"]).status.success());
    assert!(std::fs::symlink_metadata(dir.path().join("pyproject.toml"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(std::fs::read_to_string(&target)
        .unwrap()
        .contains("[tool.rumk.global]"));
    std::fs::remove_file(&target).unwrap();
    assert_eq!(
        run(dir.path(), &["init", "--pyproject"]).status.code(),
        Some(2)
    );
    assert!(!target.exists());
}
