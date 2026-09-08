use rumk::config::Config;
use serde_json::Value;
use std::process::Command;

fn rumk() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rumk"))
}

#[test]
fn directory_json_matches_rumdl_flat_diagnostic_shape() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("one.mk"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("two.mk"), "clean:\n    true\n").unwrap();

    let output = rumk()
        .args([
            "check",
            directory.path().to_str().unwrap(),
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = document.as_array().unwrap();

    assert!(!output.status.success());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic["file"].is_string()));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic["rule"].is_string()));
}

#[test]
fn auto_discovered_config_errors_are_reported() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    std::fs::write(directory.path().join(".rumk.toml"), "not valid toml = [").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to parse config file"));
}

#[test]
fn fix_on_a_clean_file_is_a_byte_for_byte_noop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let content = ".PHONY: all\nall:\n\ttrue\n";
    std::fs::write(&path, content).unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), content.as_bytes());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Fixed 0 issues"));
}

#[cfg(unix)]
#[test]
fn fix_writes_through_a_symlink_instead_of_replacing_it() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("real.mk");
    let link = directory.path().join("Makefile");
    std::fs::write(&target, ".PHONY: all\nall:\n    true\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(std::fs::read_link(&link).unwrap(), target);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        ".PHONY: all\nall:\n\ttrue\n"
    );
}

#[test]
fn a_byte_order_mark_is_read_past_the_way_gnu_make_reads_past_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = "\u{feff}.PHONY: all\nall:\n\t@echo hi\n";
    std::fs::write(&path, original).unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn a_fix_keeps_the_byte_order_mark_and_counts_it_in_byte_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "\u{feff}.PHONY: all\nall:\n    @echo hi\n").unwrap();

    let reported = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&reported.stdout).unwrap();
    let recipe = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK001")
        .unwrap();

    // The mark takes three bytes, so the spaces the recipe is indented with
    // stand at bytes 20 to 24 of the file.
    assert_eq!(recipe["fix"]["range"]["start"], 20);
    assert_eq!(recipe["fix"]["range"]["end"], 24);

    let fixed = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    assert!(fixed.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "\u{feff}.PHONY: all\nall:\n\t@echo hi\n"
    );
}

#[test]
fn fix_reports_only_issues_remaining_after_the_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, ".PHONY: all\nall:\n    true\n").unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: all\nall:\n\ttrue\n"
    );
    assert!(stdout.contains("Fixed 1 issue"));
    assert!(stdout.contains("[MK001]"));
    assert!(stdout.contains("[fixed]"));
}

#[test]
fn check_fix_applies_only_the_repairs_make_cannot_tell_apart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all clean:\n    make -C sub && gmake test\n").unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "all clean:\n\tmake -C sub && gmake test\n"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fixed 1 issue"));
    // The recipe indentation is repaired, and the declaration and the sub-make
    // spelling are reported with their fixes withheld.
    assert!(stdout.contains("[MK001]"));
    assert!(stdout.contains("[MK201]"));
    assert!(stdout.contains("[MK203]"));
    assert!(stdout.contains("2 fixes can change what Make does"));
    assert!(stdout.contains("--unsafe-fixes"));
}

#[test]
fn check_fix_applies_the_repairs_that_change_make_behavior_when_asked() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all clean:\n    make -C sub && gmake test\n").unwrap();

    let output = rumk()
        .args(["check", path.to_str().unwrap(), "--fix", "--unsafe-fixes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        ".PHONY: all clean\nall clean:\n\t$(MAKE) -C sub && $(MAKE) test\n"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[MK001]"));
    assert!(stdout.contains("[MK201]"));
    assert!(stdout.contains("[MK203]"));
    assert!(stdout.contains("Fixed 3 issues"));
}

#[test]
fn configured_unsafe_fixes_are_applied_without_the_flag() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all clean:\n\tmake -C sub\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[global]\nunsafe-fixes = true\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        ".PHONY: all clean\nall clean:\n\t$(MAKE) -C sub\n"
    );
}

#[test]
fn no_unsafe_fixes_overrides_the_configured_setting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all clean:\n\tmake -C sub\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[global]\nunsafe-fixes = true\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--no-unsafe-fixes"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "all clean:\n\tmake -C sub\n"
    );
}

