# Real-project corpus verification

`scripts/audit-corpus.py` checks clean local Git checkouts with the actual Rumk
executable. It does not download projects, run Make, or write source files.
`scripts/corpus-projects.json` pins a sample of Git, Redis, and Zstandard by full
commit ID. Upstream source is not copied into this repository.

Clone each manifest repository into a directory named for its manifest key and
check out the specified revision. Then run:

```sh
cargo build --release --locked
python3 scripts/audit-corpus.py \
  --manifest scripts/corpus-projects.json \
  --project /path/to/git --project /path/to/redis --project /path/to/zstd \
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

Generated reports stay outside commits. CI tests the auditor on a disposable
repository, including a Make expression that must never execute, deliberate
output instability, and an injected filesystem mutation. Run those checks with:

```sh
cargo build --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
```
