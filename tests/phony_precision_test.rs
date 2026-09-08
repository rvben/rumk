//! Reduced, authored regressions from the pinned upstream diagnostic review.
use rumk::parser::parse;
use rumk::project::{Project, ProjectOptions};
use rumk::rules::best_practices::MissingPhony;
use rumk::rules::Rule;

#[test]
fn configured_commands_keep_include_activity_and_output_safeguards() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let rule = MissingPhony::default().command_targets(["verify".into()]);
    for (source, included, expected) in [
        ("verify:\n\t@echo checked\n", "", true),
        ("include shared.mk\n", "verify:\n\t@echo checked\n", true),
        (
            "include shared.mk\nverify:\n\t@echo checked\n",
            ".PHONY: $(EXTRA) verify\n",
            false,
        ),
        ("ifeq (a,b)\nverify:\n\t@echo checked\nendif\n", "", false),
        ("verify:\n\t$(CC) input.c -o $@\n", "", false),
        ("Verify:\n\t@echo checked\n", "", false),
    ] {
        std::fs::write(directory.path().join("shared.mk"), included).unwrap();
        let project =
            Project::load_with_root_content(&root, source.into(), &ProjectOptions::default())
                .unwrap();
        assert_eq!(
            !rule.check_project(&project).is_empty(),
            expected,
            "{source}"
        );
    }
}

fn check(source: &str, included: Option<&str>) -> Vec<rumk::diagnostic::Diagnostic> {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    if let Some(content) = included {
        std::fs::write(directory.path().join("shared.mk"), content).unwrap();
    }
    let project =
        Project::load_with_root_content(&root, source.into(), &ProjectOptions::default()).unwrap();
    MissingPhony::default().check_project(&project)
}

#[test]
fn literal_phony_names_survive_an_unresolved_neighbor() {
    for declaration in [
        ".PHONY: all clean $(DYNAMIC)\n",
        ".PHONY: $(DYNAMIC) all clean\n",
        ".PHONY: all $(DYNAMIC) clean\n",
        ".PHONY: all $(shell printf extra) clean\n",
        ".PHONY: all $(wildcard tasks-*) clean\n",
        ".PHONY: all $(sort $(DYNAMIC)) clean # test\n",
        ".PHONY: all \\\n $(DYNAMIC) clean\n",
    ] {
        let source = format!("all clean:\n\t@:\n{declaration}");
        assert!(check(&source, None).is_empty(), "{source}");
        let included = "include shared.mk\nall clean:\n\t@:\n";
        assert!(
            check(included, Some(declaration)).is_empty(),
            "{declaration}"
        );
    }
}

