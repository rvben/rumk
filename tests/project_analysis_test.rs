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
