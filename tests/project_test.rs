use std::path::PathBuf;

use rumk::eval::{BlockedReason, Truth};
use rumk::project::{DefaultGoal, IncludeResolution, Project, ProjectOptions};

#[test]
fn resolves_nested_static_includes_and_preserves_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let fragments = directory.path().join("mk");
    std::fs::create_dir(&fragments).unwrap();
    std::fs::write(&root, "include mk/common.mk\nall: shared\n").unwrap();
    std::fs::write(
        fragments.join("common.mk"),
        "include mk/nested.mk\nshared:\n\t@:\n",
    )
    .unwrap();
    std::fs::write(fragments.join("nested.mk"), "NESTED := yes\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 3);
    assert_eq!(project.edges().len(), 2);
    let common = match project.edges()[0].resolution {
        IncludeResolution::Resolved(id) => id,
        ref resolution => panic!("expected resolved include, got {resolution:?}"),
    };
    assert_eq!(project.file(common).path.file_name().unwrap(), "common.mk");
    assert_eq!(project.edges()[1].from, common);
    assert!(project.cycles().is_empty());
}

#[test]
fn searches_configured_include_paths_after_the_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let includes = directory.path().join("includes");
    std::fs::create_dir(&includes).unwrap();
    std::fs::write(&root, "include shared.mk\n").unwrap();
    std::fs::write(includes.join("shared.mk"), "SHARED := yes\n").unwrap();
    let options = ProjectOptions {
        include_paths: vec![PathBuf::from("includes")],
        ..ProjectOptions::default()
    };

    let project = Project::load(&root, &options).unwrap();
    assert!(matches!(
        project.edges()[0].resolution,
        IncludeResolution::Resolved(_)
    ));
}

#[test]
fn resolves_nested_directives_from_the_make_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let fragments = directory.path().join("mk");
    std::fs::create_dir(&fragments).unwrap();
    std::fs::write(&root, "include mk/common.mk\n").unwrap();
    std::fs::write(fragments.join("common.mk"), "include nested.mk\n").unwrap();
    std::fs::write(fragments.join("nested.mk"), "WRONG := location\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 2);
    assert!(matches!(
        project.edges()[1].resolution,
        IncludeResolution::Missing { .. }
    ));
}

#[test]
fn records_missing_optional_and_dynamic_includes_without_guessing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "include required.mk\n-include optional.mk\ninclude $(wildcard generated/*.mk)\n",
    )
    .unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert!(matches!(
        project.edges()[0].resolution,
        IncludeResolution::Missing { .. }
    ));
    assert!(!project.edges()[0].optional);
    assert!(matches!(
        project.edges()[1].resolution,
        IncludeResolution::Missing { .. }
    ));
    assert!(project.edges()[1].optional);
    assert_eq!(project.edges()[2].resolution, IncludeResolution::Dynamic);
}

#[test]
fn detects_include_cycles_without_loading_files_twice() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let second = directory.path().join("second.mk");
    std::fs::write(&root, "include second.mk\n").unwrap();
    std::fs::write(&second, "include Makefile\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 2);
    assert_eq!(project.cycles().len(), 1);
    let names = project.cycles()[0]
        .sources
        .iter()
        .map(|source| {
            project
                .file(*source)
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["Makefile", "second.mk", "Makefile"]);
}

#[test]
fn enforces_the_file_limit_without_panicking() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "include one.mk\n").unwrap();
    std::fs::write(directory.path().join("one.mk"), "ONE := yes\n").unwrap();
    let options = ProjectOptions {
        working_directory: None,
        include_paths: Vec::<PathBuf>::new(),
        max_files: 1,
        ..ProjectOptions::default()
    };

    let project = Project::load(&root, &options).unwrap();
    assert_eq!(project.files().len(), 1);
    assert_eq!(
        project.edges()[0].resolution,
        IncludeResolution::LimitExceeded
    );
}

#[test]
fn resolves_variable_expanded_includes_in_evaluation_order() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("mk")).unwrap();
    std::fs::write(
        &root,
        "DIR := mk\nFILES = $(DIR)/one.mk $(DIR)/two.mk\ninclude $(FILES)\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("mk/one.mk"), "ONE := yes\n").unwrap();
    std::fs::write(directory.path().join("mk/two.mk"), "TWO := yes\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 3);
    assert_eq!(project.edges().len(), 2);
    assert_eq!(project.edges()[0].expression, "$(FILES)");
    assert_eq!(project.edges()[0].expanded.as_deref(), Some("mk/one.mk"));
    assert_eq!(project.edges()[1].expanded.as_deref(), Some("mk/two.mk"));
    assert_eq!(project.edges()[0].trace[0].variable, "FILES");
    assert!(project
        .edges()
        .iter()
        .all(|edge| matches!(edge.resolution, IncludeResolution::Resolved(_))));
}

#[test]
fn evaluates_known_branches_and_preserves_unknown_ones() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "MODE := release\n",
            "ifeq ($(MODE),release)\n",
            "include active.mk\n",
            "else\n",
            "include inactive.mk\n",
            "endif\n",
            "ifdef FROM_ENV\n",
            "include uncertain.mk\n",
            "endif\n",
        ),
    )
    .unwrap();
    std::fs::write(directory.path().join("active.mk"), "ACTIVE := yes\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert!(matches!(
        project.edges()[0].resolution,
        IncludeResolution::Resolved(_)
    ));
    assert_eq!(project.edges()[1].resolution, IncludeResolution::Inactive);
    assert_eq!(project.edges()[2].resolution, IncludeResolution::Dynamic);
    assert_eq!(
        project.evaluation().activity(project.root(), 3),
        Truth::True
    );
    assert_eq!(
        project.evaluation().activity(project.root(), 5),
        Truth::False
    );
    assert_eq!(
        project.evaluation().activity(project.root(), 8),
        Truth::Unknown
    );
}

#[test]
fn never_executes_unsafe_make_functions_while_loading() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let sentinel = directory.path().join("should-not-exist");
    std::fs::write(
        &root,
        format!(
            "GENERATED := $(shell touch {})\ninclude $(GENERATED)\n",
            sentinel.display()
        ),
    )
    .unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert!(!sentinel.exists());
    assert_eq!(project.edges()[0].resolution, IncludeResolution::Dynamic);
    assert!(project.edges()[0]
        .blocked
        .contains(&BlockedReason::UnsafeFunction("shell".into())));
}

#[test]
fn honors_predefined_variables_and_infers_gnu_default_goal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "MODE = ignored\n",
            "ifeq ($(MODE),ci)\n",
            "include ci.mk\n",
            "endif\n",
            "all:\n",
        ),
    )
    .unwrap();
    std::fs::write(directory.path().join("ci.mk"), "from-include:\n").unwrap();
    let mut options = ProjectOptions::default();
    options
        .predefined_variables
        .insert("MODE".into(), "ci".into());

    let project = Project::load(&root, &options).unwrap();

    assert!(matches!(
        project.edges()[0].resolution,
        IncludeResolution::Resolved(_)
    ));
    assert_eq!(
        project.evaluation().default_goal(),
        &DefaultGoal::Known("from-include".into())
    );
}