#[test]
fn uncertain_phony_expressions_do_not_make_new_names_definitely_phony() {
    let diagnostics = check(
        "all clean test:\n\t@:\n.PHONY: all $(DYNAMIC) clean # test\n",
        None,
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].message,
        "Target 'test' should be declared .PHONY"
    );
    for declaration in [
        "ifeq (a,b)\n.PHONY: all $(DYNAMIC)\nendif\n",
        ".PHONY_EXTRA: all $(DYNAMIC)\n",
        ".PHONY: all$(DYNAMIC)\n",
        "# .PHONY: all $(DYNAMIC)\n",
        ".PHONY: $(filter all,other) $(DYNAMIC)\n",
    ] {
        assert_eq!(
            check(&format!("all:\n\t@:\n{declaration}"), None).len(),
            1,
            "{declaration}"
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let project = Project::load_with_root_content(
        &directory.path().join("Makefile"),
        ".PHONY: $(DYNAMIC) all\nall:\n\t@:\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    // An unresolved expression can affect token boundaries. Suppress a
    // heuristic recommendation, but do not invent a definite graph fact.
    assert!(!project.analysis().targets["all"].phony);
}

#[test]
fn named_compiler_outputs_are_not_command_targets() {
    for compiler in [
        "$(CC)", "${CC}", "$(CXX)", "cc", "gcc", "clang", "c++", "g++", "clang++",
    ] {
        for output in ["-o $@", "-o '$@'", "-o \"$@\"", "-o test", "-o$@"] {
            let source = format!(
                "test: source.c\n\t{compiler} $(CFLAGS) $< {output}\nclean:\n\trm -f test\n"
            );
            let diagnostics = check(&source, None);
            assert_eq!(diagnostics.len(), 1, "{source}");
            assert_eq!(
                diagnostics[0].message, "Target 'clean' should be declared .PHONY",
                "{source}"
            );
            let local = MissingPhony::default().check(&parse(&source), &source);
            assert_eq!(local[0].message, diagnostics[0].message, "{source}");
            let root = "include shared.mk\n";
            assert_eq!(
                check(root, Some(&source))[0].message,
                diagnostics[0].message,
                "{source}"
            );
        }
    }
}

#[test]
fn stamp_outputs_are_not_command_targets_but_touch_inputs_are_not_outputs() {
    for command in [
        "touch all",
        "touch $@",
        "touch '$@'",
        "touch -- all",
        "touch other ./all",
    ] {
        let source = format!("all:\n\t{command}\nclean:\n\trm -f all\n");
        let findings = check(&source, None);
        assert_eq!(findings.len(), 1, "{source}: {findings:?}");
        assert_eq!(
            findings[0].message,
            "Target 'clean' should be declared .PHONY"
        );
        assert_eq!(
            MissingPhony::default().check(&parse(&source), &source)[0].message,
            findings[0].message
        );
        assert_eq!(
            check("include shared.mk\n", Some(&source))[0].message,
            findings[0].message
        );
    }
    for command in [
        "touch unrelated",
        "touch -r all unrelated",
        "touch -c all",
        "echo touch all",
        "touch all; rm all",
    ] {
        let source = format!("all:\n\t{command}\n");
        assert_eq!(check(&source, None).len(), 1, "{source}");
    }
    assert_eq!(
        check(
            "all:\nifeq (a,b)\n\ttouch all\nendif\n\t@echo command\n",
            None
        )
        .len(),
        1
    );
}

#[test]
fn compiler_lookalikes_and_other_output_names_still_get_checked() {
    for recipe in [
        "echo '$(CC) source.c -o $@'",
        "echo cc source.c -o $@",
        "$(CC) source.c -o another-file",
        "$(CC) source.c -o $@ -o another-file",
        "$(CC) source.c -o $@.bin",
        "$(CC) source.c -o $$@",
        "rm -f $@",
        "./test",
        "$(MAKE) -C tests $@",
    ] {
        let source = format!("test:\n\t{recipe}\n");
        assert_eq!(check(&source, None).len(), 1, "{source}");
    }
}

#[test]
fn inactive_compiler_recipes_do_not_silence_active_command_targets() {
    let source = "test:\nifeq (a,b)\n\t$(CC) source.c -o $@\nendif\n\t./run-tests\n";
    assert_eq!(check(source, None).len(), 1);
    assert_eq!(
        MissingPhony::default().check(&parse(source), source).len(),
        1
    );
}

#[test]
fn reviewed_phony_cases_agree_with_gnu_make_timestamp_behavior() {
    use std::process::Command;
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    let version = Command::new(&make).arg("--version").output();
    if !version.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make")) {
        eprintln!("GNU Make unavailable; timestamp probes skipped");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("test"), "existing artifact").unwrap();
    let query = |source: &str| {
        std::fs::write(root.join("Makefile"), source).unwrap();
        Command::new(&make)
            .current_dir(root)
            .args(["-rR", "-q", "test"])
            .output()
            .unwrap()
    };
    // No recipe is executed: -q observes whether Make would rebuild the
    // existing output. The compiler spelling is the reduced upstream idiom.
    for file_target in ["test:\n\t$(CC) source.c -o $@\n", "test:\n\ttouch $@\n"] {
        assert!(check(file_target, None).is_empty());
        assert_eq!(query(file_target).status.code(), Some(0));
        assert_eq!(
            query(&format!(".PHONY: test\n{file_target}")).status.code(),
            Some(1)
        );
    }
    for declaration in [
        ".PHONY: test $(shell printf extra)\n",
        ".PHONY: $(shell printf extra) test\n",
    ] {
        let source = format!("test:\n\t@:\n{declaration}");
        assert!(check(&source, None).is_empty());
        assert_eq!(query(&source).status.code(), Some(1));
    }
    // Double-colon command targets already run even if a same-named file
    // exists; a .PHONY suggestion here is a convention, not a proven bug.
    assert_eq!(query("test::\n\t@:\n").status.code(), Some(1));
}

#[test]
fn conditional_orphan_recipe_review_distinguishes_supported_and_unsupported_branches() {
    use rumk::rules::syntax::InvalidSyntax;
    use std::process::Command;
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    if !Command::new(&make)
        .arg("--version")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make"))
    {
        eprintln!("GNU Make unavailable; conditional syntax probes skipped");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    for (arch, fails) in [("supported", false), ("", true)] {
        let source = format!(
            "ARCH := {arch}\nifeq ($(ARCH),)\n\t$(error unsupported architecture)\nendif\nall:;\n"
        );
        std::fs::write(directory.path().join("Makefile"), &source).unwrap();
        let output = Command::new(&make)
            .current_dir(directory.path())
            .args(["-rR", "all"])
            .output()
            .unwrap();
        assert_eq!(!output.status.success(), fails);
        if fails {
            assert!(String::from_utf8_lossy(&output.stderr).contains("before first target"));
        }
        // MK006 detects a conditional defect; it does not establish which
        // architecture an unknown host-dependent expression will choose.
        assert!(!InvalidSyntax.check(&parse(&source), &source).is_empty());
    }
}

#[test]
fn reviewed_recursive_make_warning_matches_gnu_make_dry_run_behavior() {
    use rumk::diagnostic::Applicability;
    use rumk::rules::best_practices::RecursiveMake;
    use std::process::Command;
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    if !Command::new(&make)
        .arg("--version")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make"))
    {
        eprintln!("GNU Make unavailable; recursive invocation probe skipped");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("child.mk"),
        "check:\n\t@echo reviewed-child-probe\n",
    )
    .unwrap();
    let original = "all:\n\tmake -f child.mk check\n";
    let diagnostics = RecursiveMake.check(&parse(original), original);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].fix.as_ref().unwrap().applicability,
        Applicability::Unsafe
    );
    let fixed = rumk::fix::apply_fixes(original, &diagnostics).content;
    for (source, visits_child) in [(original, false), (fixed.as_str(), true)] {
        std::fs::write(directory.path().join("Makefile"), source).unwrap();
        let output = Command::new(&make)
            .current_dir(directory.path())
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .env_remove("GNUMAKEFLAGS")
            .args(["-rR", "-n", "all"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).contains("reviewed-child-probe"),
            visits_child
        );
    }
}
