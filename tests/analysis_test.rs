use rumk::analysis::{ReferenceContext, ReferenceKind, SemanticIndex, StructuralIssueKind};
use rumk::parser::{parse, VariableScope};

#[test]
fn makefile_caches_its_semantic_index() {
    let makefile = parse("all: dependency\n").unwrap();

    assert!(std::ptr::eq(makefile.analysis(), makefile.analysis()));
    assert!(makefile.analysis().target("all").is_some());
}

#[test]
fn indexes_variables_targets_and_dependency_edges_deterministically() {
    let source = concat!(
        "CC := cc\n",
        "CC += -pthread\n",
        ".PHONY: all orphan\n",
        "all: app | generated\n",
        "all: docs\n",
        "app: private CFLAGS += -g\n",
    );
    let makefile = parse(source).unwrap();
    let index = SemanticIndex::build(&makefile);

    let cc = index.variable("CC").unwrap();
    assert_eq!(cc.definitions.len(), 2);
    assert_eq!(cc.definitions[0].location.line, 1);
    assert_eq!(cc.definitions[1].location.line, 2);

    let cflags = index.variable("CFLAGS").unwrap();
    assert_eq!(
        cflags.definitions[0].scope,
        VariableScope::TargetSpecific(vec!["app".into()])
    );

    let all = index.target("all").unwrap();
    assert!(all.phony);
    assert_eq!(all.declarations.len(), 2);
    assert_eq!(
        all.dependencies
            .iter()
            .map(|edge| (edge.prerequisite.as_str(), edge.order_only))
            .collect::<Vec<_>>(),
        [("app", false), ("generated", true), ("docs", false)]
    );
    assert!(index.target("orphan").unwrap().phony);
    assert!(index.target("orphan").unwrap().declarations.is_empty());

    let names: Vec<_> = index.targets.keys().map(String::as_str).collect();
    assert_eq!(names, ["all", "app", "orphan"]);
}

#[test]
fn extracts_make_references_with_context_and_exact_locations() {
    let source = concat!(
        "OBJECTS := $(patsubst %.c,%.o,$(SOURCES)) # $(IGNORED)\n",
        "FLAGS = $($(flavor)_FLAGS)\n",
        "all: $(OBJECTS)\n",
        "\t@echo $@ $(@D) $(FLAGS) $$HOME\n",
    );
    let makefile = parse(source).unwrap();
    let index = SemanticIndex::build(&makefile);

    let patsubst = index
        .references
        .iter()
        .find(|reference| reference.name == "patsubst")
        .unwrap();
    assert_eq!(patsubst.kind, ReferenceKind::Function);
    assert_eq!(patsubst.context, ReferenceContext::Assignment);
    assert_eq!((patsubst.location.line, patsubst.location.column), (1, 12));

    assert_eq!(index.references_to("SOURCES").count(), 1);
    assert_eq!(index.references_to("IGNORED").count(), 0);
    assert_eq!(index.references_to("HOME").count(), 0);

    let automatic = index
        .references
        .iter()
        .find(|reference| reference.kind == ReferenceKind::Automatic)
        .unwrap();
    assert_eq!(automatic.name, "@");
    assert_eq!(automatic.context, ReferenceContext::Recipe);
    assert_eq!((automatic.location.line, automatic.location.column), (4, 8));
    assert!(index
        .references
        .iter()
        .any(|reference| { reference.name == "@D" && reference.kind == ReferenceKind::Automatic }));

    assert!(index
        .references
        .iter()
        .any(|reference| reference.kind == ReferenceKind::Dynamic));
    assert!(index
        .references
        .iter()
        .any(|reference| reference.name == "flavor" && reference.kind == ReferenceKind::Function));
}

#[test]
fn indexes_static_and_dynamic_includes() {
    let makefile = parse("include base.mk $(wildcard config/*.mk)\n-include local.mk\n").unwrap();
    let index = SemanticIndex::build(&makefile);

    assert_eq!(index.includes.len(), 3);
    assert!(!index.includes[0].dynamic);
    assert!(index.includes[1].dynamic);
    assert!(!index.includes[2].dynamic);
    assert!(index.includes[2].optional);
}

#[test]
fn builds_nested_conditional_blocks_and_reports_malformed_structure() {
    let source = concat!(
        "ifdef OUTER\n",
        "ifeq ($(MODE),debug)\n",
        "else\n",
        "endif\n",
        "endif\n",
        "else\n",
        "endif\n",
        "ifndef OPEN\n",
    );
    let makefile = parse(source).unwrap();
    let index = SemanticIndex::build(&makefile);

    assert_eq!(index.conditional_blocks.len(), 2);
    assert_eq!(index.conditional_blocks[0].start_line, 1);
    assert_eq!(index.conditional_blocks[0].end_line, 5);
    assert_eq!(index.conditional_blocks[1].start_line, 2);
    assert_eq!(index.conditional_blocks[1].else_line, Some(3));
    assert_eq!(
        index
            .structural_issues
            .iter()
            .map(|issue| issue.kind)
            .collect::<Vec<_>>(),
        [
            StructuralIssueKind::UnexpectedElse,
            StructuralIssueKind::UnexpectedEndif,
            StructuralIssueKind::UnterminatedConditional,
        ]
    );
}

#[test]
fn accepts_else_if_chains_with_a_final_else() {
    let source = concat!(
        "ifeq ($(MODE),debug)\n",
        "else ifeq ($(MODE),release)\n",
        "else ifdef CI\n",
        "else\n",
        "endif\n",
    );
    let index = SemanticIndex::build(&parse(source).unwrap());

    assert!(index.structural_issues.is_empty());
    assert_eq!(index.conditional_blocks.len(), 1);
    assert_eq!(index.conditional_blocks[0].branch_lines, [2, 3, 4]);
    assert_eq!(index.conditional_blocks[0].else_line, Some(4));
}

#[test]
fn finds_only_concrete_dependency_cycles() {
    let source = concat!(
        "alpha: beta\n",
        "beta: gamma\n",
        "gamma: alpha\n",
        "self: self\n",
        "leaf: external\n",
        "pattern-%: pattern-%\n",
        "dynamic: $(LATER)\n",
    );
    let index = SemanticIndex::build(&parse(source).unwrap());

    assert_eq!(
        index.dependency_cycles(),
        [
            vec![
                String::from("alpha"),
                String::from("beta"),
                String::from("gamma")
            ],
            vec![String::from("self")]
        ]
    );
}
