#![no_main]

//! Linting arbitrary bytes must not crash.
//!
//! The bytes reach the rules the way `rumk check` hands a file to them: the
//! byte order mark is split off and anything that is not UTF-8 is decoded
//! lossily, so the whole pipeline runs over hostile input, from the parser and
//! the semantic index through every rule enabled by default.
//!
//! Nothing here bounds the input size. An input Rumk cannot survive is a
//! finding rather than noise, and libFuzzer's own `-max_len` decides how large
//! the inputs it generates are.

use std::collections::BTreeSet;
use std::path::Path;

use libfuzzer_sys::fuzz_target;
use rumk::config::Config;
use rumk::lint::{lint, LintContext};
use rumk::source;

fuzz_target!(|data: &[u8]| {
    let (bytes, _) = source::split_byte_order_mark(data);
    let text = String::from_utf8_lossy(bytes);
    let config = Config::default();
    let covered_files = BTreeSet::new();
    // The project pass reads the files a Makefile includes, so it stays out of
    // reach here: an `include` line the fuzzer wrote would name paths on the
    // host running it, which is neither reproducible nor its business.
    let context = LintContext {
        config: &config,
        path: Path::new("Makefile"),
        project_root: false,
        contextual: false,
        layout_only: false,
        covered_files: &covered_files,
    };

    // An inline configuration comment can name a rule that does not exist,
    // which is an error the run reports rather than a failure to lint.
    let _ = lint(&text, &context);
});
