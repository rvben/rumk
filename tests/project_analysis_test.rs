use rumk::project::{Project, ProjectOptions};
use rumk::project_analysis::SourceLocation;

#[test]
fn merges_symbols_and_preserves_every_source_location() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(
        &root,
        "ROOT := yes\ninclude shared.mk\nall: shared\n\t@echo $(SHARED)\n",
    )
    .unwrap();
    std::fs::write(&shared, "SHARED := yes\nshared: generated\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();
    let index = project.analysis();

    assert!(std::ptr::eq(project.analysis(), project.analysis()));
    let shared_source = project
        .files()
        .iter()
        .find(|file| file.path.ends_with("shared.mk"))
        .unwrap()
        .id;
    assert_eq!(
        index.variable("SHARED").unwrap().definitions[0].location,
        SourceLocation {
            source: shared_source,
            line: 1,
            column: 1,
        }
    );
    assert_eq!(index.references_to("SHARED").count(), 1);
    assert_eq!(
        index.target("shared").unwrap().dependencies[0]
            .location
            .source,
        shared_source
    );
}

#[test]
fn detects_cycles_that_cross_include_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "include shared.mk\nall: library\nlibrary: objects\n").unwrap();
    std::fs::write(directory.path().join("shared.mk"), "objects: library\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(
        project.analysis().dependency_cycles(),
        [vec![String::from("library"), String::from("objects")]]
    );
}

#[test]
fn excludes_conditional_cross_file_edges_from_definite_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "include shared.mk\nalpha: beta\n").unwrap();
    std::fs::write(
        directory.path().join("shared.mk"),
        "ifdef ENABLE_CYCLE\nbeta: alpha\nendif\n",
    )
    .unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert!(project.analysis().dependency_cycles().is_empty());
}

#[test]
fn interleaves_included_statements_at_the_include_site() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "target: before\ninclude shared.mk\ntarget:: after\n").unwrap();
    std::fs::write(directory.path().join("shared.mk"), "target: shared\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();
    let declarations = &project.analysis().target("target").unwrap().declarations;
    let locations = declarations
        .iter()
        .map(|declaration| {
            (
                project
                    .file(declaration.location.source)
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                declaration.location.line,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        locations,
        [
            ("Makefile".into(), 1),
            ("shared.mk".into(), 1),
            ("Makefile".into(), 3)
        ]
    );
}

#[test]
fn expands_static_target_and_prerequisite_names_at_definition_time() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "TARGET := all\n",
            "DEPS = compile test\n",
            "$(TARGET): $(DEPS)\n",
            "compile:\n",
            "test:\n",
        ),
    )
    .unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();
    let all = project.analysis().target("all").unwrap();

    assert!(project.analysis().target("$(TARGET)").is_none());
    assert_eq!(
        all.dependencies
            .iter()
            .map(|dependency| dependency.prerequisite.as_str())
            .collect::<Vec<_>>(),
        ["compile", "test"]
    );
}

#[test]
fn indexes_only_the_definitely_active_conditional_branch() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "MODE := debug\n",
            "ifeq ($(MODE),debug)\n",
            "selected: active\n",
            "else\n",
            "selected: inactive\n",
            "endif\n",
        ),
    )
    .unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();
    let selected = project.analysis().target("selected").unwrap();

    assert_eq!(selected.declarations.len(), 1);
    assert_eq!(selected.dependencies.len(), 1);
    assert_eq!(selected.dependencies[0].prerequisite, "active");
}

#[test]
fn reevaluates_a_makefile_at_each_include_site() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        concat!(
            "NAME := first\n",
            "include shared.mk\n",
            "NAME := second\n",
            "include shared.mk\n",
        ),
    )
    .unwrap();
    std::fs::write(directory.path().join("shared.mk"), "$(NAME):\n").unwrap();

    let project = Project::load(&root, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 2);
    assert_eq!(project.edges().len(), 2);
    assert!(project.analysis().target("first").is_some());
    assert!(project.analysis().target("second").is_some());
}
