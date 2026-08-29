use rumk::parser;
use rumk::syntax::{LineEnding, SourcePosition, SyntaxKind, SyntaxTree};

#[test]
fn losslessly_round_trips_common_line_endings() {
    for source in [
        "target: dep\n\t@echo $<\n",
        "target: dep\r\n\t@echo $<\r\n",
        "target: dep\n\t@echo $<",
        "",
    ] {
        let tree = SyntaxTree::parse(source);
        assert_eq!(tree.render().as_bytes(), source.as_bytes());

        let reconstructed = tree
            .nodes()
            .iter()
            .map(|node| node.text(tree.source()))
            .collect::<String>();
        assert_eq!(reconstructed.as_bytes(), source.as_bytes());
    }
}

#[test]
fn records_exact_content_and_full_line_spans() {
    let source = "# café\r\nall:\n\t@echo ok";
    let tree = SyntaxTree::parse(source);
    let nodes = tree.nodes();

    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].content(source), "# café");
    assert_eq!(nodes[0].text(source), "# café\r\n");
    assert_eq!(nodes[0].line_ending, LineEnding::CrLf);
    assert_eq!(nodes[0].content_span.end.column, 7);
    assert_eq!(
        nodes[0].span.end,
        SourcePosition {
            offset: 9,
            line: 2,
            column: 1
        }
    );

    assert_eq!(nodes[1].content(source), "all:");
    assert_eq!(nodes[1].line_ending, LineEnding::Lf);
    assert_eq!(nodes[2].content(source), "\t@echo ok");
    assert_eq!(nodes[2].line_ending, LineEnding::None);
    assert_eq!(nodes[2].span.end.line, 3);
    assert_eq!(nodes[2].span.end.column, 10);
}

#[test]
fn classifies_source_order_without_discarding_unknown_syntax() {
    let source = concat!(
        "\n",
        "# Build everything\n",
        "CC := cc\n",
        "include local.mk\n",
        "ifdef DEBUG\n",
        "all: main.o\n",
        "\t$(CC) $^ -o $@\n",
        "$(generated-line)\n",
        "endif\n",
    );
    let tree = SyntaxTree::parse(source);
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Blank,
            SyntaxKind::Comment,
            SyntaxKind::Assignment,
            SyntaxKind::Include,
            SyntaxKind::Conditional,
            SyntaxKind::Rule,
            SyntaxKind::Recipe,
            SyntaxKind::Unknown,
            SyntaxKind::Conditional,
        ]
    );
}

#[test]
fn treats_define_contents_as_opaque_source() {
    let source = concat!(
        "override define PROGRAM\n",
        "target: this-is-data\n",
        "\techo still-data\n",
        "define NESTED\n",
        "endef\n",
        "endef\n",
    );
    let tree = SyntaxTree::parse(source);
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Define,
            SyntaxKind::DefineBody,
            SyntaxKind::DefineBody,
            SyntaxKind::Define,
            SyntaxKind::Endef,
            SyntaxKind::Endef,
        ]
    );
}

#[test]
fn recognizes_prefixed_assignments_and_directives() {
    let source = concat!(
        "export CC := clang\n",
        "private CFLAGS += -g\n",
        "unexport INTERNAL\n",
    );
    let tree = SyntaxTree::parse(source);
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Assignment,
            SyntaxKind::Assignment,
            SyntaxKind::Directive,
        ]
    );
}

#[test]
fn classifies_custom_recipe_prefixes_contextually() {
    let source = ".RECIPEPREFIX := >\nall:\n>@echo ok\n";
    let tree = SyntaxTree::parse(source);
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![SyntaxKind::Assignment, SyntaxKind::Rule, SyntaxKind::Recipe]
    );
}

#[test]
fn semantic_parser_exposes_the_lossless_tree() {
    let source = "name := naïve\nall:\n\t@echo $(name)\n";
    let makefile = parser::parse(source).unwrap();

    assert_eq!(makefile.syntax.source(), source);
    assert_eq!(makefile.syntax.render(), source);
    assert_eq!(makefile.variables["name"].value, "naïve");
    assert_eq!(makefile.rules[0].targets, ["all"]);
}
