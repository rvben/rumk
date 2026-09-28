use rumk::project::{Project, ProjectOptions};
use rumk::rules::prerequisites::{coverage, MissingPrerequisite};
use rumk::rules::Rule;

#[test]
fn unresolved_explicit_prerequisites_are_local_to_their_declaration() {
    for declaration in [
        "optional: $(EXTRA_INPUTS) hidden.txt\n",
        "optional: | ${EXTRA_INPUTS} hidden.txt\n",
        "optional another: $(EXTRA_INPUTS)\n",
        "INPUTS = $(EXTRA_INPUTS)\nMORE = $(INPUTS)\noptional: $(MORE)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rules.mk"), declaration).unwrap();
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            "include rules.mk\nprobe: missing.txt optional\nEXTRA_INPUTS = later.txt\n".into(),
            &ProjectOptions::default(),
        )
        .unwrap();
        let report = coverage(&project);
        assert!(
            report.root_blockers.is_empty(),
            "{declaration}: {:?}",
            report.root_blockers
        );
        assert_eq!(report.local_exclusions.len(), 1);
        assert!(report.local_exclusions[0].source.ends_with("rules.mk"));
        assert_eq!(report.local_exclusions[0].line, declaration.lines().count());
        let diagnostics = MissingPrerequisite.check_project(&project);
        assert_eq!(diagnostics.len(), 1, "{declaration}: {diagnostics:?}");
        assert!(diagnostics[0].message.contains("missing.txt"));
        assert_eq!(diagnostics[0].line, 2);
    }
}

#[test]
fn local_unknown_inputs_keep_possible_builtin_producers() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("Makefile"),
        "generated.c: $(EXTRA_INPUTS)\nprobe: generated.o\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    let report = coverage(&project);
    assert!(report.root_blockers.is_empty());
    assert_eq!(report.local_exclusions.len(), 1);
    assert_eq!(report.outcomes.get("possible_builtin_input"), Some(&1));
    assert!(MissingPrerequisite.check_project(&project).is_empty());
}

#[test]
fn uncertainty_that_can_add_producers_remains_global() {
    for prefix in [
        "$(EXTRA_TARGETS):\n",
        "%.txt: $(EXTRA_INPUTS)\n",
        ".PHONY: $(EXTRA_TARGETS)\n",
        "include $(EXTRA_MAKEFILES)\n",
        "ifeq ($(MODE),yes)\nmissing.txt:;\nendif\n",
        "INPUTS = $(EXTRA_INPUTS)\n$(INPUTS):\n",
        "INPUTS = $(EXTRA_INPUTS)\noptional: $(INPUTS)\nVPATH = $(INPUTS)\n",
        "optional: $(shell touch sentinel)\n",
        ".SECONDEXPANSION:\noptional: $$(EXTRA_INPUTS)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("{prefix}probe: missing.txt\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(!coverage(&project).root_blockers.is_empty(), "{prefix}");
        assert!(
            MissingPrerequisite.check_project(&project).is_empty(),
            "{prefix}"
        );
        assert!(!dir.path().join("sentinel").exists());
    }
}

#[test]
fn comments_and_function_prefix_variables_do_not_obscure_static_inputs() {
    for prefix in [
        "# $(shell touch sentinel) ${eval probe:} $(file >sentinel,x)\n",
        "VALUE = literal # $(shell touch sentinel)\n",
        "shell_flags = literal\nevaluation = literal\nfilename = literal\nVALUE := $(shell_flags) ${evaluation} $(filename)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("{prefix}probe: missing.txt\n"),
            &ProjectOptions::default(),
        ).unwrap();
        assert!(coverage(&project).root_blockers.is_empty(), "{prefix}: {:?}", coverage(&project).root_blockers);
        assert_eq!(MissingPrerequisite.check_project(&project).len(), 1, "{prefix}");
        assert!(!dir.path().join("sentinel").exists());
    }
}

