use rumk::diagnostic::Severity;
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
        Some(dunce::canonicalize(&root).unwrap().as_path())
    );
}

#[test]
fn missing_phony_only_offers_fixes_for_the_processed_root_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(&root, "include shared.mk\nall clean:\n\t@:\n").unwrap();
    std::fs::write(&shared, "test:\n\t@:\n").unwrap();

    let diagnostics = MissingPhony::default().check_project(&load(&root));
    let root_path = dunce::canonicalize(&root).unwrap();
    let shared_path = dunce::canonicalize(&shared).unwrap();

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
fn include_comments_are_not_treated_as_missing_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::write(&root, "include shared.mk # generated settings\n").unwrap();
    std::fs::write(&shared, "VALUE := yes\n").unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert!(diagnostics.is_empty());
}

#[test]
fn reports_an_include_read_before_the_variable_it_expands_is_defined() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "VALUE := yes\n").unwrap();
    std::fs::write(&root, "include $(DIR)/x.mk\nDIR := sub\n").unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK206");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].line, 1);
    assert!(diagnostics[0]
        .message
        .contains("expands 'DIR' before line 2 defines it"));
    assert!(diagnostics[0].message.contains("cannot find '/x.mk'"));
}

#[test]
fn reports_an_include_that_reads_nothing_because_its_variable_is_defined_below_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(directory.path().join("extra.mk"), "VALUE := yes\n").unwrap();
    std::fs::write(&root, "include $(FILES)\nFILES := extra.mk\n").unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0]
        .message
        .contains("expands 'FILES' before line 2 defines it, so Make reads no file at all"));
}

#[test]
fn reports_an_include_read_before_a_branch_that_may_define_its_variable() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "include $(DIR)/x.mk\nifdef FEATURE\nDIR := sub\nendif\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0]
        .message
        .contains("expands 'DIR' before line 3 defines it"));
}

#[test]
fn names_the_file_a_later_definition_is_read_from() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let settings = directory.path().join("settings.mk");
    std::fs::write(&settings, "DIR := sub\n").unwrap();
    std::fs::write(&root, "include $(DIR)/x.mk\ninclude settings.mk\n").unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0].message.contains(&format!(
        "before {}:1 defines it",
        dunce::canonicalize(&settings).unwrap().display()
    )));
}

#[test]
fn says_nothing_about_an_include_whose_variable_a_caller_supplies() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // A fragment written to be included with a directory the caller names
    // reads exactly like this, and `TOPDIR=inc make` reads the file. Rumk
    // reads neither the environment nor a parent make, so MK206 says nothing.
    std::fs::write(&root, "include $(TOPDIR)/rules.mk\n").unwrap();

    let project = load(&root);

    assert!(
        MissingInclude.check_project(&project).is_empty(),
        "{:?}",
        MissingInclude.check_project(&project)
    );
    // Opt-in MK208 is where a name no one in the project defines is reported.
    let expected = UndefinedVariableReference::default().check_project(&project);
    assert_eq!(expected.len(), 1, "{expected:?}");
    assert!(expected[0].message.contains("'TOPDIR'"));
}

#[test]
fn says_nothing_about_an_include_whose_variable_the_project_only_restates() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "NDK_ROOT := $(strip $(NDK_ROOT))\nBUILD := $(NDK_ROOT)/core\ninclude $(BUILD)/rules.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    // Reading the name back and writing it again takes the value from
    // wherever the caller put it, so the project defines nothing here.
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn warns_when_the_only_definition_is_one_a_rule_carries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "build: DIR := sub\ninclude $(DIR)/x.mk\nbuild:\n\t@:\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    // Make holds a target-specific value only while that rule runs, and reads
    // the include long before, but the project does say what the name is worth.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("which has no value there"));
}

#[test]
fn warns_when_the_only_definition_sits_in_a_branch_make_never_takes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "ifeq (1,2)\nDIR := sub\nendif\ninclude $(DIR)/x.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("which has no value there"));
}

#[test]
fn does_not_claim_a_variable_is_undefined_where_the_project_defines_it_elsewhere() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "VALUE := yes\n").unwrap();
    std::fs::write(directory.path().join("shared.mk"), "include $(DIR)x.mk\n").unwrap();
    std::fs::write(
        &root,
        // The second reading of shared.mk finds DIR without a value again,
        // which is not the same as the project never giving it one.
        "DIR := sub/\ninclude shared.mk\nundefine DIR\ninclude shared.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0]
        .message
        .contains("expands 'DIR', which has no value there"));
    assert!(!diagnostics[0]
        .message
        .contains("nothing in this project defines"));
}

#[test]
fn does_not_blame_a_definition_make_read_before_it_reached_the_include() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        directory.path().join("shared.mk"),
        "ifdef ENABLE\ninclude $(DIR)/x.mk\nendif\n",
    )
    .unwrap();
    std::fs::write(
        &root,
        // The reading that reaches the include is the second one, by which
        // point Make has read the definition of DIR and the undefine that
        // takes it back. Nothing about it is still to come.
        "include shared.mk\nDIR := sub\nENABLE := 1\nundefine DIR\ninclude shared.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0]
        .message
        .contains("expands 'DIR', which has no value there"));
    assert!(!diagnostics[0].message.contains("defines it"));
}

