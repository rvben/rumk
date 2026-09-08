# Makefile comparison corpus

Small, authored regression fixtures shared by `comparison_corpus_test.rs` and
`scripts/compare-linters.py`. These are synthetic probes, not sampled production
projects. The separate `tests/fixtures/corpus` suite covers production-style
multi-file projects. No competitor source code is embedded here.

The `mixed-phony`, `compiled-command-name`, and `compiler-output-lookalike`
cases are authored reductions from reviewing the pinned upstream sample. They
check that explicit phony declarations and real compiler outputs are not confused
with missing command-target declarations. `tests/phony_precision_test.rs` covers
includes, inactive branches, output lookalikes, and GNU Make behavior as well.

`manifest.json` states the purpose, dialect, selected rumk rules, and exact
expected rule/file/line tuples. Every defect has valid lookalikes. The runner
uses temporary copies, records input hashes and commands, and rejects input
mutations in check mode. Fixtures must remain harmless even if a validator
invokes Make; never include destructive recipes or side-effecting Make functions.

The `phony-rebuild`, `phony-order-only`, and `phony-consumer` cases distinguish
ordinary phony dependencies from order-only setup and phony consumers. The
recipe-prefix cases distinguish repeated flags from unique flags and shell text
on continuation lines. GNU Make behavior tests live in `rebuild_test.rs` and
`recipe_prefixes_test.rs`.

Run the offline regression suite:

```sh
cargo test --test comparison_corpus_test --test formatting_test --test policy_test --test phony_precision_test --test rebuild_test --test recipe_prefixes_test
```

Run all four tools after installing the pinned versions explicitly:

```sh
cargo build
python3 scripts/compare-linters.py --require-all \
  --checkmake /path/to/checkmake-0.3.2 \
  --unmake /path/to/unmake-0.0.27 \
  --mbake /path/to/mbake-1.4.6 \
  --output /tmp/rumk-comparison.json
```

The script does not install anything. It rejects mismatched competitor versions,
records executable hashes, fixes Python hash ordering and terminal settings, and reports omissions unless `--require-all` requires
all tools. Repeat with `--baseline /tmp/rumk-comparison.json` and a different
output path to check observation stability. Update pins deliberately in the
runner and this document together; a version mismatch never silently upgrades.
The baseline compares checkmake's complete JSON diagnostic multiset because its
diagnostics can vary in order. Raw output is retained unchanged; duplicate,
added, removed, or changed findings still affect the comparison. Other tools'
output is compared in its original order.

Checkmake is configured with `required = test` and `maxBodyLength = 2`. Rumk uses
the corresponding settings with per-case rule selection. Unmake runs its normal
static checks (never `--dry-run`); mbake runs `format --check` with an explicit
default configuration (never `--validate`). Both receive the same root Makefile,
as does checkmake; include traversal is each tool's responsibility.

Competitor output is evidence, not a correctness oracle. POSIX rejection of a
GNU fixture is a dialect distinction, and mbake's formatting changes need not
be build defects. Extra convention warnings are not automatically false positives.
Only the declared rumk expectation is an assertion of correctness. Inspect raw
messages and case intent before drawing conclusions; do not rank tools by exit
status, warning count, or these deliberately small samples. Generated reports
remain local working material and must not be committed.
