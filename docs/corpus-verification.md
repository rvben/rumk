# Real-project corpus verification

`scripts/audit-corpus.py` checks clean local Git checkouts with the actual Rumk
executable. It does not download projects, run Make, or write source files.
`scripts/corpus-projects.json` pins Git, Redis, Zstandard, Lua, bbolt, musl,
xxHash, cJSON, and tree-sitter
by full commit ID. Lua adds lowercase Makefiles and a language runtime;
bbolt adds Go tooling; musl adds a libc build. This is a deliberately selected
sample, not a random population. Upstream source is not copied into this repository.
The expanded inventory contains 81 tracked Makefiles; the added projects cover
hashing, a C library and its test framework, and a multilingual parser library.

Clone each manifest repository into a directory named for its manifest key and
check out the specified revision. Then run:

```sh
cargo build --release --locked
python3 scripts/audit-corpus.py \
  --manifest scripts/corpus-projects.json \
  --project /path/to/git --project /path/to/redis --project /path/to/zstd \
  --project /path/to/lua --project /path/to/bbolt --project /path/to/musl \
  --project /path/to/xxHash --project /path/to/cJSON --project /path/to/tree-sitter \
  --output /tmp/rumk-corpus-audit.json
```

`--manifest` requires all named checkouts at their pinned revisions. Omit it to
inspect other local projects. Tracked files must be clean; input hashes also cover
untracked files. Symlinked Makefiles are not selected as audit roots. The candidate
set is tracked `Makefile`, `makefile`, `GNUmakefile`, `*.mk`, and `*.make` files.
Generated Makefiles and other names are outside this sample.

For each candidate, the audit checks:

- Repeated checks return identical diagnostics and exit status.
- Checking the same bytes through stdin returns exactly the disk diagnostics.
- Formatting through stdin, with assignment spacing enabled, is idempotent.
- No source or other checkout files change during the complete audit.

Each file is an explicit root, including fragments; observations therefore use
that file's directory as the Make working directory. This is not a simulation of
every upstream build configuration. Built-in lint defaults run with `--no-config`;
formatting additionally enables MK105. No unsafe fixes run. Non-UTF-8 input, tool
errors, and timeouts fail the audit instead of silently reducing coverage.

Reports contain the binary hash, revisions, input hashes, raw diagnostics,
formatted-content hashes, platform, and timings. A warm-up precedes the measured
runs. Timings include process startup and are not comparable to another tool
without matching scope, hardware, and methodology. Diagnostics remain unreviewed
observations: passing these invariants does not establish precision, recall,
semantic equivalence of all formatting, or overall superiority.

Add `--extend-enable MK216` to review the opt-in prerequisite rule with the same
disk/stdin parity and filesystem integrity gates. Reports record the extra rules.

The separate [semantic benchmark](semantic-benchmark.md) measures named defect
detection and working controls against authored GNU Make behavior contracts.
It never executes the upstream checkouts.

## Pinned diagnostic review

`scripts/corpus-review.json` contains a small, deliberately selected set of review
labels, bound to source revisions and file hashes. Labels distinguish
actionable mechanisms, intentional patterns, false or partly false positives,
and findings requiring project context. They are test data, not a representative
sample or a claim that every diagnostic has been reviewed.

Run the audit with both `--extend-enable MK216` and `--extend-enable MK217`, then:

```sh
python3 scripts/review-corpus.py --audit /tmp/rumk-corpus-audit.json \
  --output /tmp/rumk-reviewed-findings.json
```

The reviewer rejects stale sources, differing rule profiles, failed audits, and
ambiguous identities. It reports present labels, absent reviewed findings, and
unreviewed findings separately. An absent finding is not automatically a fixed
bug; GNU behavior regressions validate the specific remediation. Aggregated
diagnostics can mix correct advice with a false-positive target, as in Lua's
stamp-file case. No population precision or recall score is calculated.

Use the [coverage audit](prerequisite-coverage.md) alongside this review to count
excluded roots and uncertain dependency outcomes. Quiet files and unreviewed
findings must not be counted as confirmed successes.

Generated reports stay outside commits. CI tests the auditor on a disposable
repository, including a Make expression that must never execute, deliberate
output instability, and an injected filesystem mutation. Run those checks with:

```sh
cargo build --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
```
