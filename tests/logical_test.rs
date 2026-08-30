use rumk::logical::{ConditionalKind, IncludeKind, LogicalDocument, LogicalKind};
use rumk::syntax::SyntaxTree;

#[test]
fn folds_continuations_without_losing_the_original_source() {
    let source = "SOURCES := one.c \\\r\n  two.c \\\r\n\tthree.c\r\n";
    let syntax = SyntaxTree::parse(source);
    let document = LogicalDocument::parse(&syntax);
    let statement = &document.statements()[0];

    assert_eq!(document.statements().len(), 1);
    assert_eq!(statement.kind, LogicalKind::Assignment);
    assert_eq!(statement.start_line, 1);
    assert_eq!(statement.end_line, 3);
    assert_eq!(statement.text(), "SOURCES := one.c two.c three.c");
    assert_eq!(statement.raw(source), source);
}

#[test]
fn ignores_delimiters_inside_nested_make_expansions() {
    let source = concat!(
        "RESULT := $(call choose,a:b=c,$(inner:x=y))\n",
        "$(call target,a:b=c): $(call deps,x:y=z) | stamp\n",
    );
    let syntax = SyntaxTree::parse(source);
    let document = LogicalDocument::parse(&syntax);

    assert_eq!(document.statements()[0].kind, LogicalKind::Assignment);
    assert_eq!(document.statements()[1].kind, LogicalKind::Rule);
}

#[test]
fn groups_a_continued_recipe_as_one_statement() {
    let source = "all:\n\tprintf '%s\\n' one \\\n\t  two\n";
    let syntax = SyntaxTree::parse(source);
    let document = LogicalDocument::parse(&syntax);

    assert_eq!(document.statements().len(), 2);
    let recipe = &document.statements()[1];
    assert_eq!(recipe.kind, LogicalKind::Recipe);
    assert_eq!(recipe.start_line, 2);
    assert_eq!(recipe.end_line, 3);
    assert_eq!(recipe.raw(source), "\tprintf '%s\\n' one \\\n\t  two\n");
}

#[test]
fn classifies_includes_and_conditionals() {
    let source = concat!(
        "include base.mk \\\n  $(wildcard config/*.mk)\n",
        "-include local.mk\n",
        "ifeq ($(MODE),debug)\n",
        "else\n",
        "endif\n",
    );
    let syntax = SyntaxTree::parse(source);
    let document = LogicalDocument::parse(&syntax);
    let kinds: Vec<_> = document
        .statements()
        .iter()
        .map(|statement| statement.kind)
        .collect();

    assert_eq!(
        kinds,
        vec![
            LogicalKind::Include(IncludeKind::Required),
            LogicalKind::Include(IncludeKind::Optional),
            LogicalKind::Conditional(ConditionalKind::Ifeq),
            LogicalKind::Conditional(ConditionalKind::Else),
            LogicalKind::Conditional(ConditionalKind::Endif),
        ]
    );
}
