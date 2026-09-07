use std::collections::BTreeSet;

use rumk::config::Config;
use rumk::lint::{self, LintContext};
use rumk::project::Project;

#[test]
fn loaded_project_uses_its_snapshot_and_fix_passes_reload_includes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    let content = "include shared.mk\nall: first\n\t@:\nclean:\n\t@:\n";
    std::fs::write(&root, content).unwrap();
    std::fs::write(&shared, "all:: second\n").unwrap();
    let mut config = Config::default();
    config.apply_fix_overrides(None, None, Some(true)).unwrap();
    let covered = BTreeSet::new();
    let context = LintContext {
        config: &config,
        path: &root,
        project_root: true,
        contextual: true,
        layout_only: false,
        covered_files: &covered,
    };
    let project = Project::load(&root, &config.project_options(&root)).unwrap();
    let diagnostics = lint::lint_project(&project, &context).unwrap();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.rule_id == "MK004"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.fixable));

    std::fs::write(&shared, "all: second\n").unwrap();
    let cached = lint::lint_project(&project, &context).unwrap();
    assert!(cached
        .iter()
        .any(|diagnostic| diagnostic.rule_id == "MK004"));
    let fixed = lint::fix(content, diagnostics, &context).unwrap();
    assert_ne!(fixed.content, content);
    assert!(!fixed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.rule_id == "MK004"));
}