#[test]
fn the_suggested_command_keeps_the_opt_in_the_counted_fixes_need() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "all clean:\n\tmake -C sub\n",
    )
    .unwrap();

    let counted = rumk()
        .current_dir(directory.path())
        .args(["check", "--no-config", "Makefile", "--unsafe-fixes"])
        .output()
        .unwrap();
    let withheld = rumk()
        .current_dir(directory.path())
        .args(["check", "--no-config", "Makefile"])
        .output()
        .unwrap();

    // The two unsafe fixes are counted because this run asked for them, and the
    // command it prints has to ask again or it fixes nothing.
    let counted = String::from_utf8_lossy(&counted.stdout);
    assert!(counted.contains("Run `rumk check --fix --unsafe-fixes` to fix 2 issues"));
    // Nothing is applicable without the opt-in, so there is no such command to
    // print, only the note that the fixes are there to be asked for.
    let withheld = String::from_utf8_lossy(&withheld.stdout);
    assert!(!withheld.contains("Run `rumk check --fix`"));
    assert!(withheld.contains("2 fixes can change what Make does"));
}

#[test]
fn fmt_lays_the_file_out_and_leaves_what_make_does_to_check() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "clean:\n    make -C sub\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[global]\nunsafe-fixes = true\n",
    )
    .unwrap();

    let formatted = rumk()
        .current_dir(directory.path())
        .args(["fmt", "Makefile"])
        .output()
        .unwrap();

    // Formatting repairs the indentation and stops there: declaring the target
    // .PHONY and spelling the sub-make $(MAKE) are what Make does with the
    // file, so `fmt` neither applies them nor reports them, whatever the
    // configuration asked for.
    assert!(formatted.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "clean:\n\tmake -C sub\n"
    );
    let stdout = String::from_utf8_lossy(&formatted.stdout);
    assert!(stdout.contains("[MK001]"));
    assert!(!stdout.contains("[MK201]"));
    assert!(!stdout.contains("[MK203]"));
    assert!(!stdout.contains("can change what Make does"));
    // A formatting run says the file is laid out, not that nothing is wrong
    // with it, because it never looked at what Make does with the file.
    assert!(stdout.contains("1 file formatted"));
    assert!(!stdout.contains("No issues found"));

    // The findings are still there, and `check` is where they are reported.
    let checked = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&checked.stdout);
    assert_eq!(checked.status.code(), Some(1));
    assert!(stdout.contains("[MK201]"));
    assert!(stdout.contains("[MK203]"));
}

#[test]
fn fmt_check_passes_a_laid_out_file_that_check_still_reports_on() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = "all:\n\tcd build && $(MAKE) install\n";
    std::fs::write(&path, original).unwrap();

    let formatted = rumk()
        .current_dir(directory.path())
        .args(["fmt", "--check", "Makefile"])
        .output()
        .unwrap();
    let checked = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();

    // A file laid out the way Rumk lays it out passes `fmt --check` silently,
    // so the formatting gate does not fail on lint findings a project has
    // chosen to keep.
    assert_eq!(formatted.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&formatted.stdout), "");
    assert_eq!(String::from_utf8_lossy(&formatted.stderr), "");
    assert_eq!(checked.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&checked.stdout).contains("[MK201]"));
}

#[test]
fn fmt_reports_a_layout_finding_it_has_no_fix_for() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    // The recipe prefix is decided at run time, so MK001 reports the
    // space-indented line without writing a character it had to guess.
    let original = "ifdef ALTERNATE\n.RECIPEPREFIX := >\nendif\nall:\n    true\n";
    std::fs::write(&path, original).unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["fmt", "Makefile"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(stdout.contains("[MK001]"));
    assert!(!stdout.contains("rumk check --fix"));
}

#[test]
fn fmt_lays_out_a_layout_rule_whose_severity_is_configured() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "all:\n    true\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK001]\nseverity = \"warning\"\n",
    )
    .unwrap();

    let gate = rumk()
        .current_dir(directory.path())
        .args(["fmt", "--check", "Makefile"])
        .output()
        .unwrap();
    let formatted = rumk()
        .current_dir(directory.path())
        .args(["fmt", "Makefile"])
        .output()
        .unwrap();

    // Choosing what a rule's findings are called says nothing about which
    // command runs it, so a configured severity leaves MK001 a layout rule.
    assert_eq!(gate.status.code(), Some(1));
    assert!(formatted.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "all:\n\ttrue\n",
        "a severity of its own must not take MK001 out of `fmt`"
    );
    assert!(String::from_utf8_lossy(&formatted.stdout).contains("[MK001]"));
}

#[test]
fn fmt_wraps_a_long_phony_declaration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["fmt", "Makefile"])
        .output()
        .unwrap();

    // Where a line is broken is layout, so MK101 is `fmt`'s to apply.
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: build test lint \\\n        release docs\nclean:\n\trm -rf build\n"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("[MK101]"));
}

#[test]
fn long_phony_declarations_are_wrapped_when_the_phony_fix_is_withheld() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    // MK201 would have rewritten the declaration, wrapping it on the way, but
    // its fix can change what Make does and this run withheld it. The long line
    // is MK101's to wrap.
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: build test lint \\\n        release docs\nclean:\n\trm -rf build\n"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fixed 1 issue"));
    assert!(stdout.contains("[MK201]"));
    assert!(stdout.contains("1 fix can change what Make does"));
}

