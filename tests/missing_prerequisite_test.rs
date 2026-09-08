use rumk::parser::parse;
use rumk::project::{Project, ProjectOptions};
use rumk::rules::get_all_rules;

fn check(source: &str, files: &[(&str, &str)]) -> Vec<rumk::diagnostic::Diagnostic> {
    let directory = tempfile::tempdir().unwrap();
    for (name, content) in files {
        let path = directory.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    let project = Project::load_with_root_content(
        &directory.path().join("Makefile"),
        source.into(),
        &ProjectOptions::default(),
    )
    .unwrap();
    get_all_rules()
        .into_iter()
        .find(|r| r.id() == "MK216")
        .expect("MK216 registered")
        .check_project(&project)
}

#[test]
fn reports_static_missing_normal_and_order_only_inputs_at_the_declaration() {
    for source in [
        "probe: soruce.txt\n",
        "probe: | soruce.txt\n",
        "INPUT := soruce.txt\nprobe: $(INPUT)\n",
    ] {
        let findings = check(source, &[("source.txt", "input")]);
        assert_eq!(findings.len(), 1, "{source}");
        assert!(findings[0].message.contains("soruce.txt"));
        assert!(findings[0].fix.is_none());
        assert_eq!(findings[0].line, source.lines().count());
    }
    let findings = check(
        "include rules.mk\n",
        &[("rules.mk", "probe: missing.txt\n")],
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].source.as_ref().unwrap().ends_with("rules.mk"));
}

#[test]
fn existing_declared_and_search_path_inputs_are_allowed() {
    for (source, files) in [
        ("probe: input.txt\n", vec![("input.txt", "input")]),
        (
            "probe: generated.txt\ngenerated.txt:\n\t@echo generated\n",
            vec![],
        ),
        ("probe: .//generated.txt\ngenerated.txt:\n", vec![]),
        ("probe: task\n.PHONY: task\n", vec![]),
        (
            "VPATH := inputs\nprobe: input.txt\n",
            vec![("inputs/input.txt", "input")],
        ),
        (
            "VPATH := first:inputs\nprobe: input.txt\n",
            vec![("inputs/input.txt", "input")],
        ),
        (
            "include rules.mk\nprobe: generated.txt\n",
            vec![("rules.mk", "generated.txt:\n")],
        ),
    ] {
        assert!(check(source, &files).is_empty(), "{source}");
    }
}

#[test]
fn implicit_rules_and_incomplete_graphs_withhold_a_warning() {
    for (source, files) in [
        ("probe: input.o\n", vec![("input.c", "int value;\n")]),
        ("probe: input.o\n", vec![("INPUT.c", "int value;\n")]),
        (
            "probe: program\n",
            vec![("program.c", "int main(void){return 0;}\n")],
        ),
        ("probe: input.o\ninput.c:\n\t@echo generated\n", vec![]),
        (
            "probe: output.dat\n%.dat: %.src\n\t@echo generated\n",
            vec![("output.src", "input")],
        ),
        ("probe: output.dat\n.src.dat:\n\t@echo generated\n", vec![]),
        (
            "vpath %.txt inputs\nprobe: input.txt\n",
            vec![("inputs/input.txt", "input")],
        ),
        ("VPATH := $(EXTERNAL)\nprobe: input.txt\n", vec![]),
        (".DEFAULT:\n\t@echo fallback\nprobe: missing\n", vec![]),
        (
            ".SECONDEXPANSION:\nprobe: $$(INPUT)\nINPUT = missing\n",
            vec![],
        ),
        ("include $(EXTERNAL)\nprobe: missing\n", vec![]),
        ("-include generated.mk\nprobe: missing\n", vec![]),
        ("$(EXTERNAL):\nprobe: missing\n", vec![]),
        (
            "ifeq ($(EXTERNAL),yes)\nmissing:\nendif\nprobe: missing\n",
            vec![],
        ),
        ("probe: $(EXTERNAL)\n", vec![]),
        ("probe: *.txt\n", vec![]),
        ("probe: lib.a(member.o)\n", vec![]),
        ("probe: .WAIT\n", vec![]),
        ("VPATH := one;two\nprobe: input.txt\n", vec![]),
        ("$(file >missing,generated)\nprobe: missing\n", vec![]),
        ("probe: -lthing\n", vec![]),
        ("probe: file\n", vec![("file,v", "revision control input")]),
        ("probe: file\n", vec![("s.file", "SCCS input")]),
    ] {
        assert!(check(source, &files).is_empty(), "{source}");
    }
}

