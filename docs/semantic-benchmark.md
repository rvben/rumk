# Behavior-verified linter benchmark

`scripts/semantic-benchmark.py` measures ten pairs of authored broken and working
Makefiles. Labels describe observable GNU Make behavior before examining linter
output: overwritten recipes, discarded cycles, missing prerequisites, recursive
dry runs, lost directories, ignored failures, early expansion, invalid indentation,
and two same-named-file collisions. The manifest contains both source variants,
supporting files, and explicit status/output contracts. No upstream recipes run.

This is an independent behavioral oracle, **not an independently authored or
representative industry benchmark**. The sample is small and selected by Rumk's
maintainer. It includes known misses instead of removing cases Rumk cannot detect.

```sh
cargo build --release --locked
python3 scripts/semantic-benchmark.py --require-all \
  --checkmake /path/to/checkmake-0.3.2 \
  --unmake /path/to/unmake-0.0.27 \
  --mbake /path/to/mbake-1.4.6 \
  --output /tmp/semantic.json
```

The script installs nothing. Competitor version pins match the comparison
runner. `--make /path/to/gnu-make` selects the oracle, which must identify as GNU
Make. Each oracle, linter invocation, and fix runs in a fresh temporary directory
with a restricted environment and a timeout. Fixtures are trusted executable
test code: review source changes accordingly. Do not add destructive recipes,
network calls, uncontrolled Make functions, or recursive include loops.

All cases use Rumk defaults plus opt-in MK208, with an explicit configuration.
Checkmake has an empty required-target list; unmake uses static checks, never
dry-run validation; mbake uses `format --check`, never `--validate`. The GNU
fixtures are not a test of POSIX compliance. Make runs with `-rR`, so these probes
do not establish built-in implicit-rule coverage.

## Reading results

- **Detection** requires a diagnostic for the named defect. Rule identities or
  reviewed message patterns are explicit in the manifest. A nonzero exit code,
  unrelated convention warning, or generic “would reformat” does not count.
- **Control flags** count that same named warning on the working counterpart.
  These can expose overbroad rules, but a stricter naming or uniqueness policy is
  not automatically a claimed build error. Review the retained messages.
- **Unmatched output remains unscored.** This is not whole-output precision or
  recall. Add reviewed mappings when a tool identifies the defect differently;
  never silently assume its exit status supplies that evidence.
- **Safe fixes** use Rumk's default `check --fix` and mbake's formatter. Both run
  twice for idempotence and are checked against GNU behavior. Broken input may
  remain broken when a tool refuses or withholds a fix. Repair counts and working
  control preservation are separate. Checkmake and unmake have no fix invocation
  in this benchmark. GNU contracts cover the observed scenario, not every possible
  semantic difference.
- **Timing** includes process startup. One warm-up and three measured invocations
  run sequentially, alternating tool order. Raw samples and medians are retained.
  Work differs by tool; this is not an equal-feature speed ranking. Results pin
  executable hashes and version output, not Python interpreter dependency trees.

Repeat with `--baseline /tmp/semantic.json` and a different output path to require
the same identities, fixture hashes, observations, and fixed output hashes.
Timing samples are excluded; checkmake's diagnostic ordering is normalized
without dropping duplicate or changed findings. Keep the same `--runs` value.

For explicit project intent, run a separate measurement with
`--command-target verify`. This adds `[MK201] command-targets = ["verify"]` for
**every** Rumk case. Report that profile separately from the default-name score;
it is not automatic detection of arbitrary command intent.

Ordinary missing prerequisites remain outside the current rule set. A correct
general check must account for built-in and user implicit rules, generated files,
search paths, and dynamic graphs. The benchmark keeps this miss visible rather
than expanding the required-include rule beyond its contract.

Run the offline regression gate with `make check-semantic` (GNU Make and Python
3.9+). CI runs these tests along with the existing auditor tests. Generated HTML
and JSON reports stay local and must not be committed.
