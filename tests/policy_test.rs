use rumk::config::Config;
use rumk::diagnostic::Diagnostic;
use rumk::parser::parse;
use rumk::project::{Project, ProjectOptions};
use rumk::rules::policy::{GlobalIgnore, RecipeLength, RequiredTargets};
use rumk::rules::Rule;

fn project(content: &str, includes: &[(&str, &str)]) -> (tempfile::TempDir, Project) {
    let directory = tempfile::tempdir().unwrap();
    for (name, text) in includes {
        std::fs::write(directory.path().join(name), text).unwrap();
    }
    let path = directory.path().join("Makefile");
    std::fs::write(&path, content).unwrap();
    let project = Project::load(&path, &ProjectOptions::default()).unwrap();
    (directory, project)
}

fn check(rule: &dyn Rule, content: &str) -> Vec<Diagnostic> {
    let (_directory, project) = project(content, &[]);
    rule.check_project(&project)
}

#[test]
fn global_ignore_requires_an_active_unscoped_declaration() {
    for content in [
        ".IGNORE:\nall:;\n",
        "TARGET := .IGNORE\n$(TARGET):\nall:;\n",
        "NONE :=\n.IGNORE: $(NONE)\nall:;\n",
        "ifeq (yes,yes)\n.IGNORE:\nendif\nall:;\n",
    ] {
        let diagnostics = check(&GlobalIgnore, content);
        assert_eq!(diagnostics.len(), 1, "{content}");
        assert!(diagnostics[0].fix.is_none());
    }
    for content in [
        ".IGNORE: clean\nclean:;\n",
        "ifeq (yes,no)\n.IGNORE:\nendif\nall:;\n",
        "ifdef UNKNOWN\n.IGNORE:\nendif\nall:;\n",
        ".IGNORE: $(UNKNOWN)\nall:;\n",
        "define RULE\n.IGNORE:\nendef\nall:;\n",
        "all:\n\t@echo .IGNORE:\n",
    ] {
        assert!(check(&GlobalIgnore, content).is_empty(), "{content}");
    }
}

#[test]
fn global_ignore_reports_the_include_source_and_each_declaration_once() {
    let (_directory, project) = project(
        "include policy.mk\ninclude policy.mk\nall:;\n",
        &[("policy.mk", "# comment\n.IGNORE:\n")],
    );
    let diagnostics = GlobalIgnore.check_project(&project);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 2);
    assert!(diagnostics[0]
        .source
        .as_ref()
        .unwrap()
        .ends_with("policy.mk"));
}

#[test]
fn required_targets_distinguishes_definition_from_phony_membership() {
    let rule = RequiredTargets::new(["test".into()]);
    let missing = check(&rule, ".PHONY: test\nall:;\n");
    assert_eq!(missing.len(), 1);
    assert!(missing[0].message.contains("no explicit declaration"));
    let assignment_only = check(&rule, "test: VAR=foo\n.PHONY: test\n");
    assert_eq!(assignment_only.len(), 1);
    assert!(assignment_only[0]
        .message
        .contains("no explicit declaration"));
    assert!(check(&rule, "test: VAR=foo\n.PHONY: test\ntest:;\n").is_empty());
    let declared_later = check(&rule, "test: VAR=foo\ntest:;\n");
    assert_eq!(declared_later.len(), 1);
    assert_eq!(declared_later[0].line, 2);
    let not_phony = check(&rule, "# comment\ntest:;\n");
    assert_eq!(not_phony.len(), 1);
    assert_eq!(not_phony[0].line, 2);
    assert!(not_phony[0].message.contains("must be declared .PHONY"));
    assert!(check(&rule, ".PHONY: test\ntest:;\n").is_empty());
    assert!(check(&rule, "NAME := test\n.PHONY: $(NAME)\n$(NAME):;\n").is_empty());
    assert_eq!(check(&rule, "test%:;\n").len(), 1);
}

#[test]
fn required_targets_span_includes_and_report_at_the_real_declaration() {
    let rule = RequiredTargets::new(["test".into()]);
    let (_directory, project) = project(
        "include tasks.mk\n.PHONY: test\n",
        &[("tasks.mk", "test:;\n")],
    );
    assert!(rule.check_project(&project).is_empty());
    let (_directory, project) =
        self::project("include tasks.mk\n", &[("tasks.mk", "# heading\ntest:;\n")]);
    let diagnostics = rule.check_project(&project);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 2);
    assert!(diagnostics[0]
        .source
        .as_ref()
        .unwrap()
        .ends_with("tasks.mk"));
}

