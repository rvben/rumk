use std::collections::BTreeSet;

use rumk::diagnostic::Applicability;
use rumk::fix::apply_fixes;
use rumk::parser::parse;
use rumk::rules::best_practices::{
    DependencyCycle, DirectoryChangeInRecipe, DuplicateRecipe, MissingPhony, PhonyPlacement,
    RecursiveMake, ShellInRecursiveVariable, ShellStyleVariableReference,
};
use rumk::rules::style::LineLength;
use rumk::rules::syntax::{
    ConditionalStructure, InvalidSyntax, InvalidVariableSyntax, SpecialTargetPlacement, TabInRecipe,
};
use rumk::rules::Rule;

#[test]
fn line_length_ignores_comments_and_recipes_by_default() {
    let content = concat!(
        "# generated command output that is intentionally very long\n",
        "OUTPUT := this declarative value is still intentionally very long\n",
        "all:\n",
        "\techo this recipe command is intentionally very long\n",
    );
    let makefile = parse(content);

    let diagnostics = LineLength::new(20).check(&makefile, content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 2);
}

#[test]
fn line_length_wraps_a_static_phony_declaration_idempotently() {
    let content = ".PHONY: build test lint clean release\n";
    let rule = LineLength::new(32);
    let diagnostics = rule.check(&parse(content), content);

    assert!(rule.fixable());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fixable);

    let fixed = apply_fixes(content, &diagnostics).content;
    assert_eq!(fixed, ".PHONY: build test lint clean \\\n        release\n");
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn line_length_phony_fix_preserves_an_inline_comment_and_crlf() {
    let content = ".PHONY: build test lint clean # public commands\r\n";
    let rule = LineLength::new(36);
    let diagnostics = rule.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        ".PHONY: build test lint \\\r\n        clean # public commands\r\n"
    );
}

#[test]
fn line_length_phony_fix_preserves_a_missing_final_newline() {
    let content = ".PHONY: build test lint clean release";
    let diagnostics = LineLength::new(32).check(&parse(content), content);

    let fixed = apply_fixes(content, &diagnostics).content;
    assert_eq!(fixed, ".PHONY: build test lint clean \\\n        release");
    assert!(!fixed.ends_with('\n'));
}

#[test]
fn line_length_does_not_fix_dynamic_conditional_or_unwrappable_phonies() {
    let cases = [
        ".PHONY: $(COMMANDS) another-command\n",
        "ifeq ($(MODE),ci)\n.PHONY: build test lint clean\nendif\n",
        ".PHONY: command-name-that-cannot-fit\n",
        ".PHONY: build test \\\n        lint clean release\n",
        " .PHONY: build test lint clean\n",
    ];

    for content in cases {
        let diagnostics = LineLength::new(20).check(&parse(content), content);
        assert_eq!(diagnostics.len(), 1, "{content:?}");
        assert!(!diagnostics[0].fixable, "{content:?}");
    }
}

#[test]
fn line_length_offers_to_wrap_a_declaration_another_rule_also_rewrites() {
    let content = ".PHONY: lint command-one command-two\nall:\n\t@:\n";
    let diagnostics = LineLength::new(24).check(&parse(content), content);

    // MK201 will want this declaration too, to add the missing `all`. Both
    // fixes are real, and the fix loop settles which of them rewrites the line
    // first, so MK101 offers its own rather than guessing about MK201.
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fixable);
}

#[test]
fn tab_rule_accepts_inline_and_custom_prefix_recipes() {
    let content = ".RECIPEPREFIX := >\nall: ; @echo inline\n>@echo prefixed\n";
    let makefile = parse(content);

    assert!(TabInRecipe.check(&makefile, content).is_empty());
}

#[test]
fn tab_rule_still_fixes_the_complete_space_prefix() {
    let content = "all:\n    echo wrong\n";
    let makefile = parse(content);
    let diagnostics = TabInRecipe.check(&makefile, content);

    assert_eq!(diagnostics.len(), 1);
    let edit = &diagnostics[0].fix.as_ref().unwrap().edits[0];
    assert_eq!(edit.start_column, 1);
    assert_eq!(edit.end_column, 5);
    assert_eq!(edit.replacement, "\t");
    assert_eq!(
        diagnostics[0].message,
        "Recipe must be indented with tab, not spaces"
    );
}

#[test]
fn tab_rule_fixes_with_the_active_recipe_prefix() {
    let content = ".RECIPEPREFIX := >\nall:\n    echo wrong\n";
    let diagnostics = TabInRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].message,
        "Recipe must be indented with the recipe prefix '>', not spaces"
    );
    let fix = diagnostics[0].fix.as_ref().unwrap();
    assert_eq!(fix.description, "Replace spaces with the recipe prefix '>'");
    assert_eq!(fix.edits[0].start_column, 1);
    assert_eq!(fix.edits[0].end_column, 5);
    assert_eq!(fix.edits[0].replacement, ">");

    let fixed = apply_fixes(content, &diagnostics).content;
    assert_eq!(fixed, ".RECIPEPREFIX := >\nall:\n>echo wrong\n");
    let makefile = parse(&fixed);
    assert!(TabInRecipe.check(&makefile, &fixed).is_empty());
    assert!(InvalidSyntax.check(&makefile, &fixed).is_empty());
}