#[test]
fn reports_an_include_whose_variable_waits_on_a_name_defined_further_down() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // DIR is assigned before the include and still has nothing in it, because
    // the name it was computed from is only given a value below.
    std::fs::write(
        &root,
        "DIR := $(LATER)\ninclude $(DIR)/x.mk\nLATER := sub\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        "Required include '$(DIR)/x.mk' expands 'LATER' before line 3 defines it, \
         so Make cannot find '/x.mk'"
    );
}

#[test]
fn reports_an_include_whose_variable_was_computed_before_the_name_it_needed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("config.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/config.mk"), "WHICH := sub\n").unwrap();
    // LATER is given a value before the include, but FILES was computed above
    // it, so Make reads the file in this directory rather than the one in sub.
    std::fs::write(
        &root,
        "FILES := $(LATER)config.mk\nLATER := sub/\ninclude $(FILES)\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        "Required include '$(FILES)' expands 'LATER' before line 2 defines it, \
         so Make reads 'config.mk' instead"
    );
}

#[test]
fn reports_an_include_reading_a_name_given_back_before_it_is_defined_again() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("config.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/config.mk"), "WHICH := sub\n").unwrap();
    // Make read the definition on line 1 and gave the value back, so the
    // definition it has not reached here is the one on line 4, and it reads
    // the file in this directory rather than the one in sub.
    std::fs::write(
        &root,
        "DIR := old/\nundefine DIR\ninclude $(DIR)config.mk\nDIR := sub/\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        "Required include '$(DIR)config.mk' expands 'DIR' before line 4 defines it, \
         so Make reads 'config.mk' instead"
    );
}

#[test]
fn reports_an_include_a_second_reading_of_the_same_file_leaves_behind() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("x.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "WHICH := sub\n").unwrap();
    std::fs::write(
        directory.path().join("shared.mk"),
        "include $(DIR)x.mk\nDIR := sub/\n",
    )
    .unwrap();
    // Make reads shared.mk twice. The first reading has a value for DIR, the
    // second does not, and line 2 of shared.mk gives it one both times: on the
    // second reading that is a definition Make has not reached again yet.
    std::fs::write(
        &root,
        "DIR := sub/\ninclude shared.mk\nundefine DIR\ninclude shared.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        "Required include '$(DIR)x.mk' expands 'DIR' before line 2 defines it, \
         so Make reads 'x.mk' instead"
    );
}

#[test]
fn reports_an_include_a_file_read_before_and_after_it_defines_the_name_for() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let shared = directory.path().join("shared.mk");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("x.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "WHICH := sub\n").unwrap();
    std::fs::write(&shared, "DIR := sub/\n").unwrap();
    // Make reads the same definition twice. The reading on line 1 is one it
    // gave back, so what the include on line 3 waits on is the reading line 4
    // brings, which is that same line of shared.mk read again.
    std::fs::write(
        &root,
        "include shared.mk\nundefine DIR\ninclude $(DIR)x.mk\ninclude shared.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        format!(
            "Required include '$(DIR)x.mk' expands 'DIR' before {}:1 defines it, \
             so Make reads 'x.mk' instead",
            dunce::canonicalize(&shared).unwrap().display()
        )
    );
}

#[test]
fn reports_an_include_behind_the_earliest_assignment_that_waited_on_a_name() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(directory.path().join("config.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("prefixconfig.mk"), "WHICH := pre\n").unwrap();
    // Both assignments read X while it had no value. Line 4 read it after the
    // definition on line 2, but line 1 read it before, and the include is
    // behind that reading as much as the other.
    std::fs::write(
        &root,
        "A := $(X)\nX := prefix\nundefine X\nB := $(X)\ninclude $(B)$(A)config.mk\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        diagnostics[0].message,
        "Required include '$(B)$(A)config.mk' expands 'X' before line 2 defines it, \
         so Make reads 'config.mk' instead"
    );
}

#[test]
fn says_nothing_about_an_include_no_valueless_name_reaches() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // `flavor` asks what DIR is, not what it holds, and answers 'simple'
    // whatever LATER turns out to be. A name without a value is therefore not
    // why Rumk cannot expand this include, and MK206 has nothing to say.
    std::fs::write(
        &root,
        "DIR := $(LATER)\ninclude $(flavor DIR)/x.mk\nLATER := sub\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn undefined_include_variables_are_left_alone_where_make_reads_a_file_anyway() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(directory.path().join("config.mk"), "VALUE := yes\n").unwrap();
    std::fs::write(
        &root,
        concat!(
            // Make reads config.mk, which is what an unset prefix is for.
            "include $(EXTRA)config.mk\n",
            // Make reads nothing, which is what an unset file list is for.
            "include $(FILES)\n",
            // Make finds no file for this one, but the include is optional.
            "-include $(DIR)/x.mk\n",
            // A branch above may already have given the variable a value.
            "ifdef FEATURE\nLOCAL := sub\nendif\ninclude $(LOCAL)config.mk\n",
            // Make defines CURDIR itself, so this reads a real directory.
            "include $(CURDIR)/config.mk\n",
            // A target-specific value is not one Make reads makefiles with.
            "include $(SCOPED)config.mk\n",
            "build: SCOPED := sub/\nbuild:\n\t@:\n",
        ),
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn names_the_file_make_reads_when_the_project_knows_how_to_build_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "include $(DIR)generated.mk\nDIR := sub/\ngenerated.mk:\n\t@touch $@\n",
    )
    .unwrap();

    let diagnostics = MissingInclude.check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]
        .message
        .contains("expands 'DIR' before line 2 defines it, so Make reads 'generated.mk' instead"));
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
        Some(dunce::canonicalize(&shared).unwrap().as_path())
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
fn reports_an_assignment_read_before_the_definition_it_needs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // GNU Make expands ':=' where it is written, so OUT is '/app'.
    std::fs::write(&root, "OUT := $(BUILD)/app\nBUILD := build\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].rule_id, "MK208");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].line, 1);
    assert_eq!(diagnostics[0].column, 8);
    assert_eq!(
        diagnostics[0].message,
        "Variable 'BUILD' is read before line 2 defines it"
    );
}