#[test]
fn function_exclusions_keep_nested_deferred_and_recipe_expansions() {
    for (prefix, reason) in [
        ("VALUE := $(shell touch sentinel)\n", "shell_function"),
        ("VALUE := ${shell\ttouch sentinel}\n", "shell_function"),
        (
            "VALUE = $(if yes,$(shell touch sentinel))\n",
            "shell_function",
        ),
        ("VALUE = $$(shell touch sentinel)\n", "shell_function"),
        ("VALUE = \\# $(shell touch sentinel)\n", "shell_function"),
        ("other:; # $(shell touch sentinel)\n", "shell_function"),
        ("other:\n\t# $(shell touch sentinel)\n", "shell_function"),
        (
            "define VALUE\n# $(shell touch sentinel)\nendef\n",
            "shell_function",
        ),
        ("VALUE = $(eval probe: generated.txt)\n", "eval_function"),
        ("VALUE = ${file >sentinel,x}\n", "file_function"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("{prefix}probe: missing.txt\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(
            coverage(&project).root_blockers.contains_key(reason),
            "{prefix}"
        );
        assert!(
            MissingPrerequisite.check_project(&project).is_empty(),
            "{prefix}"
        );
        assert!(!dir.path().join("sentinel").exists());
    }
}

#[test]
fn relative_file_targets_do_not_exclude_roots_as_suffix_rules() {
    let dir = tempfile::tempdir().unwrap();
    for target in [
        "../lib/library.a",
        "../../tests/helper",
        "./build/library.a",
        ".cache/library.a",
    ] {
        let source = format!("probe: missing.txt\n{target}:;\n");
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            source,
            &ProjectOptions::default(),
        )
        .unwrap();
        let report = coverage(&project);
        assert!(
            report.root_blockers.is_empty(),
            "{target}: {:?}",
            report.root_blockers
        );
        assert_eq!(report.outcomes.get("missing"), Some(&1));
        assert_eq!(MissingPrerequisite.check_project(&project).len(), 1);
    }
    for rule in [
        ".c.o: extra\n",
        ".tar.gz.o:;\n",
        ".SUFFIXES: input output\ninputoutput:;\n",
        ".SUFFIXES: .dir/input .o\n.dir/input.o:;\n",
    ] {
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("probe: missing.txt\n{rule}"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(
            coverage(&project).root_blockers.contains_key("suffix_rule"),
            "{rule}"
        );
        assert!(MissingPrerequisite.check_project(&project).is_empty());
    }
}

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

#[test]
fn static_pattern_coverage_reports_concrete_per_target_inputs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.c"), "").unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("Makefile"),
        "PATTERN = %.o\na.o b.o: $(PATTERN): %.c | setup\nPATTERN = %.other\nsetup:;\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    let report = coverage(&project);
    assert!(
        report.root_blockers.is_empty(),
        "{:?}",
        report.root_blockers
    );
    assert!(report.local_exclusions.is_empty());
    assert_eq!(report.outcomes.get("missing"), Some(&1));
    assert!(report.edges.iter().any(|edge| edge.target == "b.o"
        && edge.prerequisite == "b.c"
        && edge.outcome == "missing"));
    assert_eq!(
        report
            .edges
            .iter()
            .filter(|edge| edge.order_only
                && edge.prerequisite == "setup"
                && edge.outcome == "declared_target")
            .count(),
        2
    );
}

#[test]
fn repeated_static_pattern_includes_keep_ambiguous_edge_provenance_opaque() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rules.mk"), "a.o: $(P): %.c\n").unwrap();
    let project = Project::load_with_root_content(
        &dir.path().join("Makefile"),
        "P := %.o\ninclude rules.mk\nP := a.%\ninclude rules.mk\nprobe: missing.txt\n".into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    let report = coverage(&project);
    assert!(report.root_blockers.contains_key("static_pattern_rule"));
    assert!(MissingPrerequisite.check_project(&project).is_empty());
}

#[test]
fn uncertain_recipe_only_settings_do_not_hide_independent_prerequisites() {
    for conditional in [
        "ifneq (,$(filter Windows%,$(OS)))\nEXT = .exe\nelse\nEXT =\nendif\n",
        "ifdef MODE\nEXT = .exe\nelse ifdef OTHER\nEXT = .bin\nendif\n",
        "ifdef MODE\nFLAGS = $(OPTIONAL_FLAGS)\nendif\nLOCAL = $(FLAGS)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("{conditional}probe: missing.txt\n\t@echo $(LOCAL) $@$(EXT)\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        let report = coverage(&project);
        assert!(
            report.root_blockers.is_empty(),
            "{conditional}: {:?}",
            report.root_blockers
        );
        assert_eq!(MissingPrerequisite.check_project(&project).len(), 1);
    }
}

#[test]
fn uncertain_graph_changes_and_side_effects_remain_blockers() {
    for body in [
        "missing.txt:;\n",
        "VPATH = inputs\n",
        "EXT = .exe\n",
        "FLAGS := $(shell touch sentinel)\n",
        "FLAGS != touch sentinel\n",
        "FLAGS := $(eval missing.txt:;)\n",
        "FLAGS := $(file >sentinel,x)\n",
        "FLAGS := $(guile (display 1))\n",
        "include extra.mk\n",
        ".RECIPEPREFIX = >\n",
        "undefine INPUT\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let target = if body == "EXT = .exe\n" {
            "probe$(EXT)"
        } else {
            "probe"
        };
        let project = Project::load_with_root_content(
            &dir.path().join("Makefile"),
            format!("ifdef MODE\n{body}endif\n{target}: missing.txt\n"),
            &ProjectOptions::default(),
        )
        .unwrap();
        assert!(!coverage(&project).root_blockers.is_empty(), "{body}");
        assert!(
            MissingPrerequisite.check_project(&project).is_empty(),
            "{body}"
        );
        assert!(!dir.path().join("sentinel").exists());
    }
}