#[test]
fn long_phony_declarations_are_wrapped_when_the_phony_rule_is_ignored_for_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n\n[per-file-ignores]\n\"Makefile\" = [\"MK201\"]\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--unsafe-fixes"])
        .output()
        .unwrap();

    // The run asked for unsafe fixes, but MK201 is ignored for this file, so
    // nothing else is going to rewrite the declaration.
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: build test lint \\\n        release docs\nclean:\n\trm -rf build\n"
    );
}

#[test]
fn the_fixable_count_matches_what_fixing_repairs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let reported = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--unsafe-fixes"])
        .output()
        .unwrap();
    let fixed = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--unsafe-fixes"])
        .output()
        .unwrap();

    // MK201 rewrites the declaration and MK101 wraps what MK201 leaves, one
    // after the other. Both fixes are real, so both are counted.
    assert!(String::from_utf8_lossy(&reported.stdout).contains("to fix 2 issues"));
    assert!(String::from_utf8_lossy(&fixed.stdout).contains("Fixed 2 issues"));
}

#[test]
fn a_fix_two_passes_are_needed_for_is_counted_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs # keep in sync\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--unsafe-fixes"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // MK101 wraps the whole declaration and MK201 inserts into it before the
    // comment, so one pass applies one of them and the next pass the other.
    // Two violations were repaired, and the report says two.
    assert!(output.status.success());
    assert_eq!(stdout.matches("[MK101]").count(), 1);
    assert!(stdout.contains("Fixed 2 issues"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: build test lint \\\n        release docs \\\n        clean # keep in sync\nclean:\n\trm -rf build\n"
    );
}

#[test]
fn long_phony_declarations_are_left_to_the_phony_fix_when_it_runs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(
        &path,
        ".PHONY: build test lint release docs\nclean:\n\trm -rf build\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--unsafe-fixes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ".PHONY: build test lint \\\n        release docs clean\nclean:\n\trm -rf build\n"
    );
}

#[test]
fn diff_reports_the_fixes_it_withheld() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "clean:\n\trm -rf build\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--diff"])
        .output()
        .unwrap();

    // Every fix here was withheld, so there is no patch to print. The run still
    // fails, and stderr says why and how to ask for the fix, leaving the patch
    // on stdout clean for whatever reads it.
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1 fix can change what Make does"));
    assert!(stderr.contains("rumk check --fix --unsafe-fixes"));
}

#[test]
fn quiet_diff_leaves_out_the_withheld_fix_report() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "clean:\n\trm -rf build\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--diff", "--quiet"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn check_fix_wraps_long_static_phony_declarations_at_the_configured_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, ".PHONY: build test lint clean release\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 32\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        ".PHONY: build test lint clean \\\n        release\n"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[MK101]"));
    assert!(stdout.contains("Fixed 1 issue"));
}

#[test]
fn directory_checks_enforce_ignored_paths() {
    let directory = tempfile::tempdir().unwrap();
    let ignored = directory.path().join("vendor");
    std::fs::create_dir(&ignored).unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(ignored.join("bad.mk"), "clean:\n    true\n").unwrap();
    let config = directory.path().join("rumk.toml");
    std::fs::write(&config, "[global]\nexclude = [\"vendor/**\"]\n").unwrap();

    let output = rumk()
        .args([
            "check",
            directory.path().to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = document.as_array().unwrap();

    assert!(output.status.success());
    assert!(diagnostics.is_empty());
}

#[test]
fn fmt_fixes_files_and_uses_formatter_exit_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, ".PHONY: all\nall:\n    true\n").unwrap();

    let output = rumk()
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        ".PHONY: all\nall:\n\ttrue\n"
    );
}

