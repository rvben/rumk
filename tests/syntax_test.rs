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
fn a_define_keyword_before_an_operator_names_a_variable() {
    let tree = SyntaxTree::parse("define = 1\noverride define := 2\nX := 3\n");
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Assignment,
            SyntaxKind::Assignment,
            SyntaxKind::Assignment,
        ]
    );

    let tree = SyntaxTree::parse("define X =\nbody\nendef\ndefine\nendef\n");
    let kinds: Vec<_> = tree.nodes().iter().map(|node| node.kind).collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::Define,
            SyntaxKind::DefineBody,
            SyntaxKind::Endef,
            SyntaxKind::Define,
            SyntaxKind::Endef,
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
fn reports_the_recipe_prefix_and_whether_it_is_known() {
    let prefixes = |source: &str| {
        let tree = SyntaxTree::parse(source);
        (1..=tree.nodes().len())
            .map(|line| {
                let prefix = tree.recipe_prefix_at(line);
                (prefix.character, prefix.known)
            })
            .collect::<Vec<_>>()
    };

    // A file without an assignment is read with a tab throughout, and a line
    // past the end reports the prefix Make starts with.
    assert_eq!(prefixes("all:\n\t@echo ok\n"), [('\t', true), ('\t', true)]);
    assert_eq!(
        SyntaxTree::parse("all:\n").recipe_prefix_at(9).character,
        '\t'
    );
    // An assignment takes effect on the following line.
    assert_eq!(
        prefixes(".RECIPEPREFIX := >\nall:\n>@echo ok\n"),
        [('\t', true), ('>', true), ('>', true)]
    );
    // A value Rumk cannot evaluate leaves the prefix unknown.
    assert_eq!(
        prefixes("P := >\n.RECIPEPREFIX := $(P)\nall:\n"),
        [('\t', true), ('\t', true), ('\t', false)]
    );
    // A conditional assignment gives the character the author indented for,
    // without claiming Make reads the file with it.
    assert_eq!(
        prefixes("ifdef X\n.RECIPEPREFIX = >\nendif\nall:\n"),
        [('\t', true), ('\t', true), ('>', false), ('>', false)]
    );
}

#[test]
fn reads_the_recipe_prefix_the_way_make_stores_the_value() {
    let prefix = |source: &str| {
        let tree = SyntaxTree::parse(source);
        let prefix = tree.recipe_prefix_at(tree.nodes().len());
        (prefix.character, prefix.known)
    };

    // Make strips the comment, so the value is empty and the prefix is a tab.
    assert_eq!(prefix(".RECIPEPREFIX := # reset\nall:\n"), ('\t', true));
    assert_eq!(prefix(".RECIPEPREFIX = > # keep\nall:\n"), ('>', true));
    // An escaped hash is the value Make stores, not a comment.
    assert_eq!(prefix(".RECIPEPREFIX = \\#\nall:\n"), ('#', true));
    // A hash inside a reference is part of the value.
    assert_eq!(prefix(".RECIPEPREFIX = $(x)#c\nall:\n"), ('$', true));

    // `+=` on the simply expanded value Make starts with expands what it
    // appends, so a reference leaves a character Rumk cannot read, while the
    // recursive value a `=` leaves behind keeps the reference as text.
    assert_eq!(
        prefix("P = >\n.RECIPEPREFIX += $(P)\nall:\n"),
        ('\t', false)
    );
    assert_eq!(
        prefix("P = >\n.RECIPEPREFIX :=\n.RECIPEPREFIX += $(P)\nall:\n"),
        ('\t', false)
    );
    assert_eq!(
        prefix("P = >\n.RECIPEPREFIX =\n.RECIPEPREFIX += $(P)\nall:\n"),
        ('$', true)
    );
    assert_eq!(prefix(".RECIPEPREFIX += >\nall:\n"), ('>', true));

    // A conditional statement that cannot change the character it would have
    // read anyway leaves the prefix as certain as it was.
    assert_eq!(
        prefix(".RECIPEPREFIX = >\nifdef X\n.RECIPEPREFIX += !\nendif\nall:\n"),
        ('>', true)
    );
    assert_eq!(
        prefix(".RECIPEPREFIX = >\nifdef X\n.RECIPEPREFIX = >\nendif\nall:\n"),
        ('>', true)
    );
    assert_eq!(
        prefix(".RECIPEPREFIX = >\nifdef X\n.RECIPEPREFIX = !\nendif\nall:\n"),
        ('!', false)
    );
    // An undecided branch that may have made the value simply expanded leaves
    // a later append undecided too.
    assert_eq!(
        prefix("P = >\n.RECIPEPREFIX =\nifdef X\n.RECIPEPREFIX :=\nendif\n.RECIPEPREFIX += $(P)\nall:\n"),
        ('\t', false)
    );
}

#[test]
fn semantic_parser_exposes_the_lossless_tree() {
    let source = "name := naïve\nall:\n\t@echo $(name)\n";
    let makefile = parser::parse(source);

    assert_eq!(makefile.syntax.source(), source);
    assert_eq!(makefile.syntax.render(), source);
    assert_eq!(makefile.variables["name"].value, "naïve");
    assert_eq!(makefile.rules[0].targets, ["all"]);
}