#[test]
fn required_targets_do_not_invent_absence_from_incomplete_graphs() {
    let rule = RequiredTargets::new(["test".into()]);
    for content in [
        "include absent.mk\nall:;\n",
        "-include generated.mk\nall:;\n",
        "include $(UNKNOWN)\nall:;\n",
        "ifdef UNKNOWN\ntest:;\nendif\nall:;\n",
        "$(TARGETS):;\n",
        "$(eval test:;)\nall:;\n",
        "X := $(eval test:;)\nall:;\n",
        "$(call generate)\nall:;\n",
        "ifeq (x,x)\nall:;\n",
    ] {
        assert!(check(&rule, content).is_empty(), "{content}");
    }
    assert_eq!(check(&rule, "ifeq (x,y)\ntest:;\nendif\nall:;\n").len(), 1);
}

#[test]
fn recipe_length_counts_logical_commands_not_physical_lines() {
    let rule = RecipeLength::new(2);
    let content = "all:\n\t# comment\n\t@echo one \\\n\t  two\n\t\n\t-echo three\n";
    assert!(rule.check(&parse(content), content).is_empty());
    let content = format!("{content}\t@echo four\n");
    let diagnostics = rule.check(&parse(&content), &content);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("3 logical"));
    assert!(diagnostics[0].fix.is_none());
}

#[test]
fn recipe_length_handles_inline_grouped_oneshell_and_inactive_commands() {
    let rule = RecipeLength::new(1);
    for content in [
        "all:; echo one\n\techo two\n",
        "a b &:\n\techo one\n\techo two\n",
        ".ONESHELL:\nall:\n\techo one\n\techo two\n",
    ] {
        assert_eq!(rule.check(&parse(content), content).len(), 1, "{content}");
    }
    let content = "all:\n\techo one\nifeq (x,y)\n\techo two\nendif\n";
    assert!(rule.check(&parse(content), content).is_empty());
}

#[test]
fn policy_configuration_is_validated_and_round_trips() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rumk.toml");
    std::fs::write(&path, "[MK104]\nenabled = true\nmax-lines = 3\n[MK215]\nenabled = true\nrequired = ['test','test','verify']\n").unwrap();
    let config = Config::from_file(&path).unwrap();
    assert_eq!(config.get("MK104.max-lines").as_deref(), Some("3"));
    assert!(config.get("MK215.required").unwrap().contains("verify"));
    std::fs::write(&path, config.render(false, false)).unwrap();
    assert_eq!(
        Config::from_file(&path).unwrap().render(false, false),
        config.render(false, false)
    );
    for invalid in [
        "[MK104]\nenabled=true\nmax-lines=0",
        "[MK104]\nenabled=true\nmax-lines='3'",
        "[MK215]\nenabled=true\nrequired=['']",
        "[MK215]\nenabled=true\nrequired=['a b']",
        "[MK215]\nenabled=true\nrequired=['$(X)']",
        "[MK215]\nenabled=true\nrequired=[4]",
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(Config::from_file(&path).is_err(), "{invalid}");
    }
}

#[test]
fn global_ignore_diagnostics_agree_with_gnu_make_failure_propagation() {
    use std::process::Command;
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    let Ok(version) = Command::new(&make).arg("--version").output() else {
        return;
    };
    if !String::from_utf8_lossy(&version.stdout).contains("GNU Make") {
        return;
    }
    for (declaration, ignores_failure) in [
        (".IGNORE:", true),
        (".IGNORE: cleanup", false),
        (".IGNORE: | cleanup", false),
    ] {
        let text = format!("{declaration}\n.PHONY: all cleanup\nall:\n\tfalse\ncleanup:;\n");
        let (directory, project) = project(&text, &[]);
        let output = Command::new(&make)
            .current_dir(directory.path())
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .env_remove("GNUMAKEFLAGS")
            .args(["--no-print-directory", "--no-builtin-rules", "all"])
            .output()
            .unwrap();
        assert_eq!(output.status.success(), ignores_failure, "{declaration}");
        assert_eq!(
            !GlobalIgnore.check_project(&project).is_empty(),
            ignores_failure,
            "{declaration}"
        );
    }
}
