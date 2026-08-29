use rumk::diagnostic::{Diagnostic, Edit, Fix, Severity};
use rumk::fix::apply_fixes;

#[test]
fn no_fixes_preserve_content_exactly() {
    let content = ".PHONY: clean\nclean:\n\ttrue\n";
    assert_eq!(apply_fixes(content, &[]), content);
}

#[test]
fn a_fix_preserves_crlf_and_the_final_newline() {
    let content = "clean:\r\n    true\r\n";
    let diagnostic = Diagnostic::new("MK001", Severity::Error, "spaces", 2, 1)
        .with_fix(Fix::new("replace indentation").add_edit(Edit::new(2, 1, 2, 5, "\t")));

    assert_eq!(apply_fixes(content, &[diagnostic]), "clean:\r\n\ttrue\r\n");
}