#[test]
fn fmt_check_prints_a_diff_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = ".PHONY: all\nall:\n    true\n";
    std::fs::write(&path, original).unwrap();

    let output = rumk()
        .args(["fmt", "--check", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(String::from_utf8_lossy(&output.stdout).contains("-    true"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("+\ttrue"));
}

#[test]
fn warnings_fail_by_default_and_fail_on_can_relax_the_policy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, "clean:\n\ttrue\n").unwrap();

    let default_output = rumk()
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let relaxed_output = rumk()
        .args(["check", "--fail-on", "error", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(default_output.status.code(), Some(1));
    assert_eq!(relaxed_output.status.code(), Some(0));
}

#[test]
fn rule_and_config_commands_provide_rumdl_style_introspection() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK101]\nline-length = 88\n",
    )
    .unwrap();

    let rule_output = rumk().args(["rule", "MK101"]).output().unwrap();
    let lint_rule_output = rumk().args(["rule", "MK201"]).output().unwrap();
    let read_rule_output = rumk().args(["rule", "MK007"]).output().unwrap();
    let fixable_output = rumk().args(["rule", "--fixable"]).output().unwrap();
    let config_output = rumk()
        .current_dir(directory.path())
        .args(["config", "get", "MK101.line-length"])
        .output()
        .unwrap();
    let file_output = rumk()
        .current_dir(directory.path())
        .args(["config", "file"])
        .output()
        .unwrap();

    assert!(rule_output.status.success());
    let rule_stdout = String::from_utf8_lossy(&rule_output.stdout);
    assert!(rule_stdout.contains("MK101"));
    assert!(rule_stdout.contains("Category: style"));
    assert!(rule_stdout.contains("Default: enabled"));
    assert!(rule_stdout.contains("Fixable: yes"));
    assert!(rule_stdout.contains("Scope: file"));
    assert!(rule_stdout.contains("Commands: check, fmt"));
    assert!(rule_stdout.contains("line-length = 120"));
    assert!(rule_stdout.contains("docs/mk101.md"));
    let lint_rule_stdout = String::from_utf8_lossy(&lint_rule_output.stdout);
    assert!(lint_rule_stdout.contains("Scope: project"));
    assert!(lint_rule_stdout.contains("Commands: check\n"));
    // MK007 is not a layout rule, but it reports a path that could not be read
    // and both commands read paths.
    assert!(String::from_utf8_lossy(&read_rule_output.stdout).contains("Commands: check, fmt"));
    assert_eq!(
        String::from_utf8_lossy(&fixable_output.stdout)
            .lines()
            .map(|line| line.split_whitespace().next().unwrap())
            .collect::<Vec<_>>(),
        ["MK001", "MK101", "MK105", "MK106", "MK201", "MK203", "MK211", "MK218"]
    );
    assert_eq!(String::from_utf8_lossy(&config_output.stdout).trim(), "88");
    assert!(String::from_utf8_lossy(&file_output.stdout).contains(".rumk.toml"));
}

#[test]
fn pyproject_configuration_works_with_discovery_explicit_path_and_isolation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("pyproject.toml"),
        "[project]\nname = 'example'\n[tool.rumk.MK101]\nline-length = 88\n",
    )
    .unwrap();
    for flags in [
        vec![],
        vec!["--config", "pyproject.toml"],
        vec!["--isolated"],
    ] {
        let output = rumk()
            .current_dir(directory.path())
            .args(&flags)
            .args(["config", "get", "MK101.line-length"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let expected = if flags.contains(&"--isolated") {
            "120"
        } else {
            "88"
        };
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    }
}

#[test]
fn config_get_exposes_missing_phony_placement() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK201]\nplacement = \"top\"\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["config", "get", "MK201.placement"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "top");
}

#[test]
fn configured_missing_phony_placement_drives_fixes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK201]\nplacement = \"adjacent\"\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "all:\n\t@:\nclean:\n\t@:\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "--fix", "--unsafe-fixes", "Makefile"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("Makefile")).unwrap(),
        ".PHONY: all\nall:\n\t@:\n.PHONY: clean\nclean:\n\t@:\n"
    );
}

#[test]
fn check_without_paths_scans_the_current_directory() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Makefile"), "clean:\n\ttrue\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Makefile:1:1"));
}

