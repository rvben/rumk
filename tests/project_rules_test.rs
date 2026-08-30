use rumk::project::{Project, ProjectOptions};
use rumk::rules::best_practices::{DependencyCycle, DuplicateRecipe, MissingPhony};
use rumk::rules::project::{
    IncludeCycle, MissingInclude, MixedTargetSeparators, UndefinedVariableReference,
    UnreachableTarget, UnresolvedIncludeExpression,
};
use rumk::rules::Rule;

fn load(root: &std::path::Path) -> Project {
    Project::load(root, &ProjectOptions::default()).unwrap()
}

#[test]
fn reports_mixed_separators_across_files_at_the_conflicting_source() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(&root, "include shared.mk\nserver: first\n").unwrap();
    std::fs::write(&shared, "server:: second\n").unwrap();

    let diagnostics = MixedTargetSeparators.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK004");
    assert_eq!(
        diagnostics[0].source.as_deref(),
        Some(root.canonicalize().unwrap().as_path())
    );
}

#[test]
fn missing_phony_only_offers_fixes_for_the_processed_root_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(&root, "include shared.mk\nall clean:\n\t@:\n").unwrap();
    std::fs::write(&shared, "test:\n\t@:\n").unwrap();

    let diagnostics = MissingPhony.check_project(&load(&root));
    let root_path = root.canonicalize().unwrap();
    let shared_path = shared.canonicalize().unwrap();

    assert_eq!(diagnostics.len(), 2);
    let root_diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source.as_deref() == Some(root_path.as_path()))
        .unwrap();
    assert!(root_diagnostic.fixable);
    assert!(root_diagnostic.message.contains("'all', 'clean'"));
    let included_diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source.as_deref() == Some(shared_path.as_path()))
        .unwrap();
    assert!(!included_diagnostic.fixable);
    assert!(included_diagnostic.fix.is_none());
}

#[test]
fn missing_include_allows_optional_dynamic_and_remakeable_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "include missing.mk\n",
            "-include optional.mk\n",
            "include $(wildcard generated/*.mk)\n",
            "include generated.mk\n",
            "generated.mk:\n\t@touch $@\n",
        ),
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("missing.mk"));
}

#[test]
fn missing_include_reports_the_expanded_path() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "FILE := absent.mk\ninclude $(FILE)\n").unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("absent.mk"));
    assert!(!diagnostics[0].message.contains("$(FILE)"));
}

#[test]
fn reports_include_cycles_on_the_closing_directive() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(&root, "include shared.mk\n").unwrap();
    std::fs::write(&shared, "VALUE := yes\ninclude Makefile\n").unwrap();

    let diagnostics = IncludeCycle.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 2);
    assert_eq!(
        diagnostics[0].source.as_deref(),
        Some(shared.canonicalize().unwrap().as_path())
    );
}

#[test]
fn undefined_references_respect_project_builtins_and_predefined_variables() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "KNOWN := yes\n",
            "OUTPUT := $(KNOWN) $(MAKE) $(FROM_CLI) $(MISSING)\n",
            "all:\n\t@echo $(RECIPE_PARAMETER)\n",
            "define command\n\t@echo $(1) $(DEFERRED_PARAMETER)\nendef\n",
        ),
    )
    .unwrap();
    let rule = UndefinedVariableReference::new([String::from("FROM_CLI")]);

    let diagnostics = rule.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("MISSING"));
}

#[test]
fn undefined_references_ignore_recipe_and_deferred_macro_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "define ssh_to\n",
            "ssh $(1)@$(2) -i $(3)\n",
            "endef\n",
            "deploy:\n",
            "\t@test -n \"$(HOST)\"\n",
        ),
    )
    .unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert!(diagnostics.is_empty());
}

#[test]
fn reachability_requires_explicit_entries_and_follows_cross_file_edges() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "include shared.mk\nall: library\norphan:\n\t@:\n").unwrap();
    std::fs::write(
        directory.path().join("shared.mk"),
        "library: object\nobject:\n",
    )
    .unwrap();
    let project = load(&root);

    let inferred = UnreachableTarget::default().check_project(&project);
    assert!(inferred.is_empty());
    let diagnostics = UnreachableTarget::new(vec![String::from("all")]).check_project(&project);

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("orphan"));
}

#[test]
fn context_sensitive_rules_merge_phonies_recipes_and_dependency_edges() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "include shared.mk\n",
            "all: library\n",
            "server:\n\t@echo root\n",
            "library: object\n",
        ),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("shared.mk"),
        concat!(
            ".PHONY: all\n",
            "server:\n\t@echo shared\n",
            "object: library\n",
        ),
    )
    .unwrap();
    let project = load(&root);

    assert!(MissingPhony.check_project(&project).is_empty());
    let duplicate = DuplicateRecipe.check_project(&project);
    assert_eq!(duplicate.len(), 1);
    assert_eq!(duplicate[0].rule_id, "MK204");
    let cycles = DependencyCycle.check_project(&project);
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].rule_id, "MK205");
}

#[test]
fn unresolved_include_explains_the_safety_boundary_and_trace() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "FILES := $(wildcard generated/*.mk)\ninclude $(FILES)\n",
    )
    .unwrap();
    let project = load(&root);

    let diagnostics = UnresolvedIncludeExpression.check_project(&project);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK210");
    assert_eq!(diagnostics[0].severity, rumk::diagnostic::Severity::Info);
    assert!(diagnostics[0].message.contains("function 'wildcard'"));
    assert!(diagnostics[0].message.contains("via FILES at"));
}

#[test]
fn unresolved_include_reports_unsafe_functions_without_executing_them() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let sentinel = directory.path().join("forbidden");
    std::fs::write(
        &root,
        format!(
            "FILES := $(shell touch {})\ninclude $(FILES)\n",
            sentinel.display()
        ),
    )
    .unwrap();
    let project = load(&root);

    let diagnostics = UnresolvedIncludeExpression.check_project(&project);

    assert!(!sentinel.exists());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]
        .message
        .contains("function 'shell' is intentionally never executed"));
}
