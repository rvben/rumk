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
