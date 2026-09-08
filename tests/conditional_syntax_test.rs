use rumk::project::{Project, ProjectOptions};
use rumk::rules::{syntax::InvalidSyntax, Rule};

#[test]
fn project_syntax_requires_active_statements_but_preserves_structural_errors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    for (source, expected) in [
        ("MODE = native\nifeq ($(MODE),)\n\techo invalid\nendif\nall:;\n", 0),
        ("MODE =\nifeq ($(MODE),)\n\techo invalid\nendif\nall:;\n", 1),
        ("MODE := $(shell touch must-not-exist)\nifeq ($(MODE),)\n\techo invalid\nendif\nall:;\n", 0),
        ("MODE = native\nifeq ($(MODE),)\nnot a statement\nendif\nall:;\n", 0),
        ("MODE =\nifeq ($(MODE),)\nnot a statement\nendif\nall:;\n", 1),
        ("ifeq ($(UNKNOWN),yes)\nnot a statement\n", 1),
        ("ifeq ($(UNKNOWN),yes)\nelse\nelse\nendif\n", 1),
        ("VALUE := text \\\n $(BROKEN\nall:;\n", 1),
    ] {
        std::fs::write(&root, source).unwrap();
        let project = Project::load(&root, &ProjectOptions::default()).unwrap();
        let diagnostics = InvalidSyntax.check_project(&project);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:?}");
        assert!(!directory.path().join("must-not-exist").exists());
    }
}

#[test]
fn malformed_conditional_expressions_are_not_hidden_by_their_unknown_result() {
    let directory = tempfile::tempdir().unwrap();
    let project = Project::load_with_root_content(
        &directory.path().join("Makefile"),
        "ifeq \"$(BROKEN\" \"yes\"\nall:;\nendif\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    assert!(InvalidSyntax
        .check_project(&project)
        .iter()
        .any(|d| d.message.contains("Unterminated")));
}

#[test]
fn included_syntax_uses_parent_variables_and_keeps_source_locations() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let fragment = directory.path().join("guard.mk");
    std::fs::write(&fragment, "ifeq ($(MODE),)\n\techo invalid\nendif\n").unwrap();
    for (value, expected) in [("native", 0), ("", 1)] {
        std::fs::write(&root, format!("MODE = {value}\ninclude guard.mk\nall:;\n")).unwrap();
        let project = Project::load(&root, &ProjectOptions::default()).unwrap();
        let diagnostics = InvalidSyntax.check_project(&project);
        assert_eq!(diagnostics.len(), expected, "{diagnostics:?}");
        if expected == 1 {
            assert_eq!(diagnostics[0].line, 2);
            assert!(diagnostics[0]
                .source
                .as_ref()
                .unwrap()
                .ends_with("guard.mk"));
        }
    }
}
