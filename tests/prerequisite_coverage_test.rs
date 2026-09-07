use rumk::project::{Project, ProjectOptions};
use rumk::rules::prerequisites::{coverage, MissingPrerequisite};
use rumk::rules::Rule;

#[test]
fn coverage_accounts_for_each_visible_edge_and_matches_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("present.txt"), "input").unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("Makefile"),
        "probe: present.txt declared missing.txt -lthing\ndeclared:\n.PRECIOUS: probe\ngenerated/%.dat: template/%.src\n\t@echo generated\n".into(),
        &ProjectOptions::default(),
    ).unwrap();
    let report = coverage(&project);
    assert!(
        report.root_blockers.is_empty(),
        "{:?}",
        report.root_blockers
    );
    assert_eq!(report.outcomes.values().sum::<usize>(), report.edges.len());
    for reason in [
        "file_or_io_uncertainty",
        "declared_target",
        "missing",
        "unsupported_name",
        "special_target",
        "pattern_declaration",
    ] {
        assert_eq!(report.outcomes.get(reason), Some(&1), "{reason}");
    }
    let diagnostics = MissingPrerequisite.check_project(&project);
    let missing: Vec<_> = report
        .edges
        .iter()
        .filter(|edge| edge.outcome == "missing")
        .collect();
    assert_eq!(missing.len(), diagnostics.len());
    assert_eq!(missing[0].line, diagnostics[0].line);
    assert!(diagnostics[0].message.contains(&missing[0].prerequisite));
}

#[test]
fn coverage_retains_overlapping_root_exclusions_without_running_shells() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("fragment.mk"),
        "VALUE := $(shell touch never-created)\ninclude absent.mk\nprobe: missing.txt\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    let report = coverage(&project);
    for reason in ["fragment_root", "shell_function", "unresolved_include"] {
        assert!(report.root_blockers.contains_key(reason), "{reason}");
    }
    assert_eq!(report.outcomes.get("root_excluded"), Some(&1));
    assert!(MissingPrerequisite.check_project(&project).is_empty());
    assert!(!dir.path().join("never-created").exists());
}

#[test]
fn phony_coverage_uses_the_expansion_at_each_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("commands.mk"), ".PHONY: $(COMMAND)\n").unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("Makefile"),
        "COMMAND = first\ninclude commands.mk\nCOMMAND = second\ninclude commands.mk\nprobe: missing.txt\n".into(),
        &ProjectOptions::default(),
    ).unwrap();
    let source = project
        .files()
        .iter()
        .find(|file| file.path.ends_with("commands.mk"))
        .unwrap()
        .id;
    let lists: Vec<_> = project
        .evaluation()
        .rules(source, 1)
        .iter()
        .map(|rule| rule.prerequisites.clone())
        .collect();
    assert_eq!(
        lists,
        vec![vec!["first".to_string()], vec!["second".to_string()]]
    );
    assert!(coverage(&project).root_blockers.is_empty());
    assert_eq!(MissingPrerequisite.check_project(&project).len(), 1);
}
