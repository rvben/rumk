use rumk::config::Config;
use rumk::parser::parse;
use rumk::rules::{
    portability::{Edition, PosixPortability},
    Rule,
};
fn check(text: &str, edition: Edition) -> Vec<String> {
    PosixPortability(edition)
        .check(&parse(text), text)
        .into_iter()
        .map(|d| d.message)
        .collect()
}
#[test]
fn posix_editions_distinguish_standardized_features() {
    for text in [
        "X ::= value\n",
        "X :::= value\n",
        "X += value\n",
        "X ?= value\n",
        "X != echo value\n",
        ".PHONY: clean\nclean:;\n",
        ".NOTPARALLEL:\nall:;\n",
        "all: first .WAIT second\n",
        "-include optional.mk\n",
        "all:\n\t@echo $^ $+\n",
        "X = $(SRC:%.c=%.o)\n",
    ] {
        assert!(!check(text, Edition::Posix2017).is_empty(), "{text}");
        assert!(
            check(text, Edition::Posix2024).is_empty(),
            "{text}: {:?}",
            check(text, Edition::Posix2024)
        );
    }
}
#[test]
fn gnu_extensions_are_flagged_in_both_profiles() {
    for text in [
        "X := value\n",
        "ifdef X\nall:;\nendif\n",
        "define X\nvalue\nendef\n",
        "all::;\n",
        "all &: dep\n",
        "%.o: %.c\n",
        "out: | setup\n",
        "all: X = y\n",
        "export X = y\n",
        ".ONESHELL:\nall:;\n",
        ".RECIPEPREFIX = >\n",
        "X = $(shell echo hi)\n",
        "all:\n\t@echo $|\n",
    ] {
        for edition in [Edition::Posix2017, Edition::Posix2024] {
            assert!(!check(text, edition).is_empty(), "{text}");
        }
    }
}
#[test]
fn comments_shell_escapes_suffix_rules_and_portable_macros_are_not_extensions() {
    let text=".POSIX:\n# ifdef X and $(shell echo no)\nX = hello\nSRC = x.c\nOBJ = $(SRC:.c=.o)\n.SUFFIXES: .c .o\n.c.o:\n\t@echo $$HOME ${X} $@ $< $? $(shell)\n";
    for edition in [Edition::Posix2017, Edition::Posix2024] {
        assert!(
            check(text, edition).is_empty(),
            "{:?}",
            check(text, edition)
        );
    }
}
#[test]
fn dialect_profile_enables_portability_without_advising_gnu_phony_for_2017() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rumk.toml");
    for dialect in ["posix", "posix2017", "posix2024"] {
        std::fs::write(&path, format!("[global]\ndialect='{dialect}'\n")).unwrap();
        let config = Config::from_file(&path).unwrap();
        assert!(config.rules.iter().any(|r| r.id() == "MK301"));
        assert_eq!(
            config.rules.iter().any(|r| r.id() == "MK201"),
            dialect == "posix2024"
        );
    }
    for dialect in ["bsd", "typo"] {
        std::fs::write(&path, format!("[global]\ndialect='{dialect}'\n")).unwrap();
        assert!(Config::from_file(&path).is_err());
    }
}

#[test]
fn edition2024_requires_blanks_and_limits_synchronization_targets() {
    for source in ["X::=value\n", ".NOTPARALLEL: all\n", ".WAIT:; echo hi\n"] {
        assert!(!check(source, Edition::Posix2024).is_empty(), "{source}");
    }
    for source in [
        "X ::= value\n",
        ".NOTPARALLEL:\n",
        ".WAIT:\n",
        "include a.mk b.mk\n",
    ] {
        assert!(check(source, Edition::Posix2024).is_empty(), "{source}");
    }
    assert!(!check("include a.mk b.mk\n", Edition::Posix2017).is_empty());
}

#[test]
fn project_profile_checks_includes_and_requires_only_the_entry_marker() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(&root, "# portable entry\n.POSIX:\ninclude tasks.mk\n").unwrap();
    std::fs::write(directory.path().join("tasks.mk"), "X := value\nall:;\n").unwrap();
    let project = rumk::project::Project::load(&root, &Default::default()).unwrap();
    let diagnostics = PosixPortability(Edition::Posix2024).check_project(&project);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
    assert!(diagnostics[0]
        .source
        .as_ref()
        .unwrap()
        .ends_with("tasks.mk"));
    std::fs::write(&root, "X = before\n.POSIX:\nall:;\n").unwrap();
    let project = rumk::project::Project::load(&root, &Default::default()).unwrap();
    assert!(PosixPortability(Edition::Posix2024)
        .check_project(&project)
        .iter()
        .any(|d| d.message.contains("begin with .POSIX")));
}
