use rumk::parser::parse;
use rumk::rules::syntax::{InvalidVariableSyntax, TabInRecipe};
use rumk::rules::Rule;

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
fn make_builtin_variable_names_are_valid() {
    let content = ".RECIPEPREFIX := >\n.VARIABLES := value\n";
    let makefile = parse(content).unwrap();

    assert!(InvalidVariableSyntax.check(&makefile, content).is_empty());
}