#[test]
fn tab_rule_tracks_the_recipe_prefix_in_source_order() {
    let cases = [
        ("all:\n    echo\n.RECIPEPREFIX := >\n", vec!["\t"]),
        (".RECIPEPREFIX = >\nall:\n    echo\n", vec![">"]),
        (
            ".RECIPEPREFIX := >\n.RECIPEPREFIX =\nall:\n    echo\n",
            vec!["\t"],
        ),
        (
            ".RECIPEPREFIX := >\nall:\n    echo\n.RECIPEPREFIX := |\nother:\n    echo\n",
            vec![">", "|"],
        ),
    ];

    for (content, prefixes) in cases {
        let replacements: Vec<String> = TabInRecipe
            .check(&parse(content), content)
            .iter()
            .map(|diagnostic| {
                diagnostic.fix.as_ref().unwrap().edits[0]
                    .replacement
                    .clone()
            })
            .collect();
        assert_eq!(replacements, prefixes, "{content:?}");
    }
}

#[test]
fn tab_rule_says_nothing_when_the_prefix_can_be_the_space_it_reads() {
    // GNU Make reads recipes with a space where `.RECIPEPREFIX` expands to one,
    // so a leading space is the prefix rather than a defect.
    let cases = [
        "P := >\n.RECIPEPREFIX := $(P)\nall:\n    echo\n",
        ".RECIPEPREFIX != printf %s '>'\nall:\n    echo\n",
        "define .RECIPEPREFIX\n>\nendef\nall:\n    echo\n",
        "e :=\nspace := $(e) $(e)\n.RECIPEPREFIX := $(space)\nall:\n echo\n",
    ];

    for content in cases {
        let diagnostics = TabInRecipe.check(&parse(content), content);
        assert!(diagnostics.is_empty(), "{content:?}");
    }
}

#[test]
fn tab_rule_reports_without_a_fix_when_the_prefix_is_undecided() {
    // Every branch here reads recipes with a tab or with '>', so the spaces are
    // wrong whichever one Make takes, while the character to write is open.
    let cases = [
        "ifdef X\n.RECIPEPREFIX = >\nendif\nall:\n    echo\n",
        // An included file can assign the prefix Make reads from here on.
        "include config.mk\nall:\n    echo\n",
        "-include config.mk\n.RECIPEPREFIX := >\nall:\n    echo\n",
    ];

    for content in cases {
        let diagnostics = TabInRecipe.check(&parse(content), content);

        assert_eq!(diagnostics.len(), 1, "{content:?}");
        assert!(diagnostics[0].fix.is_none(), "{content:?}");
        assert!(!diagnostics[0].fixable, "{content:?}");
        assert_eq!(
            diagnostics[0].message,
            "Recipe must be indented with the active recipe prefix, not spaces",
            "{content:?}"
        );
        assert_eq!(
            apply_fixes(content, &diagnostics).content,
            content,
            "{content:?}"
        );
    }
}

#[test]
fn missing_phony_rule_groups_targets_and_preserves_crlf() {
    let content = "all:\r\n\t@:\r\nclean:\r\n\t@:\r\n";
    let makefile = parse(content);
    let rule = MissingPhony::default();
    let diagnostics = rule.check(&makefile, content);

    assert!(rule.fixable());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fixable);
    assert_eq!(diagnostics[0].fix.as_ref().unwrap().edits.len(), 1);
    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        ".PHONY: all clean\r\nall:\r\n\t@:\r\nclean:\r\n\t@:\r\n"
    );
}

#[test]
fn missing_phony_extends_a_canonical_group_and_preserves_its_comment() {
    let content = concat!(
        ".PHONY: lint # public commands\n",
        "lint:\n\t@:\n",
        "all:\n\t@:\n",
        "clean:\n\t@:\n",
    );
    let rule = MissingPhony::default();
    let diagnostics = rule.check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics).content;

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        fixed,
        concat!(
            ".PHONY: lint all clean # public commands\n",
            "lint:\n\t@:\n",
            "all:\n\t@:\n",
            "clean:\n\t@:\n",
        )
    );
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn missing_phony_preserves_an_existing_per_section_style() {
    let content = concat!(
        ".PHONY: lint\n",
        "lint:\n\t@:\n",
        ".PHONY: deploy\n",
        "deploy:\n\t@:\n",
        "clean:\n\t@:\n",
        "test:\n\t@:\n",
    );
    let diagnostics = MissingPhony::default().check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].fix.as_ref().unwrap().edits.len(), 2);
    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        concat!(
            ".PHONY: lint\n",
            "lint:\n\t@:\n",
            ".PHONY: deploy\n",
            "deploy:\n\t@:\n",
            ".PHONY: clean\n",
            "clean:\n\t@:\n",
            ".PHONY: test\n",
            "test:\n\t@:\n",
        )
    );
}