#[test]
fn discovery_respects_gitignore_with_a_cli_escape_hatch() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".git")).unwrap();
    std::fs::write(directory.path().join(".gitignore"), "ignored.mk\n").unwrap();
    std::fs::write(directory.path().join("ignored.mk"), "all:\n    true\n").unwrap();

    let respected = rumk()
        .current_dir(directory.path())
        .args(["check", "--output-format", "json"])
        .output()
        .unwrap();
    let overridden = rumk()
        .current_dir(directory.path())
        .args([
            "check",
            "--respect-gitignore=false",
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(respected.status.code(), Some(0));
    assert!(serde_json::from_slice::<Value>(&respected.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(overridden.status.code(), Some(1));
}

#[test]
fn cli_rule_selection_and_fix_policy_match_rumdl_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let original = ".PHONY: all\nall:\n    true\n";
    std::fs::write(&path, original).unwrap();

    let disabled = rumk()
        .args(["check", "--disable", "MK001", path.to_str().unwrap()])
        .output()
        .unwrap();
    let unfixable = rumk()
        .args(["fmt", "--unfixable", "MK001", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(disabled.status.code(), Some(0));
    assert_eq!(unfixable.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn init_creates_a_valid_config_and_refuses_to_overwrite_it() {
    for filename in [".rumk.toml", "pyproject.toml"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(filename);

        let created = rumk()
            .args(["init", "--output", path.to_str().unwrap()])
            .output()
            .unwrap();
        let repeated = rumk()
            .args(["init", "--output", path.to_str().unwrap()])
            .output()
            .unwrap();

        assert_eq!(created.status.code(), Some(0));
        assert!(Config::from_file(&path).is_ok());
        assert_eq!(repeated.status.code(), Some(2));
    }
}

#[cfg(unix)]
#[test]
fn init_does_not_follow_a_dangling_symlink() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("untouched.toml");
    let link = directory.path().join(".rumk.toml");
    std::os::unix::fs::symlink(&destination, &link).unwrap();
    let output = rumk()
        .arg("init")
        .arg("--output")
        .arg(&link)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!destination.exists());
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn explicit_files_bypass_include_filters_but_not_excludes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("custom.mk");
    std::fs::write(&path, "all:\n    true\n").unwrap();

    let included = rumk()
        .current_dir(directory.path())
        .args(["check", "--include", "somewhere-else/**", "custom.mk"])
        .output()
        .unwrap();
    let excluded = rumk()
        .current_dir(directory.path())
        .args(["check", "--exclude", "custom.mk", "custom.mk"])
        .output()
        .unwrap();

    assert_eq!(included.status.code(), Some(1));
    assert_eq!(excluded.status.code(), Some(0));
}

#[test]
fn project_diagnostics_point_to_included_files_and_are_deduplicated() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "include shared.mk\nserver: first\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("shared.mk"), "server:: second\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", ".", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mixed = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["rule"] == "MK004")
        .collect::<Vec<_>>();

    assert_eq!(mixed.len(), 1);
    assert_eq!(mixed[0]["file"], "Makefile");
    assert_eq!(mixed[0]["line"], 2);
}

#[test]
fn required_include_diagnostics_respect_optional_and_generated_makefiles() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        concat!(
            "include missing.mk\n",
            "-include optional.mk\n",
            "include generated.mk\n",
            "generated.mk:\n\t@touch $@\n",
        ),
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let missing = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["rule"] == "MK206")
        .collect::<Vec<_>>();

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["file"], "Makefile");
    assert!(missing[0]["message"]
        .as_str()
        .unwrap()
        .contains("missing.mk"));
}

#[test]
fn project_configuration_drives_include_search_and_opt_in_semantics() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("src")).unwrap();
    std::fs::create_dir_all(directory.path().join("mk")).unwrap();
    std::fs::write(
        directory.path().join("src/Makefile"),
        concat!(
            "include shared.mk\n",
            "EXTRA := $(wildcard optional/*.mk)\n",
            "include $(EXTRA)\n",
            "RESULT := $(FROM_CLI) $(MISSING)\n",
            "all: library\n\t@echo $(RECIPE_PARAMETER)\n",
            "orphan:\n",
        ),
    )
    .unwrap();
    std::fs::write(directory.path().join("mk/shared.mk"), "library:\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        concat!(
            "[global]\n",
            "include-paths = [\"mk\"]\n",
            "predefined-variables = { FROM_CLI = \"yes\" }\n",
            "entry-targets = [\"all\"]\n",
            "[MK208]\n",
            "enabled = true\n",
            "[MK209]\n",
            "enabled = true\n",
            "[MK210]\n",
            "enabled = true\n",
        ),
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "src/Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = diagnostics.as_array().unwrap();

    assert!(!diagnostics.iter().any(|item| item["rule"] == "MK206"));
    assert!(diagnostics.iter().any(
        |item| item["rule"] == "MK208" && item["message"].as_str().unwrap().contains("MISSING")
    ));
    assert!(!diagnostics.iter().any(|item| {
        item["rule"] == "MK208" && item["message"].as_str().unwrap().contains("FROM_CLI")
    }));
    assert!(diagnostics.iter().any(|item| {
        item["rule"] == "MK209" && item["message"].as_str().unwrap().contains("orphan")
    }));
    assert!(diagnostics.iter().any(|item| {
        item["rule"] == "MK210" && item["message"].as_str().unwrap().contains("wildcard")
    }));
}

#[test]
fn included_phony_declarations_prevent_standalone_false_positives() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "include shared.mk\nall:\n\t@:\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("shared.mk"), ".PHONY: all\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", ".", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(!diagnostics
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["rule"] == "MK201"));
}

