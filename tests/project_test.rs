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
fn reads_include_paths_the_way_gnu_make_unescapes_them() {
    // GNU Make removes the escapes in an include's file list only after
    // expanding it, so a backslash reaches the expansion and a value may bring
    // its own. A blank ends a path where the run of backslashes before it is
    // even, and each run halves where a blank consumed it.
    for (source, expected) in [
        ("include foo\\ bar.mk\n", vec!["foo bar.mk"]),
        ("include foo\\\\ bar.mk\n", vec!["foo\\", "bar.mk"]),
        ("include foo\\\\\\ bar.mk\n", vec!["foo\\ bar.mk"]),
        ("X := foo\\ bar.mk\ninclude $(X)\n", vec!["foo bar.mk"]),
        (
            "X := foo\\\\ bar.mk\ninclude $(X)\n",
            vec!["foo\\", "bar.mk"],
        ),
        (
            "SLASH := \\\\\nX := foo$(SLASH)\ninclude $(X)\n",
            vec!["foo\\\\"],
        ),
        ("X := bar.mk\ninclude \\$(X)\n", vec!["\\bar.mk"]),
        ("include \\#foo\n", vec!["#foo"]),
        // The run before a comment halves as Make recognizes the comment,
        // whether or not it leaves the `#` escaped.
        ("include foo\\\\#comment\n", vec!["foo\\"]),
        ("include a.mk  b.mk\n", vec!["a.mk", "b.mk"]),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Makefile");
        std::fs::write(&root, source).unwrap();

        let project = Project::load(&root, &ProjectOptions::default()).unwrap();

        let read: Vec<&str> = project
            .edges()
            .iter()
            .filter_map(|edge| edge.expanded.as_deref())
            .collect();
        assert_eq!(read, expected, "reading {source:?}");
    }
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
fn loads_indented_conditionals_after_a_recipe_without_panicking() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let content = "all:\n\t@echo all\n  ifeq ($(MODE),debug)\n  CFLAGS := -g\n  endif\n";
    std::fs::write(&path, content).unwrap();

    let project = Project::load(&path, &ProjectOptions::default()).unwrap();

    assert_eq!(project.files().len(), 1);
    assert!(!project.analysis().has_structural_issues());
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

#[test]
fn static_include_globs_follow_gnu_make_order_and_directory_rules() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("mk")).unwrap();
    for (name, value) in [("b", "second"), ("a", "first"), (".hidden", "hidden")] {
        std::fs::write(
            directory.path().join(format!("mk/{name}.mk")),
            format!("ORDER += {value}\n"),
        )
        .unwrap();
    }
    let source = "DIR = mk\ninclude $(DIR)/*.mk\n.PHONY: probe\nprobe:;@echo $(ORDER)\n";
    std::fs::write(&root, source).unwrap();
    let project = Project::load(&root, &ProjectOptions::default()).unwrap();
    assert_eq!(
        project
            .edges()
            .iter()
            .map(|edge| edge.expanded.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["mk/a.mk", "mk/b.mk"]
    );
    assert_eq!(
        project.evaluation().expand("$(ORDER)").value.as_deref(),
        Some("first second")
    );
    assert!(project
        .edges()
        .iter()
        .all(|edge| matches!(edge.resolution, IncludeResolution::Resolved(_))));
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    if std::process::Command::new(&make)
        .arg("--version")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make"))
    {
        let output = std::process::Command::new(make)
            .current_dir(directory.path())
            .env("LC_ALL", "C")
            .args(["-rR", "-s", "probe"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "first second"
        );
    }
    for pattern in ["mk/?.mk", "mk/a*.mk"] {
        let project = Project::load_with_root_content(
            &root,
            format!("include {pattern}\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(project
            .edges()
            .iter()
            .all(|edge| matches!(edge.resolution, IncludeResolution::Resolved(_))));
    }
}

#[test]
fn uncertain_include_globs_keep_dependency_analysis_blocked() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap().join("Makefile");
    std::fs::create_dir(directory.path().join("mk")).unwrap();
    for pattern in [
        "missing/*.mk",
        "mk/*.mk",
        "*/file.mk",
        "mk/[ab].mk",
        "mk/**.mk",
        "$(wildcard mk/*.mk)",
        "$(UNKNOWN)/*.mk",
    ] {
        let project = Project::load_with_root_content(
            &root,
            format!("-include {pattern}\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert_eq!(project.edges().len(), 1, "{pattern}");
        assert!(
            matches!(project.edges()[0].resolution, IncludeResolution::Dynamic),
            "{pattern}"
        );
    }
    std::fs::write(directory.path().join("mk/a.mk"), "VALUE = disk\n").unwrap();
    let mut options = ProjectOptions::default();
    options.source_overrides.insert(
        directory.path().canonicalize().unwrap().join("mk/b.mk"),
        "VALUE = buffer\n".into(),
    );
    let project =
        Project::load_with_root_content(&root, "include mk/*.mk\n".into(), &options).unwrap();
    assert_eq!(
        project.evaluation().expand("$(VALUE)").value.as_deref(),
        Some("buffer")
    );
    options.max_files = 1;
    let project =
        Project::load_with_root_content(&root, "include mk/*.mk\n".into(), &options).unwrap();
    assert!(matches!(
        project.edges()[0].resolution,
        IncludeResolution::Dynamic
    ));
}

#[test]
fn include_filename_lists_expand_before_any_fragment_changes_variables() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(directory.path().join("first.mk"), "NEXT = wrong.mk\n").unwrap();
    std::fs::write(directory.path().join("correct.mk"), "RESULT = correct\n").unwrap();
    std::fs::write(directory.path().join("wrong.mk"), "RESULT = wrong\n").unwrap();
    for first in ["first.mk", "first*.mk"] {
        let source = format!(
            "NEXT = correct.mk\ninclude {first} $(NEXT)\n.PHONY: probe\nprobe:;@echo $(RESULT)\n"
        );
        std::fs::write(&root, &source).unwrap();
        let project =
            Project::load_with_root_content(&root, source, &ProjectOptions::default()).unwrap();
        assert_eq!(
            project.evaluation().expand("$(RESULT)").value.as_deref(),
            Some("correct")
        );
        let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
        if std::process::Command::new(&make)
            .arg("--version")
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make"))
        {
            let output = std::process::Command::new(make)
                .current_dir(directory.path())
                .args(["-rR", "-s", "probe"])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "correct");
        }
    }
}

/// CPU time the calling thread has spent, which other work on the machine
/// does not inflate the way it inflates elapsed time.
#[cfg(unix)]
fn thread_cpu_time() -> std::time::Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
        0
    );
    std::time::Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

/// Loading reads every statement once and looks up what the parser recorded
/// for it. Looking that record up by searching all of them takes time in the
/// square of the file's length, which a generated Makefile of tens of
/// thousands of rules turns into seconds.
#[cfg(unix)]
#[test]
fn loads_a_long_makefile_in_time_linear_in_its_length() {
    const RULES: usize = 60_000;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let content: String = (0..RULES)
        .map(|index| format!("V{index} = v{index}\nout{index}:\n"))
        .collect();
    std::fs::write(&path, content).unwrap();

    let started = thread_cpu_time();
    let project = Project::load(&path, &ProjectOptions::default()).unwrap();
    let spent = thread_cpu_time() - started;

    // Every statement was read: the last assignment holds its value and the
    // last rule is known.
    let last = RULES - 1;
    assert_eq!(
        project
            .evaluation()
            .expand(&format!("$(V{last})"))
            .as_known(),
        Some(format!("v{last}").as_str())
    );
    assert!(project.analysis().target(&format!("out{last}")).is_some());
    // About 2s where each record is found by its line, and 15s where every
    // statement searches them all, in an unoptimized build.
    assert!(
        spent < std::time::Duration::from_secs(5),
        "loading {RULES} rules took {spent:?} of CPU time"
    );
}