#[test]
fn missing_phony_does_not_extend_a_conditional_declaration() {
    let content = concat!(
        "ifeq ($(MODE),lint)\n",
        ".PHONY: lint\n",
        "endif\n",
        "all:\n\t@:\n",
    );
    let diagnostics = MissingPhony::default().check(&parse(content), content);

    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        concat!(
            "ifeq ($(MODE),lint)\n",
            ".PHONY: lint\n",
            "endif\n",
            ".PHONY: all\n",
            "all:\n\t@:\n",
        )
    );
}

#[test]
fn missing_phony_merges_and_wraps_an_overlong_canonical_group() {
    let existing_names = (1..=16)
        .map(|index| format!("command-{index:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    let existing = format!(".PHONY: {existing_names}\n");
    let content = format!("{existing}all:\n\t@:\n");
    let rule = MissingPhony::default();
    let diagnostics = rule.check(&parse(&content), &content);
    let fixed = apply_fixes(&content, &diagnostics).content;

    assert_eq!(fixed.matches(".PHONY:").count(), 1);
    assert!(fixed.contains(" \\\n        "));
    assert!(fixed.contains("command-16 all"));
    assert!(fixed.lines().all(|line| line.chars().count() <= 120));
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn missing_phony_can_place_a_group_before_the_first_rule() {
    let content = concat!("SHELL := /bin/sh\n", "\n", "all:\n\t@:\n", "clean:\n\t@:\n",);
    let rule = MissingPhony::new(PhonyPlacement::Top);
    let diagnostics = rule.check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics).content;

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        fixed,
        concat!(
            "SHELL := /bin/sh\n",
            "\n",
            ".PHONY: all clean\n",
            "all:\n\t@:\n",
            "clean:\n\t@:\n",
        )
    );
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn missing_phony_can_extend_the_top_declaration() {
    let content = concat!(
        ".PHONY: lint\n",
        "lint:\n\t@:\n",
        ".PHONY: deploy\n",
        "deploy:\n\t@:\n",
        "clean:\n\t@:\n",
        "test:\n\t@:\n",
    );
    let rule = MissingPhony::new(PhonyPlacement::Top);
    let diagnostics = rule.check(&parse(content), content);

    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        concat!(
            ".PHONY: lint clean test\n",
            "lint:\n\t@:\n",
            ".PHONY: deploy\n",
            "deploy:\n\t@:\n",
            "clean:\n\t@:\n",
            "test:\n\t@:\n",
        )
    );
}

#[test]
fn missing_phony_can_place_declarations_adjacent_to_targets() {
    let content = "all:\n\t@:\nclean:\n\t@:\n";
    let rule = MissingPhony::new(PhonyPlacement::Adjacent);
    let diagnostics = rule.check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics).content;

    assert_eq!(
        fixed,
        ".PHONY: all\nall:\n\t@:\n.PHONY: clean\nclean:\n\t@:\n"
    );
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn make_builtin_variable_names_are_valid() {
    let content = ".RECIPEPREFIX := >\n.VARIABLES := value\n";
    let makefile = parse(content);

    assert!(InvalidVariableSyntax.check(&makefile, content).is_empty());
}

#[test]
fn make_permits_punctuation_and_computed_variable_names() {
    let content = concat!(
        "package/version := 1\n",
        "feature+flags := enabled\n",
        "$(PREFIX)_SOURCES := main.c\n",
        "$(two words) := yes\n",
    );
    let makefile = parse(content);

    assert!(InvalidVariableSyntax.check(&makefile, content).is_empty());
}

#[test]
fn conditional_structure_reports_only_malformed_blocks() {
    let valid = "ifdef A\nifeq ($(MODE),debug)\nelse\nendif\nendif\n";
    assert!(ConditionalStructure.check(&parse(valid), valid).is_empty());

    let invalid = "else\nendif\nifndef OPEN\n";
    let diagnostics = ConditionalStructure.check(&parse(invalid), invalid);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.line)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.rule_id == "MK003"));
}

