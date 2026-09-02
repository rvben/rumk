use rumk::logical::{ConditionalKind, IncludeKind, LogicalDocument, LogicalKind};
use rumk::syntax::SyntaxTree;

#[test]
fn folds_continuations_without_losing_the_original_source() {
    let source = "SOURCES := one.c    \\\r\n  two.c \t\\\r\n\tthree.c\r\n";
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
fn preserves_a_dangling_backslash_without_a_following_line() {
    let source = "VALUE := unfinished\\";
    let syntax = SyntaxTree::parse(source);
    let document = LogicalDocument::parse(&syntax);

    assert_eq!(document.statements()[0].text(), source);
    assert_eq!(document.statements()[0].raw(source), source);
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

fn kinds(source: &str) -> Vec<LogicalKind> {
    LogicalDocument::parse(&SyntaxTree::parse(source))
        .statements()
        .iter()
        .map(|statement| statement.kind)
        .collect()
}

#[test]
fn prefixed_lines_without_an_open_rule_are_statements_or_orphans() {
    let source = "\techo\n\tall: dep\n\tFOO = 1\n\t# note\n\tifeq (1,1)\n\tendif\n\t\\\n";

    assert_eq!(
        kinds(source),
        [
            LogicalKind::OrphanRecipe,
            LogicalKind::OrphanRecipe,
            LogicalKind::Assignment,
            LogicalKind::Comment,
            LogicalKind::Conditional(ConditionalKind::Ifeq),
            LogicalKind::Conditional(ConditionalKind::Endif),
            LogicalKind::Blank,
        ]
    );
}

#[test]
fn load_lines_are_directives() {
    assert_eq!(
        kinds("load plugin.so\n-load plugin.so(init)\nexport load = 1\n"),
        [
            LogicalKind::Directive,
            LogicalKind::Directive,
            LogicalKind::Assignment,
        ]
    );
}

#[test]
fn an_assignment_is_found_before_any_keyword() {
    assert_eq!(
        kinds("ifdef = 1\ninclude := 2\ndefine = 3\nvpath ?= 4\nexport += 5\nendif\n"),
        [
            LogicalKind::Assignment,
            LogicalKind::Assignment,
            LogicalKind::Assignment,
            LogicalKind::Assignment,
            LogicalKind::Assignment,
            LogicalKind::Conditional(ConditionalKind::Endif),
        ]
    );
}

/// Make reads a name up to the first whitespace outside a reference. What is
/// left is a rule when the operator supplies a colon, an `export` directive
/// when a modifier leads, and otherwise nothing it can read.
#[test]
fn a_name_ends_at_whitespace_outside_a_reference() {
    assert_eq!(
        kinds(
            "two words = 1\ntwo words := 2\nexport a b = 3\n$(a b)$(c (d) e) = 4\nall: a b = 5\n"
        ),
        [
            LogicalKind::Unknown,
            LogicalKind::Rule,
            LogicalKind::Directive,
            LogicalKind::Assignment,
            LogicalKind::Rule,
        ]
    );
}

#[test]
fn a_comment_ends_the_line_before_separators_are_looked_for() {
    assert_eq!(
        kinds("garbage # x: y\nFOO # := 1\nexport # FOO := 1\nX = 1 # y: z\n"),
        [
            LogicalKind::Unknown,
            LogicalKind::Unknown,
            LogicalKind::Directive,
            LogicalKind::Assignment,
        ]
    );
}

#[test]
fn a_prefixed_continuation_into_the_end_of_the_file_needs_its_newline() {
    assert_eq!(kinds("\t\\"), [LogicalKind::OrphanRecipe]);
    assert_eq!(kinds("\t\\\n"), [LogicalKind::Blank]);
    assert_eq!(
        kinds("all:\n\t\\\n"),
        [LogicalKind::Rule, LogicalKind::Recipe]
    );
}

#[test]
fn a_continuation_into_the_end_of_the_file_drops_its_backslash() {
    let source = "X := a \\\n";
    let document = LogicalDocument::parse(&SyntaxTree::parse(source));

    assert_eq!(document.statements()[0].text(), "X := a");
}

#[test]
fn statements_close_the_open_rule_while_comments_and_conditionals_keep_it() {
    assert_eq!(
        kinds("all:\n\t@true\nFOO = 1\n\t@true\n"),
        [
            LogicalKind::Rule,
            LogicalKind::Recipe,
            LogicalKind::Assignment,
            LogicalKind::OrphanRecipe,
        ]
    );
    assert_eq!(
        kinds("all: FOO = 1\n\t@true\n"),
        [LogicalKind::Rule, LogicalKind::OrphanRecipe]
    );
    assert_eq!(
        kinds("all:\n# note\n\nifdef X\n\t@true\nendif\n\t@true\n"),
        [
            LogicalKind::Rule,
            LogicalKind::Comment,
            LogicalKind::Blank,
            LogicalKind::Conditional(ConditionalKind::Ifdef),
            LogicalKind::Recipe,
            LogicalKind::Conditional(ConditionalKind::Endif),
            LogicalKind::Recipe,
        ]
    );
}

#[test]
fn space_indented_text_is_a_recipe_only_under_an_open_rule() {
    assert_eq!(
        kinds("all:\n    echo\n"),
        [LogicalKind::Rule, LogicalKind::Recipe]
    );
    assert_eq!(kinds("    echo\n"), [LogicalKind::Unknown]);
}