#[test]
fn inactive_inputs_and_unconfigured_rules_do_not_report() {
    assert!(check("ifeq (a,b)\nprobe: missing\nendif\nprobe:\n", &[]).is_empty());
    assert!(!rumk::rules::get_default_rules()
        .iter()
        .any(|rule| rule.id() == "MK216"));
    let rule = get_all_rules()
        .into_iter()
        .find(|r| r.id() == "MK216")
        .unwrap();
    assert!(rule
        .check(&parse("probe: missing\n"), "probe: missing\n")
        .is_empty());
}

#[test]
fn unrelated_patterns_do_not_hide_missing_inputs() {
    for pattern in [
        "generated/%.dat: templates/%.src\n\t@echo generated\n",
        "./generated/%.dat: templates/%.src\n\t@echo generated\n",
        ".generated/%.dat: templates/%.src\n\t@echo generated\n",
        "generated/%.dat:: templates/%.src\n\t@echo generated\n",
        "generated/%.dat: intermediate/%.mid\n\t@echo generated\nintermediate/%.mid: templates/%.src\n\t@echo generated\n",
        "generated/%.dat: impossible/%.src\n\t@echo first\ngenerated/%.dat: templates/%.src\n\t@echo second\n",
    ] {
        let source = format!("probe: missing.txt\n{pattern}");
        let findings = check(&source, &[]);
        assert_eq!(findings.len(), 1, "{source}");
        assert_eq!(findings[0].line, 1);
        assert!(findings[0].message.contains("missing.txt"));
    }
}

#[test]
fn matching_patterns_and_builtin_sources_remain_uncertain() {
    for source in [
        "probe: output.o\n%.c: templates/%.src\n\t@echo generated\n",
        "VPATH := generated\nprobe: output.o\ngenerated/%.c: templates/%.src\n\t@echo generated\n",
        "probe: output.o\ns.%: revisions/%\n\t@echo generated\n",
        "probe: missing.txt\n%:\n\t@echo fallback\n",
    ] {
        assert!(check(source, &[]).is_empty(), "{source}");
    }
}

#[test]
fn pattern_prerequisites_are_not_checked_as_literal_root_inputs() {
    assert!(check(
        "probe:\ngenerated/%.dat: shared.txt\n\t@echo generated\n",
        &[]
    )
    .is_empty());
    let findings = check(
        "probe: missing.txt\ninclude patterns.mk\n",
        &[(
            "patterns.mk",
            "generated/%.dat: shared.txt\n\t@echo generated\n",
        )],
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 1);
}

#[test]
fn rejects_matching_producers_with_missing_inputs() {
    for source in [
        "probe: generated/output.dat\ngenerated/%.dat: templates/%.src\n\t@echo generated\n",
        "probe: generated/output.dat\ngenerated/%.dat:: intermediate/%.mid\n\t@echo generated\nintermediate/%.mid: templates/%.src\n\t@echo intermediate\n",
        "probe: generated/output.dat\ngenerated/%.dat: | templates/%.src\n\t@echo generated\n",
        "probe: generated/output.dat\ngenerated/%.dat: absent/%.src\n\t@echo first\ngenerated/%.dat: missing/%.src\n\t@echo second\n",
    ] {
        let findings = check(source, &[("templates/unrelated.src", "input")]);
        assert_eq!(findings.len(), 1, "{source}");
        assert_eq!(findings[0].line, 1);
    }
}

#[test]
fn accepts_viable_alternatives_and_preserves_unsupported_chains() {
    for (source, files) in [
        ("probe: generated/output.dat\ngenerated/%.dat: absent/%.src\n\t@echo first\ngenerated/%.dat: templates/%.src\n\t@echo second\n", vec![("templates/output.src", "input")]),
        ("probe: nested/output.dat\n%.dat: %.src\n\t@echo generated\n", vec![("nested/output.src", "input")]),
        ("VPATH := inputs\nprobe: generated/output.dat\ngenerated/%.dat: templates/%.src\n\t@echo generated\n", vec![("inputs/templates/output.src", "input")]),
        ("probe: generated/output.dat\ngenerated/%.dat:: templates/%.src\n\t@echo generated\ntemplates/output.src:\n\t@echo source\n", vec![]),
        ("probe: generated/output.dat\ngenerated/%.dat: intermediate/%.mid\n\t@echo generated\nintermediate/%.mid: templates/%.src\n\t@echo intermediate\n", vec![]),
        ("probe: generated/output.dat\ngenerated/%.dat: object.o\n\t@echo generated\n", vec![("object.c", "input")]),
    ] {
        assert!(check(source, &files).is_empty(), "{source}");
    }
}

