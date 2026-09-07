use rumk::diagnostic::{Diagnostic, Edit, Fix, Severity};
use rumk::fix::apply_fixes;

#[test]
fn no_fixes_preserve_content_exactly() {
    let content = ".PHONY: clean\nclean:\n\ttrue\n";
    let applied = apply_fixes(content, &[]);

    assert_eq!(applied.content, content);
    assert!(applied.fixed.is_empty());
}

#[test]
fn a_fix_overlapping_one_already_applied_is_left_for_the_next_pass() {
    let content = ".PHONY: all\n";
    let wrap = Diagnostic::new("MK101", Severity::Warning, "too long", 1, 1)
        .with_fix(Fix::new("wrap").add_edit(Edit::new(1, 1, 1, 12, ".PHONY: all \\\n        ")));
    let declare = Diagnostic::new("MK201", Severity::Warning, "missing", 1, 9)
        .with_fix(Fix::new("declare").add_edit(Edit::new(1, 9, 1, 12, "all clean")));
    let applied = apply_fixes(content, &[wrap, declare]);

    // Both rewrite the declaration, so only one of them lands. The other is
    // reported as unapplied, for a caller that re-lints to offer again.
    assert_eq!(applied.content, ".PHONY: all clean\n");
    assert_eq!(applied.fixed, vec![1]);
}

#[test]
fn a_fix_is_applied_whole_or_not_at_all() {
    let content = "all clean test:\n\t@:\n";
    let rename = Diagnostic::new("MK900", Severity::Warning, "rename", 1, 11)
        .with_fix(Fix::new("rename").add_edit(Edit::new(1, 11, 1, 15, "check")));
    let both = Diagnostic::new("MK901", Severity::Warning, "rewrite", 1, 1).with_fix(
        Fix::new("rewrite")
            .add_edit(Edit::new(1, 1, 1, 4, "build"))
            .add_edit(Edit::new(1, 11, 1, 15, "verify")),
    );
    let applied = apply_fixes(content, &[rename, both]);

    // The second fix wants a span the first one took. Applying the rest of it
    // would leave the file holding half of a fix nothing reported.
    assert_eq!(applied.content, "all clean check:\n\t@:\n");
    assert_eq!(applied.fixed, vec![0]);
}

#[test]
fn a_fix_whose_edits_want_the_same_span_is_not_applied() {
    let content = "all:\n\t@:\n";
    let conflicting = Diagnostic::new("MK900", Severity::Warning, "rewrite", 1, 1).with_fix(
        Fix::new("rewrite")
            .add_edit(Edit::new(1, 1, 1, 4, "build"))
            .add_edit(Edit::new(1, 2, 1, 3, "x")),
    );
    let applied = apply_fixes(content, &[conflicting]);

    assert_eq!(applied.content, content);
    assert!(applied.fixed.is_empty());
}

#[test]
fn a_fix_preserves_crlf_and_the_final_newline() {
    let content = "clean:\r\n    true\r\n";
    let diagnostic = Diagnostic::new("MK001", Severity::Error, "spaces", 2, 1)
        .with_fix(Fix::new("replace indentation").add_edit(Edit::new(2, 1, 2, 5, "\t")));

    assert_eq!(
        apply_fixes(content, &[diagnostic]).content,
        "clean:\r\n\ttrue\r\n"
    );
}

#[test]
fn edit_columns_are_character_based_for_utf8_input() {
    let content = "éx\n";
    let diagnostic = Diagnostic::new("TEST", Severity::Warning, "replace", 1, 2)
        .with_fix(Fix::new("replace x").add_edit(Edit::new(1, 2, 1, 3, "y")));

    assert_eq!(apply_fixes(content, &[diagnostic]).content, "éy\n");
}

#[test]
fn edits_preserve_gaps_and_order_insertions_at_shared_boundaries() {
    let content = "éx\r\nkeep\r\nlast";
    let edits = [
        Edit::new(1, 2, 1, 3, "longer"),
        Edit::new(1, 2, 1, 2, "first"),
        Edit::new(1, 2, 1, 2, "second"),
        Edit::new(3, 1, 3, 5, ""),
        Edit::new(3, 5, 3, 5, "end"),
    ];
    let diagnostics: Vec<_> = edits
        .into_iter()
        .map(|edit| {
            Diagnostic::new("TEST", Severity::Warning, "edit", 1, 1)
                .with_fix(Fix::new("edit").add_edit(edit))
        })
        .collect();
    let applied = apply_fixes(content, &diagnostics);
    assert_eq!(applied.content, "ésecondfirstlonger\r\nkeep\r\nend");
    assert_eq!(applied.fixed, [0, 1, 2, 3, 4]);
}

#[test]
fn edit_positions_handle_eof_and_reject_invalid_ranges() {
    use rumk::fix::edit_byte_range;

    for (content, edit, expected) in [
        ("", Edit::new(1, 1, 1, 1, "x"), Some((0, 0))),
        ("é\r\n", Edit::new(1, 2, 2, 1, ""), Some((2, 4))),
        ("a\n", Edit::new(2, 1, 2, 1, "x"), Some((2, 2))),
        ("a", Edit::new(1, 99, 1, 100, "x"), Some((1, 1))),
        ("a", Edit::new(0, 1, 1, 1, ""), None),
        ("a", Edit::new(1, 0, 1, 1, ""), None),
        ("a", Edit::new(1, 1, 2, 1, ""), None),
        ("a", Edit::new(1, 2, 1, 1, ""), None),
    ] {
        assert_eq!(
            edit_byte_range(content, &edit),
            expected,
            "{content:?}: {edit:?}"
        );
    }
}