#[test]
fn explicit_roots_lint_unselected_includes_without_fixing_them() {
    let directory = tempfile::tempdir().unwrap();
    let shared = directory.path().join("shared.mk");
    std::fs::write(directory.path().join("Makefile"), "include shared.mk\n").unwrap();
    std::fs::write(&shared, "target:\n    echo wrong\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--fix", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let recipe = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK001")
        .unwrap();

    assert_eq!(recipe["file"], "shared.mk");
    assert_eq!(recipe["fixable"], false);
    assert_eq!(
        std::fs::read_to_string(shared).unwrap(),
        "target:\n    echo wrong\n"
    );
}

#[test]
fn json_carries_the_edit_of_a_project_aware_fix_for_the_checked_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "include shared.mk\nall:\n\t@:\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("shared.mk"), "helper:\n\t@:\n").unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let phony = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK201" && diagnostic["file"] == "Makefile")
        .unwrap();

    // The fix is reported so an editor can offer it, and marked as one this run
    // did not apply because it can change what Make does.
    assert_eq!(phony["fixable"], false);
    assert_eq!(phony["fix"]["applicability"], "unsafe");
    assert_eq!(phony["fix"]["replacement"], ".PHONY: all\n");
    assert_eq!(phony["fix"]["range"]["start"], 18);
    assert_eq!(phony["fix"]["range"]["end"], 18);
}

#[test]
fn json_marks_a_fix_make_cannot_tell_apart_as_safe() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n    echo hi\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let indentation = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK001")
        .unwrap();

    assert_eq!(indentation["fixable"], true);
    assert_eq!(indentation["fix"]["applicability"], "safe");
}

#[test]
fn per_file_ignores_apply_to_project_diagnostic_source_paths() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "server:: first\ninclude shared.mk\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("shared.mk"), "server: second\n").unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[per-file-ignores]\n\"shared.mk\" = [\"MK004\"]\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(!diagnostics
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["rule"] == "MK004"));
}

/// Strips every permission from `path` and reports whether that made it
/// unreadable; root reads a file regardless of its mode, and then there is
/// nothing to test.
#[cfg(unix)]
fn make_unreadable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::read(path).is_err()
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_reported_without_stopping_the_run() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    let locked = directory.path().join("aaa.mk");
    std::fs::write(&locked, "all:\n\ttrue\n").unwrap();
    std::fs::write(
        directory.path().join("zzz.mk"),
        ".PHONY: clean\nclean:\n    rm -f out\n",
    )
    .unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "."])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout.contains("aaa.mk:1:1: [MK007] File could not be read: Permission denied"),
        "{stdout}"
    );
    assert!(stdout.contains("zzz.mk:3:1: [MK001]"), "{stdout}");
    assert!(
        stdout.contains("Found 2 issues in 2 files (3 files checked)"),
        "{stdout}"
    );
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_fails_every_command_unless_its_severity_is_lowered() {
    let directory = tempfile::tempdir().unwrap();
    let locked = directory.path().join("Makefile");
    std::fs::write(&locked, "all:\n\ttrue\n").unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    for args in [
        &["check", "--fail-on", "never", "Makefile"][..],
        &["fmt", "Makefile"],
        &["fmt", "--check", "Makefile"],
    ] {
        let output = rumk()
            .current_dir(directory.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
    }

    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK007]\nseverity = \"warning\"\n",
    )
    .unwrap();
    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "--fail-on", "error", "Makefile"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Makefile:1:1: [MK007]"));
}

#[cfg(unix)]
#[test]
fn a_disabled_mk007_skips_an_unreadable_file_with_a_warning() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    let locked = directory.path().join("locked.mk");
    std::fs::write(&locked, "all:\n\ttrue\n").unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "--disable", "MK007", "."])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found in 1 file"));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("warning: locked.mk could not be read and MK007 is disabled: Permission denied"));

    let silent = rumk()
        .current_dir(directory.path())
        .args(["check", "--silent", "--disable", "MK007", "."])
        .output()
        .unwrap();

    assert_eq!(silent.status.code(), Some(0));
    assert!(silent.stdout.is_empty());
    assert!(
        silent.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&silent.stderr)
    );
}