#[test]
fn unsupported_pattern_forms_preserve_uncertainty() {
    for source in [
        "probe: generated/output.dat\ngenerated/%.dat: missing/%.src\n",
        "probe: generated/output.dat\ngenerated/%.dat generated/%.aux: missing/%.src\n\t@echo generated\n",
        "probe: generated/output.dat\ngenerated/%.dat &: missing/%.src\n\t@echo generated\n",
        "probe: generated/output.dat\ngenerated/%.dat: *.src\n\t@echo generated\n",
    ] {
        assert!(check(source, &[]).is_empty(), "{source}");
    }
}

#[test]
fn slashless_patterns_restore_directories_only_for_pattern_inputs() {
    let source = "probe: generated/output.dat\n%.dat:: shared.txt\n\t@echo generated\n";
    assert!(check(source, &[("shared.txt", "input")]).is_empty());
    assert_eq!(check(source, &[("generated/shared.txt", "input")]).len(), 1);
}

#[test]
fn broad_direct_pattern_may_build_a_different_builtin_source() {
    assert!(check(
        "probe: generated/output.o\ngenerated/%: templates/%.src\n\t@echo generated\n",
        &[("templates/output.c.src", "input")]
    )
    .is_empty());
}

#[test]
fn recipe_only_optional_settings_do_not_hide_missing_inputs() {
    for settings in [
        "LOCAL = $(OPTIONAL)\nMYCFLAGS = $(LOCAL) -O2\nCFLAGS = $(MYCFLAGS)\n",
        "ALL_POFILES = $(wildcard *.po)\n",
    ] {
        let source = format!("{settings}probe: missing.txt\n\t@echo $(CFLAGS) $(ALL_POFILES)\n");
        assert_eq!(check(&source, &[]).len(), 1, "{source}");
        assert!(check(&source, &[("missing.txt", "input")]).is_empty());
    }
}

#[test]
fn graph_facing_and_indirect_settings_remain_uncertain() {
    for source in [
        "INPUT = $(OPTIONAL)\nALIAS = $(INPUT)\nprobe: missing.txt $(ALIAS)\n",
        "VALUE = $(OPTIONAL)\nVPATH = $(VALUE)\nprobe: missing.txt\n",
        "VALUE = $(OPTIONAL)\ninclude $(VALUE)\nprobe: missing.txt\n",
        "VALUE = $(OPTIONAL)\nifeq ($(VALUE),yes)\nextra:\nendif\nprobe: missing.txt\n",
        "VALUE = $(OPTIONAL)\n$(NAME) = $(VALUE)\nprobe: missing.txt\n",
        "VALUE = $(OPTIONAL)\n.EXTRA_PREREQS = $(VALUE)\nprobe: missing.txt\n",
        "VALUE = $(shell touch never-created)\nprobe: missing.txt\n",
    ] {
        assert!(check(source, &[]).is_empty(), "{source}");
    }
}

#[test]
fn resolved_phony_lists_do_not_exclude_the_project() {
    for source in [
        ".PHONY: probe\nprobe: missing.txt\n",
        "COMMAND = probe\n.PHONY: $(COMMAND)\nCOMMAND = changed\nprobe: missing.txt\n",
    ] {
        assert_eq!(check(source, &[]).len(), 1, "{source}");
        assert!(check(source, &[("missing.txt", "input")]).is_empty());
    }
    assert!(check(".PHONY: $(UNKNOWN)\nprobe: missing.txt\n", &[]).is_empty());
}

#[test]
fn selective_vpath_detects_missing_inputs_and_preserves_working_searches() {
    let source = "vpath %.txt sources\nprobe: input.txt\n\t@echo found\n";
    assert_eq!(check(source, &[]).len(), 1);
    assert!(check(source, &[("sources/input.txt", "input")]).is_empty());
    for source in [
        "vpath input.txt first sources\nprobe: input.txt\n",
        "vpath %.txt first:sources\nprobe: input.txt\n",
        "vpath %.txt first\nvpath %.txt sources\nprobe: input.txt\n",
        "SEARCH = sources\nvpath %.txt $(SEARCH)\nSEARCH = wrong\nprobe: input.txt\n",
        "vpath %.src sources\nprobe: output.dat\n%.dat: %.src\n\t@echo generated\n",
    ] {
        assert!(
            check(
                source,
                &[
                    ("sources/input.txt", "input"),
                    ("sources/output.src", "input")
                ]
            )
            .is_empty(),
            "{source}"
        );
    }
}

