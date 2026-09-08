# Reproducible cross-tool performance

`scripts/benchmark-linters.py` measures CLI wall time and per-process peak resident
memory on clean, pinned real-project checkouts. It complements the generated
workloads in `scripts/benchmark.py` and the behavior oracle in
`scripts/semantic-benchmark.py`.

```sh
cargo build --release --locked
python3 scripts/benchmark-linters.py \
  --checkmake /path/to/checkmake-0.3.2 \
  --unmake /path/to/unmake-0.0.27 \
  --mbake /path/to/mbake-1.4.6 \
  --project /path/to/git --project /path/to/redis \
  --output /tmp/rumk-performance.json
```

Use project names and revisions from `scripts/corpus-projects.json`. The runner
verifies each revision, rejects tracked modifications, and copies tracked files
into a temporary directory. It checks the root Makefile with each tool's static
mode, never invoking Make or upstream recipes. Input hashes before and after
every command detect mutations. Explicit configurations isolate the measured
profiles from personal configuration. Competitor versions and executable hashes
are recorded; dependencies of interpreter-based executables are not fully pinned.

Five measured samples follow two warmups, alternating tool order. Wall time
includes process startup, linting, output serialization, and the system `time`
wrapper. Each child gets an independent peak RSS measurement (`time -l` on macOS,
`time -f %M` on Linux), not a cumulative maximum from earlier children. Other
platforms report unavailable RSS as null. Separate `--version` samples describe
startup overhead but are not subtracted from checks. Timeouts terminate the
process group on POSIX systems. A timeout is retained with no median; remaining
repetitions for that tool/project are skipped. Other tools and projects continue.
Raw output and statuses remain available.

These commands perform different work: Rumk traverses resolved includes;
checkmake applies its checks; unmake checks POSIX syntax; mbake checks formatting.
Nonzero statuses can reflect valid dialect or formatting differences. Do not
turn their timings into an equal-feature ranking. Compare raw samples, workload
size, output, and machine load. Stable diagnostics are checked between repetitions;
checkmake's JSON order is normalized without dropping duplicates.

Reports stay outside version control. Re-run on the same machine with matching
input and executable hashes to assess variance before claiming an improvement.
