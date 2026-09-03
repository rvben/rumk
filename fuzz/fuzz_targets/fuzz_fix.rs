#![no_main]

//! Fixing arbitrary text must settle, and must not break the file.
//!
//! `rumk fmt` re-lints after every fix pass until no fix changes the text any
//! more, so a run that ends in an error is two rules undoing each other's work
//! rather than a file Rumk cannot format. A fix must also never leave behind a
//! statement GNU Make would refuse to read: MK006 reports those, and the count
//! it reports may only fall.
//!
//! Only text that is already valid UTF-8 is fixed, which is what `rumk fmt`
//! does: a file it had to decode lossily is never written back.

use std::collections::BTreeSet;
use std::path::Path;

use libfuzzer_sys::fuzz_target;
use rumk::config::Config;
use rumk::diagnostic::Diagnostic;
use rumk::lint::{self, LintContext};
use rumk::source;

fuzz_target!(|data: &[u8]| {
    let (bytes, _) = source::split_byte_order_mark(data);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let config = Config::default();
    let covered_files = BTreeSet::new();
    let context = LintContext {
        config: &config,
        path: Path::new("Makefile"),
        project_root: false,
        contextual: false,
        layout_only: false,
        covered_files: &covered_files,
    };

    let Ok(diagnostics) = lint::lint(text, &context) else {
        return;
    };
    let before = syntax_errors(&diagnostics);
    let fixed = match lint::fix(text, diagnostics, &context) {
        Ok(fixed) => fixed,
        Err(error) => panic!("fixes did not settle: {error:#}\ninput: {text:?}"),
    };
    let after = syntax_errors(&fixed.diagnostics);
    assert!(
        after <= before,
        "fixing introduced a statement GNU Make rejects ({before} before, {after} after)\n\
         input: {text:?}\noutput: {:?}",
        fixed.content
    );
});

/// How many statements GNU Make would refuse to read the text holds.
fn syntax_errors(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == "MK006")
        .count()
}
