use rumk::diagnostic::{Diagnostic, Severity};
use rumk::inline_config::apply_inline_suppressions;

fn diagnostic(rule: &str, line: usize) -> Diagnostic {
    Diagnostic::new(rule, Severity::Warning, "test", line, 1)
}

#[test]
fn disable_enable_and_next_line_directives_are_scoped() {
    let content = "# rumk-disable MK201\nclean:\n# rumk-enable MK201\ntest:\n# rumk-disable-next-line MK101\nlong line\nlast line\n";
    let diagnostics = vec![
        diagnostic("MK201", 2),
        diagnostic("MK201", 4),
        diagnostic("MK101", 6),
        diagnostic("MK101", 7),
    ];

    let remaining = apply_inline_suppressions(content, diagnostics).unwrap();
    assert_eq!(
        remaining
            .iter()
            .map(|diagnostic| (diagnostic.rule_id.as_str(), diagnostic.line))
            .collect::<Vec<_>>(),
        [("MK201", 4), ("MK101", 7)]
    );
}

#[test]
fn recipe_shell_comments_are_not_treated_as_rumk_directives() {
    let content = "target:\n\t# rumk-disable MK001\n";
    let diagnostics = vec![diagnostic("MK001", 2)];

    assert_eq!(
        apply_inline_suppressions(content, diagnostics)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn unknown_inline_rules_are_configuration_errors() {
    let error = apply_inline_suppressions("# rumk-disable MK999\n", Vec::new()).unwrap_err();
    assert!(error.contains("Unknown rule"));
}