#[cfg(unix)]
#[test]
fn an_mk007_ignored_for_a_path_skips_it_without_a_warning() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[per-file-ignores]\n\"locked.mk\" = [\"MK007\"]\n",
    )
    .unwrap();
    let locked = directory.path().join("locked.mk");
    std::fs::write(&locked, "all:\n\ttrue\n").unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "."])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found in 1 file"));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn a_makefile_below_an_unsearchable_directory_is_reported_as_mk007() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let locked = directory.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("Makefile"), "all:\n\ttrue\n").unwrap();
    std::fs::write(
        directory.path().join("other.mk"),
        ".PHONY: clean\nclean:\n    rm -f out\n",
    )
    .unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::metadata(locked.join("Makefile")).is_ok();
    let restore = |directory: &std::path::Path| {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    };
    if readable {
        restore(&locked);
        return;
    }

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "locked/Makefile", "other.mk"])
        .output()
        .unwrap();
    restore(&locked);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(1));
    // Rumk may not even ask what this path is, so the diagnostic does not call
    // it a file.
    assert!(
        stdout.contains("locked/Makefile:1:1: [MK007] Path could not be read: Permission denied"),
        "{stdout}"
    );
    assert!(stdout.contains("other.mk:3:1: [MK001]"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_reported_without_stopping_the_walk() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let locked = directory.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("Makefile"), "all:\n\ttrue\n").unwrap();
    std::fs::write(
        directory.path().join("other.mk"),
        ".PHONY: clean\nclean:\n    rm -f out\n",
    )
    .unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&locked).is_ok();
    let restore = || {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    };
    if readable {
        restore();
        return;
    }

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "."])
        .output()
        .unwrap();
    let disabled = rumk()
        .current_dir(directory.path())
        .args(["check", "--disable", "MK007", "."])
        .output()
        .unwrap();
    restore();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(1));
    // The failure is the operating system's, not the walker's wrapper text
    // repeating the path that already prefixes the diagnostic.
    assert!(
        stdout.contains(
            "locked:1:1: [MK007] Directory could not be read, so any Makefile in it was \
             missed: Permission denied"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("other.mk:3:1: [MK001]"), "{stdout}");
    // The directory is reported, but it is not a file Rumk checked.
    assert!(
        stdout.contains("Found 2 issues in 2 files (1 file checked)"),
        "{stdout}"
    );

    assert_eq!(disabled.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&disabled.stderr)
        .contains("warning: locked could not be read and MK007 is disabled: Permission denied"));
}

#[cfg(unix)]
#[test]
fn an_excluded_unreadable_directory_is_not_reported() {
    use std::os::unix::fs::PermissionsExt;

    for exclude in ["locked", "locked/**", "**/locked/**"] {
        let directory = tempfile::tempdir().unwrap();
        let locked = directory.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("Makefile"), "all:\n\ttrue\n").unwrap();
        std::fs::write(
            directory.path().join("Makefile"),
            ".PHONY: all\nall:\n\ttrue\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join(".rumk.toml"),
            format!("[global]\nexclude = [\"{exclude}\"]\n"),
        )
        .unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&locked).is_ok();
        let restore = || {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        };
        if readable {
            restore();
            return;
        }

        let output = rumk()
            .current_dir(directory.path())
            .args(["check", "."])
            .output()
            .unwrap();
        restore();
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert_eq!(output.status.code(), Some(0), "{exclude}: {stdout}");
        assert!(stdout.contains("No issues found in 1 file"), "{stdout}");
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn a_path_that_does_not_exist_remains_a_tool_error() {
    let directory = tempfile::tempdir().unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "missing.mk"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is neither a file nor a directory"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_file_that_is_not_utf8_is_linted_lossily_and_never_fixed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let bytes = b".PHONY: all\nall:\n    echo caf\xc3\xa9 \xff\n";
    std::fs::write(&path, bytes).unwrap();

    let fixed = rumk()
        .current_dir(directory.path())
        .args(["check", "--fix", "--output-format", "json", "Makefile"])
        .output()
        .unwrap();
    let diagnostics: Value = serde_json::from_slice(&fixed.stdout).unwrap();
    let diagnostics = diagnostics.as_array().unwrap();
    let recipe = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK001")
        .unwrap();
    let encoding = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["rule"] == "MK007")
        .unwrap();

    assert_eq!(fixed.status.code(), Some(1));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(recipe["fixable"], false);
    assert!(recipe.get("fix").is_none());
    assert_eq!(encoding["severity"], "warning");
    assert_eq!(encoding["line"], 3);
    assert_eq!(encoding["column"], 15);

    let checked = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&checked.stdout);

    assert!(stdout.contains("Makefile:3:1: [MK001]"), "{stdout}");
    assert!(
        stdout.contains("Makefile:3:15: [MK007] File is not valid UTF-8"),
        "{stdout}"
    );
    assert!(!stdout.contains("[*]"), "{stdout}");
    assert!(!stdout.contains("rumk fmt"), "{stdout}");
}

#[test]
fn an_include_that_is_not_valid_utf8_is_still_part_of_the_project() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        ".PHONY: all\ninclude parts.mk\nSOURCES := $(TARGETS)\nall:\n\ttrue\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("parts.mk"),
        b"TARGETS := \xff-build\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "--extend-enable", "MK208", "Makefile"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // GNU Make reads the include as bytes, so it is analyzed rather than
    // reported as unreadable, and the variables it defines are known.
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("No issues found in 1 file"), "{stdout}");

    let checked = rumk()
        .current_dir(directory.path())
        .args(["check", "--extend-enable", "MK208", "."])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&checked.stdout);

    // Checking the include itself still reports how it is encoded.
    assert_eq!(checked.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("parts.mk:1:12: [MK007] File is not valid UTF-8"),
        "{stdout}"
    );
    assert!(!stdout.contains("[MK206]"), "{stdout}");
    assert!(!stdout.contains("[MK208]"), "{stdout}");
}