#[test]
fn reports_an_append_read_before_the_definition_it_needs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // Appending to a simple variable expands the addition where it is written.
    std::fs::write(&root, "OUT := start\nOUT += $(BUILD)\nBUILD := build\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);
    assert!(diagnostics[0]
        .message
        .contains("'BUILD' is read before line 3 defines it"));
}

#[test]
fn names_the_file_the_definition_an_assignment_was_read_before_is_in() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    let settings = directory.path().join("settings.mk");
    std::fs::write(&settings, "BUILD := build\n").unwrap();
    std::fs::write(&root, "OUT := $(BUILD)/app\ninclude settings.mk\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains(&format!(
        "is read before {}:1 defines it",
        dunce::canonicalize(&settings).unwrap().display()
    )));
}

#[test]
fn says_nothing_about_a_value_make_expands_only_when_it_is_used() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // A recursive assignment reads BUILD when OUT is used, by which time
    // Make has read line 2, so 'build/app' is what it holds.
    std::fs::write(&root, "OUT = $(BUILD)/app\nBUILD := build\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn says_nothing_where_the_definition_below_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // Reading BUILD above line 2 produces exactly what reading it below does.
    std::fs::write(&root, "OUT := $(BUILD)/app\nBUILD :=\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn says_nothing_where_the_definition_below_only_restates_the_caller() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // A fragment that transforms a value its caller supplies reads like this,
    // and line 2 gives BUILD nothing the caller did not already give it.
    std::fs::write(&root, "OUT := $(BUILD)/app\nBUILD := $(BUILD)x\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn reports_a_name_with_no_definition_make_certainly_reads_only_once() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(
        &root,
        "OUT := $(BUILD)/app\nifdef FEATURE\nBUILD := build\nendif\n",
    )
    .unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    // Make may never read line 3, so this is a name with no definition rather
    // than one read too early, and it is reported once either way.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].message,
        "Variable 'BUILD' is referenced but not defined"
    );
}

#[test]
fn says_nothing_where_a_branch_leaves_the_definition_below_restating_the_caller() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // Line 3 leaves EXT indeterminate, so what line 5 writes is unknown: it may
    // be exactly the value the caller supplied, which is what line 1 reads.
    std::fs::write(
        &root,
        "BAD := $(strip $(EXT))\nifdef BAD\nEXT := other\nendif\nEXT := $(strip $(EXT))\n",
    )
    .unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn reports_a_definition_a_branch_left_indeterminate_through_another_name() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // Line 3 leaves Y indeterminate, but line 5 writes X from Y rather than from
    // X, so it gives X a value line 1 could not have read.
    std::fs::write(&root, "A := $(X)\nifdef Q\nY := one\nendif\nX := $(Y)\n").unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    // Y itself has no definition Make certainly reads, which is the other thing
    // the rule reports.
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 5);
    assert_eq!(
        diagnostics[0].message,
        "Variable 'Y' is referenced but not defined"
    );
    assert_eq!(diagnostics[1].line, 1);
    assert_eq!(
        diagnostics[1].message,
        "Variable 'X' is read before line 5 defines it"
    );
}

#[test]
fn names_the_definition_make_certainly_reads_past_one_it_may_not() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    // Make may never read line 3, but it certainly reads line 5, so OUT is
    // '/app' whichever way the condition goes.
    std::fs::write(
        &root,
        "OUT := $(BUILD)/app\nifdef FEATURE\nBUILD := debug\nendif\nBUILD := build\n",
    )
    .unwrap();

    let diagnostics = UndefinedVariableReference::default().check_project(&load(&root));

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
    assert_eq!(
        diagnostics[0].message,
        "Variable 'BUILD' is read before line 5 defines it"
    );
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

    assert!(MissingPhony::default().check_project(&project).is_empty());
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
    assert_eq!(diagnostics[0].severity, Severity::Info);
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
