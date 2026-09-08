use rumk::{
    fix::apply_fixes,
    parser::parse,
    rules::{formatting::RuleSpacing, Rule},
};
#[test]
fn static_rule_spacing_is_idempotent_and_preserves_recipe_suffixes() {
    for (source, expected) in [
        ("all  :  one   two\n", "all: one two\n"),
        (
            "all   other : one\t two  |  ready\r\n",
            "all other: one two | ready\r\n",
        ),
        (
            "all  :  one ;  @echo '$@ 😀'  ",
            "all: one ;  @echo '$@ 😀'  ",
        ),
        ("all :    # comment\n", "all: # comment\n"),
    ] {
        let ds = RuleSpacing.check(&parse(source), source);
        assert_eq!(ds.len(), 1);
        let fixed = apply_fixes(source, &ds).content;
        assert_eq!(fixed, expected);
        assert!(RuleSpacing.check(&parse(&fixed), &fixed).is_empty());
        let before = parse(source);
        let after = parse(&fixed);
        assert_eq!(before.rules[0].targets, after.rules[0].targets);
        assert_eq!(before.rules[0].prerequisites, after.rules[0].prerequisites);
        assert_eq!(
            before.rules[0].order_only_prerequisites,
            after.rules[0].order_only_prerequisites
        );
    }
}
#[test]
fn expansion_pattern_assignment_escape_and_continuation_lookalikes_are_untouched() {
    for source in [
        "all: X = significant  value\n",
        "$(NAME) :  dep\n",
        "all :  $(INPUTS)\n",
        "a\\ b :  c\\ d\n",
        "%.o :  %.c\n",
        "all :: dep\n",
        "all &: dep\n",
        "all : a \\\n b\n",
        "define RULE\nall  :  dep\nendef\n",
        "all:\n\t@echo a : b\n",
    ] {
        assert!(
            RuleSpacing.check(&parse(source), source).is_empty(),
            "{source}"
        );
    }
}
#[test]
fn gnu_make_observes_identical_static_rules_before_and_after_formatting() {
    use std::process::Command;
    if Command::new("make").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = "all  :  one   two  |  setup ; @echo '$@:$^:$<'\none two setup:;\n";
    let fixed = apply_fixes(source, &RuleSpacing.check(&parse(source), source)).content;
    let run = |text: &str| {
        std::fs::write(dir.path().join("Makefile"), text).unwrap();
        Command::new("make")
            .current_dir(dir.path())
            .args(["-rRn", "all"])
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .output()
            .unwrap()
    };
    let before = run(source);
    let after = run(&fixed);
    assert!(before.status.success());
    assert_eq!(before.status.code(), after.status.code());
    assert_eq!(before.stdout, after.stdout);
    assert_eq!(before.stderr, after.stderr);
}