#[test]
fn a_lossily_linted_file_still_follows_fail_on() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        b".PHONY: all\nall:\n\techo \xff\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join(".rumk.toml"),
        "[MK007]\nseverity = \"error\"\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "--fail-on", "never", "Makefile"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("Makefile:3:7: [MK007] File is not valid UTF-8"),
        "{stdout}"
    );
}

#[test]
fn inline_suppressions_cover_the_utf8_warning() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        b"# rumk-disable MK007\n.PHONY: all\nall:\n\techo \xff\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("No issues found in 1 file"));
}

#[test]
fn a_message_names_a_file_the_way_the_location_beside_it_is_named() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("inc")).unwrap();
    std::fs::write(
        directory.path().join("inc/frag.mk"),
        "include $(CONFIG)/x.mk\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("inc/defs.mk"), "CONFIG := inc\n").unwrap();
    std::fs::write(directory.path().join("inc/x.mk"), "X := 1\n").unwrap();
    std::fs::write(
        directory.path().join("Makefile"),
        "include inc/frag.mk\ninclude inc/defs.mk\n",
    )
    .unwrap();

    let output = rumk()
        .current_dir(directory.path())
        .args(["check", "."])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let report = stdout
        .lines()
        .find(|line| line.contains("[MK206]"))
        .unwrap_or_default();
    let fragment = std::path::Path::new("inc").join("frag.mk");
    let definition = std::path::Path::new("inc").join("defs.mk");
    assert!(
        report.starts_with(&format!("{}:1:1:", fragment.display())),
        "{report}"
    );
    assert!(
        report.contains(&format!("before {}:1 defines it", definition.display())),
        "{report}"
    );
    assert!(
        !report.contains(&directory.path().display().to_string()),
        "{report}"
    );
}

#[test]
fn local_rules_still_check_unselected_includes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("Makefile"), "include shared.mk\n").unwrap();
    std::fs::write(directory.path().join("shared.mk"), "all:\n    echo test\n").unwrap();
    let output = rumk()
        .current_dir(directory.path())
        .args([
            "check",
            "Makefile",
            "--enable",
            "MK001",
            "--output-format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert_eq!(diagnostics[0]["file"], "shared.mk");
    assert_eq!(diagnostics[0]["rule"], "MK001");
    assert_eq!(diagnostics[0]["fixable"], false);
}

#[test]
fn large_directory_checks_match_serial_batches() {
    let directory = tempfile::tempdir().unwrap();
    let names: Vec<_> = (0..160).map(|i| format!("part{i:03}.mk")).collect();
    for (i, name) in names.iter().enumerate() {
        std::fs::write(
            directory.path().join(name),
            format!("target{i}:\n    echo {i}\n"),
        )
        .unwrap();
    }
    let check = |paths: &[String]| {
        let output = rumk()
            .current_dir(directory.path())
            .args([
                "--no-config",
                "check",
                "--enable",
                "MK001",
                "--output-format",
                "json",
            ])
            .args(paths)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<Vec<Value>>(&output.stdout).unwrap()
    };
    let serial: Vec<_> = names.chunks(80).flat_map(check).collect();
    let parallel = check(&[".".to_string()]);
    assert_eq!(parallel.len(), 160);
    assert_eq!(parallel, serial);
}

#[test]
fn external_variable_config_applies_to_disk_and_stdin_without_hiding_typos() {
    use std::io::Write;
    use std::process::Stdio;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("rumk.toml"),
        "[MK208]\nenabled=true\nexternal-variables=['TESTS']\n",
    )
    .unwrap();
    let source = "FLAGS = $(TESTS) $(TSET)\n";
    std::fs::write(directory.path().join("Makefile"), source).unwrap();
    let disk = rumk()
        .current_dir(directory.path())
        .args(["check", "Makefile", "--output-format", "json"])
        .output()
        .unwrap();
    let mut child = rumk()
        .current_dir(directory.path())
        .args([
            "check",
            "-",
            "--stdin-filename",
            "Makefile",
            "--output-format",
            "json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let buffer = child.wait_with_output().unwrap();
    assert_eq!(disk.status.code(), Some(1));
    assert_eq!(disk.stdout, buffer.stdout);
    assert_eq!(disk.stderr, buffer.stderr);
    let diagnostics: Value = serde_json::from_slice(&disk.stdout).unwrap();
    let values = diagnostics.as_array().unwrap();
    assert_eq!(values.len(), 1);
    assert!(values[0]["message"].as_str().unwrap().contains("'TSET'"));
    let shown = rumk()
        .current_dir(directory.path())
        .args(["config", "get", "MK208.external-variables"])
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert!(String::from_utf8(shown.stdout).unwrap().contains("TESTS"));
}
