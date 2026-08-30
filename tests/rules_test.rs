use rumk::fix::apply_fixes;
use rumk::parser::parse;
use rumk::rules::best_practices::{DependencyCycle, DuplicateRecipe, MissingPhony, RecursiveMake};
use rumk::rules::style::LineLength;
use rumk::rules::syntax::{
    ConditionalStructure, InvalidVariableSyntax, SpecialTargetPlacement, TabInRecipe,
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
    let makefile = parse(content).unwrap();

    let diagnostics = LineLength::new(20).check(&makefile, content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].line, 2);
}

#[test]
fn tab_rule_accepts_inline_and_custom_prefix_recipes() {
    let content = ".RECIPEPREFIX := >\nall: ; @echo inline\n>@echo prefixed\n";
    let makefile = parse(content).unwrap();

    assert!(TabInRecipe.check(&makefile, content).is_empty());
}

#[test]
fn tab_rule_still_fixes_the_complete_space_prefix() {
    let content = "all:\n    echo wrong\n";
    let makefile = parse(content).unwrap();
    let diagnostics = TabInRecipe.check(&makefile, content);

    assert_eq!(diagnostics.len(), 1);
    let edit = &diagnostics[0].fix.as_ref().unwrap().edits[0];
    assert_eq!(edit.start_column, 1);
    assert_eq!(edit.end_column, 5);
    assert_eq!(edit.replacement, "\t");
}

#[test]
fn missing_phony_rule_groups_targets_and_preserves_crlf() {
    let content = "all clean:\r\n\t@:\r\n";
    let makefile = parse(content).unwrap();
    let diagnostics = MissingPhony.check(&makefile, content);

    assert!(MissingPhony.fixable());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].fixable);
    assert_eq!(diagnostics[0].fix.as_ref().unwrap().edits.len(), 1);
    assert_eq!(
        apply_fixes(content, &diagnostics),
        ".PHONY: all clean\r\nall clean:\r\n\t@:\r\n"
    );
}

#[test]
fn make_builtin_variable_names_are_valid() {
    let content = ".RECIPEPREFIX := >\n.VARIABLES := value\n";
    let makefile = parse(content).unwrap();

    assert!(InvalidVariableSyntax.check(&makefile, content).is_empty());
}

#[test]
fn conditional_structure_reports_only_malformed_blocks() {
    let valid = "ifdef A\nifeq ($(MODE),debug)\nelse\nendif\nendif\n";
    assert!(ConditionalStructure
        .check(&parse(valid).unwrap(), valid)
        .is_empty());

    let invalid = "else\nendif\nifndef OPEN\n";
    let diagnostics = ConditionalStructure.check(&parse(invalid).unwrap(), invalid);
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
    let diagnostics = RecursiveMake.check(&parse(content).unwrap(), content);

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
        apply_fixes(content, &diagnostics),
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
    let diagnostics = RecursiveMake.check(&parse(content).unwrap(), content);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].fix.as_ref().unwrap().edits.len(), 2);
    assert_eq!(
        apply_fixes(content, &diagnostics),
        ".PHONY: all\nall: ; $(MAKE) first && env MODE=debug $(MAKE) second\n"
    );
}

#[test]
fn recursive_make_reports_but_does_not_rewrite_continued_recipes() {
    let content = ".PHONY: all\nall:\n\tmake \\\n\t  -C sub\n";
    let diagnostics = RecursiveMake.check(&parse(content).unwrap(), content);

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
    let diagnostics = DuplicateRecipe.check(&parse(content).unwrap(), content);

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
    let makefile = parse(content).unwrap();

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
    let diagnostics = DependencyCycle.check(&parse(content).unwrap(), content);

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
        .check(&parse(valid).unwrap(), valid)
        .is_empty());
    let diagnostics = SpecialTargetPlacement.check(&parse(invalid).unwrap(), invalid);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].rule_id, "MK005");
}