#[test]
fn recursive_make_rule_distinguishes_commands_from_arguments() {
    let content = concat!(
        "all:\n",
        "\tmake -C first\n",
        "\tcd second && /usr/bin/make test\n",
        "\tMODE=debug command gmake check\n",
        "\tC:\\tools\\make.exe windows\n",
        "\t@echo make\n",
        "\t@printf '%s\\n' 'make'\n",
        "\t+$(MAKE) -C good\n",
        "\t@echo $$MAKE\n",
    );
    let diagnostics = RecursiveMake.check(&parse(content), content);

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.line)
            .collect::<Vec<_>>(),
        [2, 3, 4, 5]
    );
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.rule_id == "MK203"));
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.fixable));
    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        concat!(
            "all:\n",
            "\t$(MAKE) -C first\n",
            "\tcd second && $(MAKE) test\n",
            "\tMODE=debug command $(MAKE) check\n",
            "\t$(MAKE) windows\n",
            "\t@echo make\n",
            "\t@printf '%s\\n' 'make'\n",
            "\t+$(MAKE) -C good\n",
            "\t@echo $$MAKE\n",
        )
    );
}

#[test]
fn recursive_make_fix_replaces_every_command_position_on_a_line() {
    let content = ".PHONY: all\nall: ; make first && env MODE=debug gmake second\n";
    let diagnostics = RecursiveMake.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].fix.as_ref().unwrap().edits.len(), 2);
    assert_eq!(
        apply_fixes(content, &diagnostics).content,
        ".PHONY: all\nall: ; $(MAKE) first && env MODE=debug $(MAKE) second\n"
    );
}

#[test]
fn recursive_make_reports_but_does_not_rewrite_continued_recipes() {
    let content = ".PHONY: all\nall:\n\tmake \\\n\t  -C sub\n";
    let diagnostics = RecursiveMake.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].fixable);
    assert!(diagnostics[0].fix.is_none());
}

#[test]
fn duplicate_recipe_rule_allows_merged_and_double_colon_rules() {
    let content = concat!(
        "duplicate:\n",
        "\t@:\n",
        "duplicate: prerequisite\n",
        "\t@echo replacement\n",
        "merged: one\n",
        "merged: two\n",
        "event::\n",
        "\t@echo first\n",
        "event::\n",
        "\t@echo second\n",
    );
    let diagnostics = DuplicateRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK204");
    assert_eq!(diagnostics[0].line, 3);
}

#[test]
fn graph_rules_do_not_cross_conditional_branches() {
    let content = concat!(
        "ifeq ($(MODE),one)\n",
        "choice: first\n",
        "\t@echo one\n",
        "first: choice\n",
        "else\n",
        "choice: second\n",
        "\t@echo two\n",
        "second: choice\n",
        "endif\n",
    );
    let makefile = parse(content);

    assert!(DuplicateRecipe.check(&makefile, content).is_empty());
    assert!(DependencyCycle.check(&makefile, content).is_empty());
}

#[test]
fn dependency_cycle_rule_reports_components_once() {
    let content = concat!(
        "alpha: beta\n",
        "beta: alpha\n",
        "self: self\n",
        "leaf: external\n",
    );
    let diagnostics = DependencyCycle.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.line)
            .collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.rule_id == "MK205"));
}

#[test]
fn special_targets_must_not_share_the_left_hand_side() {
    let valid = ".PHONY: all\nall:\n\t@:\n";
    let invalid = ".PHONY all:\n\t@:\n";

    assert!(SpecialTargetPlacement
        .check(&parse(valid), valid)
        .is_empty());
    let diagnostics = SpecialTargetPlacement.check(&parse(invalid), invalid);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK005");
}

/// Makefiles that reach every rule offering a fix, so the applicability check
/// below has something to check.
fn fix_corpus() -> Vec<String> {
    vec![
        "CC:=cc  # retain value spaces\n".to_string(),
        "all:\n    echo hi\n".to_string(),
        "all clean:\n\tmake -C sub && gmake test\n".to_string(),
        format!(".PHONY: {}\n", ["target"; 40].join(" ")),
        "DEST := $PREFIX/share\n".to_string(),
        "all  :  dep   other\n".to_string(),
        "all:\n\t@@+-+--echo build\n".to_string(),
    ]
}

/// The applicability a rule declares is what `rumk rule` reports and what the
/// documentation promises; the applicability on the fix is what a run acts on.
/// A rule whose two disagree tells a user one thing and does another.
#[test]
fn declared_fix_applicability_matches_the_fixes_rules_produce() {
    let mut produced = BTreeSet::new();
    for content in fix_corpus() {
        let makefile = parse(&content);
        for rule in rumk::rules::get_all_rules() {
            for diagnostic in rule.check(&makefile, &content) {
                let Some(fix) = diagnostic.fix else {
                    continue;
                };
                assert_eq!(
                    fix.applicability,
                    rule.fix_applicability(),
                    "{} produces a fix marked {} while declaring {}",
                    rule.id(),
                    fix.applicability.as_str(),
                    rule.fix_applicability().as_str()
                );
                produced.insert(rule.id());
            }
        }
    }

    let declared = rumk::rules::get_all_rules()
        .iter()
        .filter(|rule| rule.fixable())
        .map(|rule| rule.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        produced, declared,
        "the corpus must reach every rule that offers a fix, or the check above passes on nothing"
    );
}

