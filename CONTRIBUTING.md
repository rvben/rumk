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

Rule diagnostics must use accurate one-based character positions. Fixes must preserve source
bytes outside their declared ranges, remain idempotent, and be followed by a fresh parse and lint
pass. CLI and configuration changes should follow `docs/rumdl-compatibility.md`.
