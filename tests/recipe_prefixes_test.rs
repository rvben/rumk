use rumk::fix::apply_fixes;
use rumk::parser::parse;
use rumk::project::{Project, ProjectOptions};
use rumk::rules::{recipe_prefixes::RepeatedRecipePrefix, Rule};
use std::process::Command;

#[test]
fn repeated_prefix_fixes_preserve_order_commands_and_source_layout() {
    for (text, expected) in [
        ("all:\n\t@@+-+--echo hi\n", "all:\n\t@+-echo hi\n"),
        ("é:;@@echo 😀\r\n", "é:;@echo 😀\r\n"),
        ("all:\r\n\t--@@echo hi", "all:\r\n\t-@echo hi"),
        (
            ".RECIPEPREFIX := @\nall:\n@@@echo hi\n",
            ".RECIPEPREFIX := @\nall:\n@@echo hi\n",
        ),
        (
            ".RECIPEPREFIX := >\nall:\n>++echo hi\n",
            ".RECIPEPREFIX := >\nall:\n>+echo hi\n",
        ),
        (
            "all:\n\t@@echo one \\\n\t @@two\n",
            "all:\n\t@echo one \\\n\t @@two\n",
        ),
    ] {
        let diagnostics = RepeatedRecipePrefix.check(&parse(text), text);
        assert_eq!(diagnostics.len(), 1, "{text:?}");
        let fixed = apply_fixes(text, &diagnostics).content;
        assert_eq!(fixed, expected);
        assert!(RepeatedRecipePrefix
            .check(&parse(&fixed), &fixed)
            .is_empty());
    }
}

#[test]
fn repeated_prefix_lookalikes_are_left_alone() {
    for text in [
        "all:\n\t@+-echo @@ -- ++\n",
        "all:\n\techo one \\\n\t@@two\n",
        "all:\n\t'@@command'\n",
        "define SCRIPT\nall:\n\t@@echo hi\nendef\n",
        ".ONESHELL:\nall:\n\t@@echo hi\n\t@@echo again\n",
        "ifdef UNKNOWN\nall:\n\t@@echo hi\nendif\n",
        ".RECIPEPREFIX := @\nall:\n@@echo hi\n",
        "all:\n\t$(FLAGS)@@echo hi\n",
    ] {
        assert!(
            RepeatedRecipePrefix.check(&parse(text), text).is_empty(),
            "{text}"
        );
    }
}

#[test]
fn included_oneshell_disables_recipe_prefix_rewrites() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mode.mk"), ".ONESHELL:\n").unwrap();
    let path = dir.path().join("Makefile");
    std::fs::write(&path, "include mode.mk\nall:\n\t@@echo hi\n").unwrap();
    let project = Project::load(&path, &ProjectOptions::default()).unwrap();
    assert!(RepeatedRecipePrefix.check_project(&project).is_empty());
}

#[test]
fn gnu_make_preserves_echoing_failure_and_recursive_dry_run_behavior() {
    if Command::new("make").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for text in [
        "all:\n\t@@echo visible\n",
        "all:\n\t--false\n\t@@echo survived\n",
        "all:\n\t++echo recursive\n",
        "all:;@@echo inline\n",
    ] {
        let fixed = apply_fixes(text, &RepeatedRecipePrefix.check(&parse(text), text)).content;
        for flags in [vec!["-rR"], vec!["-rR", "-n"]] {
            let run = |source: &str| {
                std::fs::write(dir.path().join("Makefile"), source).unwrap();
                Command::new("make")
                    .current_dir(dir.path())
                    .args(&flags)
                    .env_remove("MAKEFLAGS")
                    .env_remove("MFLAGS")
                    .output()
                    .unwrap()
            };
            let before = run(text);
            let after = run(&fixed);
            assert_eq!(before.status.code(), after.status.code(), "{text}");
            assert_eq!(before.stdout, after.stdout, "{text}");
            assert_eq!(before.stderr, after.stderr, "{text}");
        }
    }
}

#[test]
fn expanded_oneshell_and_unknown_include_graphs_never_offer_prefix_fixes() {
    for text in [
        "MODE := .ONESHELL\n$(MODE):\nall:\n\t@@echo hi\n",
        "include absent.mk\nall:\n\t@@echo hi\n",
        "-include generated.mk\nall:\n\t@@echo hi\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Makefile");
        std::fs::write(&path, text).unwrap();
        let project = Project::load(&path, &ProjectOptions::default()).unwrap();
        assert!(
            RepeatedRecipePrefix.check_project(&project).is_empty(),
            "{text}"
        );
        assert!(
            RepeatedRecipePrefix.check(&parse(text), text).is_empty(),
            "{text}"
        );
    }
}
