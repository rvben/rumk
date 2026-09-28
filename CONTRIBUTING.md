# Contributing to rumk

Thank you for improving Rumk.

## Development workflow

1. Create a focused branch.
2. Add regression tests for behavior changes.
3. Run the full verification suite, which is exactly what CI runs:

   ```bash
   make ci
   ```

   Each CI job is one make target (`ci-quality`, `ci-test`, `ci-compat`,
   `msrv-check`, `ci-package`), so a failing job can be rerun on its own.
   The jobs install the pinned `pre-commit` and `maturin` into `target/ci-tools`
   themselves; Rust 1.82.0 must be installed for `msrv-check`. `ci-package` validates
   the crate package, which refuses uncommitted changes; set `ALLOW_DIRTY=1`
   to run it before committing.

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

`make check-hooks` (part of `ci-quality`) runs `scripts/verify-hooks.py`. It copies
the build inputs into temporary repositories, tests the actual Rust hook backend,
and checks formatting, include context, and configuration triggers. It never
installs hooks or changes the real repository's index. Run the Cargo checks first
so the offline hook installation can reuse downloaded crate dependencies.
For real upstream checkouts, follow [corpus verification](docs/corpus-verification.md).
The pinned sample and audit runner check repeatability, editor-buffer parity,
formatting idempotence, and input integrity without running upstream Makefiles.

When building outside the workspace's `target/` directory, select those binaries
for the Python regression suite so it does not skip integration checks or use a
stale workspace build:

```sh
RUMK_TEST_BINARY=/path/to/build/debug/rumk \
RUMK_COVERAGE_BINARY=/path/to/build/debug/examples/prerequisite-coverage \
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Without these overrides, the tests use the usual `target/debug` paths.

## Private local reports

Audit results, benchmark measurements, comparison write-ups, and diagnostic review
labels are private local working material. Store them under `reports/private/`
(which is ignored at the repository root) or outside the repository. Never commit
them or place them in `docs/`. This applies to hand-written reports as well as
machine-generated output.

Commit reusable verification tools, synthetic regression fixtures, and product
or methodology documentation. Before committing, inspect the staged paths and
diff for private results; ignore rules do not cover files already tracked or
added with `git add -f`.
