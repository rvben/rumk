# Behavior-verified linter benchmark

`scripts/semantic-benchmark.py` measures thirty-five pairs of authored broken and
working Makefiles. Labels describe observable GNU Make behavior before examining linter
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

All cases use Rumk defaults plus opt-in MK208, MK216, and MK217, with an explicit
configuration.
Checkmake has an empty required-target list; unmake uses static checks, never
dry-run validation; mbake uses `format --check`, never `--validate`. The GNU
fixtures are not a test of POSIX compliance. Make runs with `-rR` except for the
explicitly labeled built-in compiler pair,
which enables built-ins and uses `-n` to inspect commands without compiling.
This exercises one built-in family, not the complete implicit-rule catalogue.

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

MK216 now covers simple missing prerequisites, generated declarations, static
VPATH, and absent built-in compiler inputs. It remains opt-in. User-pattern,
dynamic-expression, and selective-vpath defects remain deliberately uncovered
in the manifest; see [its boundaries](mk216.md). The additional pairs change
the denominator, so compare the original ten separately when assessing progress.

Run the offline regression gate with `make check-semantic` (GNU Make and Python
3.9+). CI runs these tests along with the existing auditor tests. Generated HTML
and JSON reports stay local and must not be committed.

The five pattern pairs exercise unrelated missing inputs alongside viable direct,
intermediate, terminal, and competing producers, plus a terminal-chain defect
that MK216 now diagnoses. Five further pairs cover missing single-step sources,
order-only sources, competing failed producers, and slashless terminal rules with
pattern and literal inputs. A separate GNU regression verifies that a broad
pattern can generate a built-in compiler input even when its direct match fails.
Compare shared cases separately when comparing reports with different denominators.

Three corpus-derived pairs test missing sources alongside optional compiler
settings, recipe-only wildcard expansion, and resolved `.PHONY` declarations.
The [coverage audit](prerequisite-coverage.md) measures how many real roots and
visible dependencies are eligible separately from the authored detection score.

Use `--external-variable TESTS` for a separate name-only MK208 profile applied
uniformly to every case. It does not pass values to GNU Make or alter the oracle.
The optional-setting typo pair intentionally flags its working control in the
unconfigured profile; `expected_rumk_control_flag` records that limitation and
is not used to derive scores. The configured profile must still catch `TSET`.
A reduced Git spelling error provides a real-corpus defect control.

Three phony-rebuild pairs distinguish normal prerequisites from order-only setup,
including expanded names and included declarations. Existing output files supply
the GNU Make oracle; changing those edges is deliberately not an automatic fix.

The summary includes per-rule broken/control counts, paired recall (detected
broken cases divided by all cases), and paired precision (detections divided by
detections plus named control flags). These are **authored-pair metrics**, not
population estimates or whole-output precision. `known_rumk_coverage_gaps` lists
expected misses explicitly; a high paired score is not proof of complete coverage.