#[test]
fn selective_vpath_clear_and_include_order_are_observed() {
    let files = [
        ("sources/input.txt", "input"),
        ("clear.mk", "vpath %.txt\n"),
    ];
    for source in [
        "vpath %.txt sources\nvpath %.txt\nprobe: input.txt\n",
        "vpath %.txt sources\nvpath\nprobe: input.txt\n",
        "vpath %.txt sources\ninclude clear.mk\nprobe: input.txt\n",
        "ifeq (no,yes)\nvpath %.txt sources\nendif\nprobe: input.txt\n",
    ] {
        assert_eq!(check(source, &files).len(), 1, "{source}");
    }
    assert!(check(
        "include clear.mk\nvpath %.txt sources\nprobe: input.txt\n",
        &files
    )
    .is_empty());
    assert!(check("vpath %.txt sources\nvpath %.c\nprobe: input.txt\n", &files).is_empty());
}

#[test]
fn uncertain_selective_searches_never_invent_a_missing_input() {
    for source in [
        "vpath %.txt $(EXTERNAL)\nprobe: input.txt\n",
        "vpath a\\%b sources\nprobe: input.txt\n",
        "ifeq ($(EXTERNAL),yes)\nvpath %.txt sources\nendif\nprobe: input.txt\n",
    ] {
        assert!(check(source, &[]).is_empty(), "{source}");
    }
}

#[test]
fn selective_vpath_decisions_agree_with_gnu_make() {
    use std::process::Command;
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    let Ok(version) = Command::new(&make).arg("--version").output() else {
        return;
    };
    if !String::from_utf8_lossy(&version.stdout).contains("GNU Make") {
        return;
    }
    for (directives, expected) in [
        ("vpath %.txt sources\n", true),
        ("vpath %.txt sources\nvpath %.txt\n", false),
        ("vpath %.txt sources\nvpath\n", false),
        (
            "SEARCH = sources\nvpath %.txt $(SEARCH)\nSEARCH = wrong\n",
            true,
        ),
        ("vpath %.txt wrong\nvpath %.txt sources\n", true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("sources")).unwrap();
        std::fs::write(directory.path().join("sources/input.txt"), "input").unwrap();
        let source = format!("{directives}probe: input.txt\n\t@echo found\n");
        std::fs::write(directory.path().join("Makefile"), &source).unwrap();
        let result = Command::new(&make)
            .current_dir(directory.path())
            .args(["-rRn", "probe"])
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .env_remove("GNUMAKEFLAGS")
            .output()
            .unwrap();
        assert_eq!(result.status.success(), expected, "{directives}");
        assert_eq!(
            check(&source, &[("sources/input.txt", "input")]).is_empty(),
            expected,
            "{directives}"
        );
    }
}

#[test]
fn an_implicit_rule_cannot_rescue_its_own_missing_input() {
    for source in [
        "probe: output.dat\n%.dat: %.src\n\t@echo generated\n",
        "probe: nested/output.dat\n%.dat: %.src\n\t@echo generated\n",
        "probe: output.dat\n%.dat: | %.src\n\t@echo generated\n",
    ] {
        assert_eq!(check(source, &[]).len(), 1, "{source}");
    }
    for (source, files) in [
        (
            "probe: output.dat\n%.dat: %.src\n\t@echo generated\n",
            vec![("output.src", "input")],
        ),
        (
            "probe: nested/output.dat\n%.dat: %.src\n\t@echo generated\n",
            vec![("nested/output.src", "input")],
        ),
        (
            "probe: output.dat\n%.dat: %.src\n\t@echo generated\n%.src: seed\n\t@echo source\n",
            vec![("seed", "input")],
        ),
        (
            "probe: output.dat\n%.dat: %.src\n\t@echo first\n%.dat: %.seed\n\t@echo second\n",
            vec![("output.seed", "input")],
        ),
    ] {
        assert!(check(source, &files).is_empty(), "{source}");
    }
}
