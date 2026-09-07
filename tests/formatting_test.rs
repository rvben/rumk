use rumk::fix::apply_fixes;
use rumk::parser::parse;
use rumk::rules::formatting::AssignmentSpacing;
use rumk::rules::Rule;
use std::process::Command;

fn format(content: &str) -> String {
    apply_fixes(content, &AssignmentSpacing.check(&parse(content), content)).content
}

#[test]
fn assignment_spacing_preserves_values_operators_comments_and_endings() {
    for (before, after) in [
        ("CC:=cc\n", "CC := cc\n"),
        (
            "CC  =  cc  # retain value spaces\n",
            "CC = cc  # retain value spaces\n",
        ),
        ("NAME=  # retained\n", "NAME = # retained\n"),
        ("EMPTY=   \n", "EMPTY =\n"),
        ("VALUE+=more\r\n", "VALUE += more\r\n"),
        ("VALUE?=fallback", "VALUE ?= fallback"),
        (
            "override export VALUE::=text\n",
            "override export VALUE ::= text\n",
        ),
        ("VALUE:::=text\n", "VALUE :::= text\n"),
        ("VALUE!=printf value\n", "VALUE != printf value\n"),
        ("CAFÉ=valeur\n", "CAFÉ = valeur\n"),
        ("VALUE=$($(NAME))\n", "VALUE = $($(NAME))\n"),
    ] {
        assert_eq!(format(before), after, "{before:?}");
        assert_eq!(format(after), after, "not idempotent: {after:?}");
        let original = parse(before);
        let formatted = parse(after);
        for (old, new) in original.assignments.iter().zip(&formatted.assignments) {
            assert_eq!(old.name, new.name);
            assert_eq!(old.value, new.value);
            assert_eq!(old.operator, new.operator);
        }
    }
}

#[test]
fn assignment_spacing_leaves_context_sensitive_syntax_alone() {
    for text in [
        " =value\n",
        "$(NAME)=value\n",
        "A\\ B=value\n",
        "A:B=value\n",
        "all: VALUE=value\n",
        "define VALUE\nCC=cc\nendef\n",
        "define VALUE :=\nCC=cc\nendef\n",
        "VALUE=one \\\n two\n",
        "all:\n\tVALUE=value; echo $$VALUE\n",
        ".RECIPEPREFIX := >\nall:\n>VALUE=value\n",
        "# VALUE=value\n",
    ] {
        assert_eq!(format(text), text, "{text:?}");
    }
}

#[test]
fn assignment_spacing_matches_gnu_make_value_and_recipe_output() {
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    match Command::new(&make).arg("--version").output() {
        Ok(output) if String::from_utf8_lossy(&output.stdout).contains("GNU Make") => {}
        _ => {
            eprintln!("GNU Make unavailable; semantic probe skipped");
            return;
        }
    }
    let directory = tempfile::tempdir().unwrap();
    // These fixtures run only our authored printf recipes, never external files.
    for ending in ["\n", "\r\n"] {
        let before = concat!(
            "EMPTY=   \n",
            "VALUE=  value  # significant trailing spaces\n",
            "COPY:=$(VALUE)\n",
            "VALUE+=tail\n",
            "COPY?=ignored\n",
            "LITERAL=\\#hash\n",
            ".PHONY: verify\n",
            "verify:\n",
            "\t@printf '<%s>|<%s>|<%s>|<%s>\\n' '$(EMPTY)' '$(VALUE)' '$(COPY)' '$(LITERAL)'\n",
        )
        .replace('\n', ending);
        let after = format(&before);
        assert_ne!(before, after);
        let run = |name: &str, content: &str| {
            std::fs::write(directory.path().join(name), content).unwrap();
            Command::new(&make)
                .current_dir(directory.path())
                .env_remove("MAKEFLAGS")
                .env_remove("MFLAGS")
                .env_remove("GNUMAKEFLAGS")
                .args([
                    "--no-print-directory",
                    "--no-builtin-rules",
                    "-f",
                    name,
                    "verify",
                ])
                .output()
                .unwrap()
        };
        let original = run("before.mk", &before);
        let fixed = run("after.mk", &after);
        assert!(
            original.status.success(),
            "{}",
            String::from_utf8_lossy(&original.stderr)
        );
        assert!(
            fixed.status.success(),
            "{}",
            String::from_utf8_lossy(&fixed.stderr)
        );
        assert_eq!(original.stdout, fixed.stdout);
        assert_eq!(format(&after), after);
    }
}

#[test]
fn assignment_spacing_cli_is_opt_in_and_obeys_suppressions_and_fix_controls() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    let text = "# rumk-disable-next-line MK105\nKEEP=value\nCHANGE=value  \n.PHONY: all\nall:;\n";
    std::fs::write(&path, text).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rumk"))
            .current_dir(directory.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["fmt", "--no-config", "--check", "Makefile"])
        .status
        .success());
    assert_eq!(
        run(&[
            "fmt",
            "--no-config",
            "--enable",
            "MK105",
            "--check",
            "Makefile"
        ])
        .status
        .code(),
        Some(1)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(run(&[
        "fmt",
        "--no-config",
        "--enable",
        "MK105",
        "--unfixable",
        "MK105",
        "Makefile"
    ])
    .status
    .success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert!(
        run(&["fmt", "--no-config", "--enable", "MK105", "Makefile"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text.replace("CHANGE=", "CHANGE = ")
    );
    assert!(run(&[
        "fmt",
        "--no-config",
        "--enable",
        "MK105",
        "--check",
        "Makefile"
    ])
    .status
    .success());
}

#[test]
fn assignment_spacing_preserves_gnu_make_expansions_across_operator_variants() {
    let make = std::env::var("GNU_MAKE").unwrap_or_else(|_| "make".into());
    let Ok(version) = Command::new(&make).arg("--version").output() else {
        return;
    };
    let version = String::from_utf8_lossy(&version.stdout);
    if !version.contains("GNU Make") {
        return;
    }
    let modern = version.contains("GNU Make 4.4") || version.contains("GNU Make 4.5");
    let operators = if modern {
        vec!["=", ":=", "::=", ":::=", "?=", "+=", "!="]
    } else {
        vec!["=", ":=", "?=", "+="]
    };
    let directory = tempfile::tempdir().unwrap();
    for operator in operators {
        for padding in ["", " ", "\t", "   "] {
            let rhs = if operator == "!=" {
                "printf value"
            } else {
                "$(BASE)  # significant padding"
            };
            let original = format!("BASE = value\nVALUE{padding}{operator}{padding}{rhs}\n$(info RESULT:[$(VALUE)])\n.PHONY: verify\nverify:;\n");
            let fixed = format(&original);
            let run = |text: &str| {
                std::fs::write(directory.path().join("Makefile"), text).unwrap();
                Command::new(&make)
                    .current_dir(directory.path())
                    .env_remove("MAKEFLAGS")
                    .env_remove("MFLAGS")
                    .env_remove("GNUMAKEFLAGS")
                    .args(["--no-print-directory", "--no-builtin-rules", "verify"])
                    .output()
                    .unwrap()
            };
            let before = run(&original);
            let after = run(&fixed);
            assert!(
                before.status.success(),
                "{operator}: {}",
                String::from_utf8_lossy(&before.stderr)
            );
            assert!(
                after.status.success(),
                "{operator}: {}",
                String::from_utf8_lossy(&after.stderr)
            );
            assert_eq!(
                before.stdout, after.stdout,
                "{operator}, padding {padding:?}"
            );
            assert_eq!(format(&fixed), fixed);
        }
    }
}
