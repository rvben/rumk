# Contributing to rumk

Thank you for improving Rumk.

## Development workflow

1. Create a focused branch.
2. Add regression tests for behavior changes.
3. Run the full verification suite:

   ```bash
   cargo test --all-targets --all-features
   cargo clippy --all-targets --all-features -- -D warnings
   cargo fmt --all -- --check
   ```

4. Use Conventional Commits for commit messages.

The parser, the evaluator, and the fix loop are also fuzzed. `make check-fuzz`
type-checks the targets against the library, which CI runs on every commit, and
`make fuzz` runs them against generated input, which needs nightly Rust and
`cargo install cargo-fuzz`. Pass `FUZZ_TIME=<seconds>` to say how long each
target gets; the scheduled workflow runs them weekly.

Rule diagnostics must use accurate one-based character positions. Fixes must preserve source
bytes outside their declared ranges, remain idempotent, and be followed by a fresh parse and lint
pass. CLI and configuration changes should follow `docs/rumdl-compatibility.md`.

## Comparison and integration checks

`make check-comparison` runs the offline corpus and new policy/formatting tests.
The [comparison corpus guide](tests/fixtures/comparison/README.md) describes the
pinned cross-tool runner and its limits. Keep generated comparison output out of
commits. Add positive cases and valid lookalikes when changing detection.

With `pre-commit==4.6.0` installed, run `python3 scripts/verify-hooks.py`. It copies
the build inputs into temporary repositories, tests the actual Rust hook backend,
and checks formatting, include context, and configuration triggers. It never
installs hooks or changes the real repository's index. Run the Cargo checks first
so the offline hook installation can reuse downloaded crate dependencies.
For real upstream checkouts, follow [corpus verification](docs/corpus-verification.md).
The pinned sample and audit runner check repeatability, editor-buffer parity,
formatting idempotence, and input integrity without running upstream Makefiles.