#[test]
fn shell_style_reference_names_what_make_actually_reads_and_offers_an_unsafe_fix() {
    // GNU Make 3.81 expands this to '/share', because '$P' is the empty
    // one-character variable 'P' and 'REFIX' is literal text.
    let content = "DEST := $PREFIX/share\n";
    let rule = ShellStyleVariableReference;
    let diagnostics = rule.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].rule_id, "MK211");
    assert_eq!(diagnostics[0].line, 1);
    assert_eq!(diagnostics[0].column, 9);
    assert_eq!(
        diagnostics[0].message,
        "'$PREFIX' reads the variable 'P' and the literal text 'REFIX'"
    );
    assert!(diagnostics[0].fixable);
    let fix = diagnostics[0].fix.as_ref().unwrap();
    assert_eq!(
        fix.applicability,
        Applicability::Unsafe,
        "writing the parentheses in makes Make read a variable it was not reading"
    );
    assert_eq!(fix.description, "Read 'PREFIX' as one variable");

    let fixed = apply_fixes(content, &diagnostics).content;
    assert_eq!(fixed, "DEST := $(PREFIX)/share\n");
    assert!(rule.check(&parse(&fixed), &fixed).is_empty());
}

#[test]
fn shell_style_reference_says_nothing_about_a_reference_make_reads_whole() {
    let content = concat!(
        "V := one\n",
        "all: $(V)\n",
        // '$T' names the whole variable 'T' whether or not the file defines
        // one, so there is nothing here Make reads differently than it looks.
        "\techo $@ $< $(V) ${V} $$HOME $V $T\n",
    );

    assert!(ShellStyleVariableReference
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_style_reference_says_nothing_where_the_file_defines_the_first_character() {
    // '$Qecho' in a file that gives 'Q' a value reads as '$(Q)echo', which is
    // what it was written to mean.
    let content = "Q := @\nall:\n\t$Qecho hi\n";

    assert!(ShellStyleVariableReference
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_style_reference_in_a_recipe_is_reported_without_a_fix() {
    let content = "all:\n\techo $HOME\n";
    let diagnostics = ShellStyleVariableReference.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);
    assert_eq!(diagnostics[0].column, 7);
    assert!(
        !diagnostics[0].fixable,
        "'$HOME' in a recipe may mean '$(HOME)' or '$$HOME', which are different edits"
    );
}

#[test]
fn shell_style_reference_says_nothing_about_a_comment() {
    let content = "# install into $PREFIX\nDEST := build # or $PREFIX\nall:\n\ttrue\n";

    assert!(ShellStyleVariableReference
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_is_reported_when_another_recipe_line_follows_it() {
    // Make runs each line in its own shell, so '$(MAKE)' runs where Make
    // started rather than in 'build'.
    let content = "all:\n\tcd build\n\t$(MAKE)\n";
    let diagnostics = DirectoryChangeInRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].rule_id, "MK212");
    assert_eq!(diagnostics[0].line, 2);
    assert_eq!(diagnostics[0].column, 2);
    assert_eq!(
        diagnostics[0].message,
        "The directory this line changes to is gone when the next line runs"
    );
}

#[test]
fn directory_change_says_nothing_when_the_same_line_uses_it() {
    let content = "all:\n\tcd build && $(MAKE)\n\tcd docs; ls\n\ttrue\n";

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_says_nothing_on_the_last_line_of_a_recipe() {
    let content = "all:\n\ttrue\n\tcd build\n";

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_says_nothing_where_one_shell_runs_the_whole_recipe() {
    let content = ".ONESHELL:\nall:\n\tcd build\n\t$(MAKE)\n";

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_in_a_variable_make_expands_at_every_reading() {
    let content = "VERSION = $(shell git describe --tags)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].rule_id, "MK213");
    assert_eq!(diagnostics[0].line, 1);
    assert_eq!(diagnostics[0].column, 1);
    assert_eq!(
        diagnostics[0].message,
        "'VERSION' runs its $(shell ...) again every time it is read"
    );
    assert!(!diagnostics[0].fixable);
}

#[test]
fn shell_call_says_nothing_where_make_runs_the_command_once() {
    let content = concat!(
        "VERSION := $(shell git describe --tags)\n",
        "COMMIT != git rev-parse HEAD\n",
        "STAMP ::= $(shell date)\n",
        "ESCAPED :::= $(shell date)\n",
        "SHELLED = $$(date)\n",
        "define ONCE :=\n$(shell date)\nendef\n",
    );

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_follows_the_flavor_the_variable_being_appended_to_has() {
    let content = concat!(
        "EAGER := start\n",
        "EAGER += $(shell date)\n",
        "LAZY = start\n",
        "LAZY += $(shell date)\n",
    );
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 4);
    assert!(diagnostics[0].message.starts_with("'LAZY'"));
}

#[test]
fn shell_call_says_nothing_about_a_value_that_reads_an_argument() {
    // A macro taking '$1' has to be expanded at each call to mean anything, so
    // the shell call inside it is what the macro is for.
    let content = "to-host = $(strip $(shell cygpath -m $1))\nfrom = $(shell echo $@)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_in_a_define_body_make_expands_at_every_reading() {
    let content = "define REPORT\n$(shell git status --short)\nendef\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
    assert_eq!(
        diagnostics[0].message,
        "'REPORT' runs its $(shell ...) again every time it is read"
    );
}

#[test]
fn shell_style_reference_is_found_inside_the_call_that_holds_it() {
    // Make expands the arguments of a call too, so '$AR' inside '$(dir ...)'
    // is read as '$(A)' followed by 'R' just as it is anywhere else.
    let content = "FOO := $(dir $AR)\nall:\n\t@echo $(shell echo $BUILD)\n";
    let diagnostics = ShellStyleVariableReference.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (1, 14));
    assert_eq!(
        diagnostics[0].message,
        "'$AR' reads the variable 'A' and the literal text 'R'"
    );
    assert_eq!((diagnostics[1].line, diagnostics[1].column), (3, 21));
    assert!(!diagnostics[1].fixable);
}

#[test]
fn shell_style_reference_offers_no_fix_on_a_line_that_continues_a_recipe() {
    // Make needs no recipe prefix on a continuation line, so the indentation
    // does not say whether the line is part of the recipe. This one is: Make
    // runs 'echo hi AR arg' as a single command.
    let content = "all:\n\t@echo hi \\\n$VAR arg\n";
    let diagnostics = ShellStyleVariableReference.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3);
    assert!(
        !diagnostics[0].fixable,
        "the line is inside a recipe, where '$(VAR)' and '$$VAR' are different edits"
    );
}

#[test]
fn directory_change_reads_a_quoted_command_name_as_the_command() {
    // Quoting a command name does not change which command the shell runs.
    let content = "all:\n\t\"cd\" build\n\tls\n";
    let diagnostics = DirectoryChangeInRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);
}

#[test]
fn directory_change_says_nothing_where_a_quoted_keyword_makes_it_an_argument() {
    // Quoting does take a word out of the shell's reserved words, so '"if"' is
    // a command named 'if' and the 'cd' after it is an argument to it. No
    // directory changes on this line.
    let content = "all:\n\t\"if\" cd\n\tls\n";

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_says_nothing_where_the_line_ends_a_subshell_a_loop_or_a_test() {
    let content = concat!(
        "all:\n",
        "\tif cd sub; then true; fi\n",
        "\tfor d in a b; do cd $$d; done\n",
        "\t(cd sub && build)\n",
        "\ttrue\n",
    );

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_says_nothing_about_a_branch_make_never_reads() {
    // Make skips the branch, so the command never runs, not even once.
    let content = "ifeq (1,0)\nZ = $(shell echo z)\nendif\nall:\n\t@echo ok\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_ignores_the_flavor_a_branch_make_never_reads_would_have_given() {
    // Make never assigns 'Z' in the skipped branch, so the '+=' below appends
    // to nothing and creates a recursive variable.
    let content = "ifeq (1,0)\nZ := base\nendif\nZ += $(shell echo side-effect)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 4);
    assert!(diagnostics[0].message.starts_with("'Z'"));
}

#[test]
fn shell_call_says_nothing_about_an_argument_a_literal_condition_rules_out() {
    // '$(if ,a,b)' has an empty condition, so Make expands only 'b'.
    let content = "J = $(if ,$(shell echo skipped),no)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_in_a_conditional_assignment() {
    // '?=' gives the variable a recursive value; there is no immediate form.
    let content = "STAMP ?= $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
}

#[test]
fn shell_call_is_reported_where_an_append_has_nothing_to_append_to() {
    // '+=' creates a recursive variable where the name has no value yet.
    let content = "FRESH += $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
}

#[test]
fn shell_style_reference_offers_no_fix_in_a_recipe_the_rule_line_carries() {
    // 'all:; echo $HOME' is a recipe, and Make prints 'OME' for it, so the
    // reference is one of the two edits a recipe leaves open.
    let content = "all: build ; echo $HOME\n";
    let diagnostics = ShellStyleVariableReference.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (1, 19));
    assert!(!diagnostics[0].fixable);
}

#[test]
fn shell_style_reference_reads_a_hash_in_an_inline_recipe_as_shell_text() {
    // Make hands the whole inline recipe to the shell, '#' included, so the
    // reference after it is text Make still expands.
    let content = "all: ; echo hi # $HOME\n";
    let diagnostics = ShellStyleVariableReference.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);

    // A '#' before the ';' is a comment that takes the semicolon with it, so
    // there is no recipe on this line and nothing to report.
    let commented = "all: # ; echo $HOME\n";
    assert!(ShellStyleVariableReference
        .check(&parse(commented), commented)
        .is_empty());
}

#[test]
fn directory_change_says_nothing_about_a_cd_a_shell_comment_hides() {
    // The shell reads '#' as a comment to the end of the line, so nothing
    // after it runs and no directory changes.
    let content = "all:\n\techo hi # old command: ; cd sub\n\tpwd\n";

    assert!(DirectoryChangeInRecipe
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_is_reported_where_a_shell_comment_follows_it() {
    // 'cd /tmp' is the last command the shell runs on this line; the text
    // after '#' is not a command at all.
    let content = "all:\n\tcd /tmp # comment; unrelated\n\tpwd\n";
    let diagnostics = DirectoryChangeInRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (2, 2));
}

#[test]
fn directory_change_reads_a_quoted_or_escaped_hash_as_text() {
    // A '#' only starts a comment where it starts a word, and quoting or
    // escaping it takes even that away.
    let cases = [
        "all:\n\techo \"a # b\" ; cd sub\n\tpwd\n",
        "all:\n\techo \\# ; cd sub\n\tpwd\n",
        "all:\n\techo a#b ; cd sub\n\tpwd\n",
    ];

    for content in cases {
        let diagnostics = DirectoryChangeInRecipe.check(&parse(content), content);
        assert_eq!(diagnostics.len(), 1, "{content:?} {diagnostics:?}");
        assert_eq!(diagnostics[0].line, 2, "{content:?}");
    }
}

#[test]
fn shell_call_says_nothing_where_a_conditional_assignment_does_nothing() {
    // '?=' does not touch a name that already has a value, so 'Z' stays the
    // simple variable ':=' made and the append is expanded once.
    let content = "Z := base\nZ ?= ignored\nZ += $(shell date)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_follows_an_append_onto_a_variable_make_expands_again() {
    // '!=' stores the output without expanding it and ':::=' escapes what it
    // expands; GNU Make calls both recursive, so an append to either is
    // expanded again at every reading.
    for operator in ["!=", ":::="] {
        let content = format!("X {operator} base\nX += $(shell date)\n");
        let diagnostics = ShellInRecursiveVariable.check(&parse(&content), &content);

        assert_eq!(diagnostics.len(), 1, "{content:?} {diagnostics:?}");
        assert_eq!(diagnostics[0].line, 2, "{content:?}");
    }
}

#[test]
fn shell_call_says_nothing_about_the_assignment_that_runs_the_command_once() {
    // The value of a '!=' or a ':::=' is expanded where it is written, whatever
    // flavor the name is left in, so the command behind it runs once.
    for operator in ["!=", ":::="] {
        let content = format!("X {operator} echo $(shell date)\n");

        assert!(
            ShellInRecursiveVariable
                .check(&parse(&content), &content)
                .is_empty(),
            "{content:?}"
        );
    }
}

#[test]
fn shell_call_says_nothing_about_a_flavor_a_target_specific_assignment_gave() {
    // 'other: X = lazy' binds 'X' only while Make builds 'other'; the file-wide
    // 'X' is still the simple one, so the append is expanded once.
    let content = "X := base\nother: X = lazy\nX += $(shell date)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_in_a_target_specific_variable_make_expands_again() {
    // 'all: X = ...' binds 'X' while Make builds 'all', and binds it
    // recursively, so every reading of '$(X)' in that recipe runs the command.
    let content = "all: X = $(shell date)\nall:\n\t@echo $(X) $(X)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 1);
    assert!(diagnostics[0].message.starts_with("'X'"));
}

#[test]
fn shell_call_reads_a_target_specific_append_as_appending_to_that_target_alone() {
    // A target-specific '+=' appends to the target-specific value, not to the
    // file-wide one, so the ':=' below leaves it nothing to take a flavor from.
    let global = "X := base\nall: X += $(shell date)\nall:\n\t@echo $(X)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(global), global);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);

    // A target-specific ':=' for the same target is one it does take.
    let local = "all: X := base\nall: X += $(shell date)\nall:\n\t@echo $(X)\n";
    assert!(ShellInRecursiveVariable
        .check(&parse(local), local)
        .is_empty());
}

#[test]
fn shell_call_says_nothing_about_a_conditional_assignment_the_file_wide_value_answers() {
    // A target-specific '?=' reads the file-wide value as a value, so it does
    // nothing and runs no command.
    let content = "X := base\nall: X ?= $(shell date)\nall:\n\t@echo $(X)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_where_undefine_gave_the_conditional_assignment_something_to_do() {
    // 'undefine' leaves nothing behind, so the '?=' assigns after all and makes
    // a recursive variable.
    let content = "X := base\nundefine X\nX ?= $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3);

    // The same for an append: there is nothing left to append to, so Make
    // creates a recursive variable.
    let appended = "X := base\noverride undefine X\nX += $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(appended), appended);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3);
}

#[test]
fn shell_call_says_nothing_where_undefine_is_one_make_may_not_read() {
    // Make may or may not reach this 'undefine', so 'X' may still hold the
    // simple value, and the append may still be expanded once.
    let content = "X := base\nifdef CI\nundefine X\nendif\nX += $(shell date)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn directory_change_reads_an_escaped_space_as_part_of_the_word_after_it() {
    // The escaped space belongs to the word holding the '#', so the '#' starts
    // no comment and the 'cd' after it is a command the shell runs.
    let content = "all:\n\techo \\ # ; cd /tmp\n\tpwd\n";
    let diagnostics = DirectoryChangeInRecipe.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);
}

#[test]
fn directory_change_reads_a_backslash_before_a_newline_as_joining_the_lines() {
    // The shell takes the backslash and the newline out of the line, so the
    // '#' still starts its word and comments the 'cd' out.
    let joined = "all:\n\techo \\\n# ; cd /tmp\n\tpwd\n";
    assert!(
        DirectoryChangeInRecipe
            .check(&parse(joined), joined)
            .is_empty(),
        "the shell reads a comment here"
    );

    // Without the space the '#' carries on the word before the backslash, so
    // it is text and the 'cd' runs.
    let word = "all:\n\techo\\\n# ; cd /tmp\n\tpwd\n";
    let diagnostics = DirectoryChangeInRecipe.check(&parse(word), word);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3);
}

#[test]
fn shell_call_reads_an_append_over_one_of_several_targets_as_that_target_alone() {
    // 'a' takes the simple flavor the first assignment gave it, so its append
    // is expanded once, but 'b' has nothing of its own and takes a recursive
    // one, so the command runs at every reading of 'b's value.
    let shared = "a b: X := base\na: X += $(shell date)\n";
    assert!(
        ShellInRecursiveVariable
            .check(&parse(shared), shared)
            .is_empty(),
        "'a' was given a simple value by the assignment naming both targets"
    );

    // One append is expanded once for the target that has a simple value and
    // again at every reading for the one that has none, whichever of the two
    // the assignment names first.
    for partial in [
        "a: X := base\na b: X += $(shell date)\n",
        "b: X := base\na b: X += $(shell date)\n",
    ] {
        let diagnostics = ShellInRecursiveVariable.check(&parse(partial), partial);

        assert_eq!(diagnostics.len(), 1, "{partial:?} {diagnostics:?}");
        assert_eq!(diagnostics[0].line, 2, "{partial:?}");
    }
}

#[test]
fn shell_call_says_nothing_about_an_ordinary_assignment_an_override_throws_away() {
    // Make reads an ordinary assignment to a name an 'override' holds and
    // throws it away, so neither the command nor the flavor is Make's.
    for assignment in [
        "X = $(shell date)",
        "X += $(shell date)",
        "X ?= $(shell date)",
    ] {
        let content = format!("override X := base\n{assignment}\n");
        assert!(
            ShellInRecursiveVariable
                .check(&parse(&content), &content)
                .is_empty(),
            "{assignment}"
        );
    }
}

#[test]
fn shell_call_is_reported_where_an_override_assignment_replaces_an_override() {
    // An 'override' assignment is one Make does make, whatever came before it.
    let content = "override X := base\noverride X = $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);

    // The append takes the flavor the 'override' before it left, so an append
    // onto a simple value is expanded once and an append onto a recursive one
    // is expanded again at every reading.
    let simple = "override X := base\noverride X += $(shell date)\n";
    assert!(
        ShellInRecursiveVariable
            .check(&parse(simple), simple)
            .is_empty(),
        "an append onto a simple value runs the command once"
    );

    let recursive = "override X = base\noverride X += $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(recursive), recursive);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 2);
}

#[test]
fn shell_call_says_nothing_where_a_plain_undefine_leaves_an_override_standing() {
    // A plain 'undefine' cannot take the value of a name an 'override' holds,
    // so the simple flavor stands and the append after it is thrown away.
    let content = "override X := base\nundefine X\nX += $(shell date)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}

#[test]
fn shell_call_is_reported_where_an_override_undefine_takes_an_override_away() {
    // 'override undefine' leaves nothing behind, so the append has nothing to
    // append to and creates a variable Make expands at every reading.
    let content = "override X := base\noverride undefine X\nX += $(shell date)\n";
    let diagnostics = ShellInRecursiveVariable.check(&parse(content), content);

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3);
}

#[test]
fn shell_call_reads_a_file_level_undefine_as_leaving_target_specific_values_alone() {
    // The 'undefine' takes the file-wide value, which is not the one the
    // append reads: 'all' still holds the simple value of its own.
    let content = "all: X := base\nundefine X\nall: X += $(shell date)\n";

    assert!(ShellInRecursiveVariable
        .check(&parse(content), content)
        .is_empty());
}
