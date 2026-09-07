# GNU behavior pairs

`manifest.json` is trusted, executable test input for
`scripts/semantic-benchmark.py`. Each case holds authored broken and working
Makefiles, supporting files, a GNU Make invocation, and independent output
contracts. Only these disposable fixtures execute; upstream checkouts do not.

Ground truth is the observable behavior, not Rumk's diagnostics. The
`expected_rumk_detection` field locks current coverage, including known misses;
it is not used to derive benchmark scores. A null `rule` marks a defect outside
the current Rumk rule set. Tool matchers identify the named defect; all unmatched
output remains available and unscored. Update a matcher only after reviewing
the raw diagnostic and both variants.

The duplicate-recipe control intentionally repeats a target with just one
recipe, which GNU Make accepts. The custom command uses a real same-named file;
it needs declared project intent for Rumk to recommend `.PHONY`. The simple prerequisite typo is covered by opt-in MK216. Added pairs retain
misses behind user patterns, dynamic expressions, and selective vpath. The
built-in compiler pair runs GNU Make with built-ins enabled under `-n`.

Keep recipes harmless and deterministic. Review fixture changes like executable
code. See [methodology](../../../docs/semantic-benchmark.md) for scoring, tool
profiles, fix validation, and reproduction commands.

Pattern pairs run GNU Make under `-n -rR`. Four prove that viable producers do
not prevent reporting an unrelated missing input. The fifth proves that a
terminal rule cannot chain a missing intermediate and records MK216 detecting it.
Additional pairs check direct pattern inputs, order-only inputs, failed competing
producers, and directory restoration for slashless terminal patterns.
