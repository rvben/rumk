# Measuring MK216 coverage

Build the read-only contributor tool with:

```sh
cargo build --example prerequisite-coverage
target/debug/examples/prerequisite-coverage Makefile
```

This JSON view calls the same analysis as MK216, regardless of rule enablement.
It lists every independent root blocker and gives each visible dependency one
outcome, with its declaring file, line, target, and order-only status. It does
not execute recipes or shell functions. It uses default project options, so it
is an audit of that configuration, not all possible build environments.

To audit pinned local checkouts, run `scripts/audit-prerequisite-coverage.py`
with `--manifest scripts/corpus-projects.json`, one `--project /path/to/checkout`
for every manifest project, and `--output reports/coverage.json`. The script
requires clean tracked files at the pinned revisions, compares two complete
coverage runs, and verifies unchanged file inventories. No upstream Makefile
is executed. Generated reports remain local and must not be committed.

Interpretation:

- An eligible root has no root blockers. It may still contain unsupported or
  uncertain dependencies, or no visible dependencies at all.
- `roots_by_blocker` counts affected roots, while `blocker_occurrences` counts
  occurrences inside their include graphs. Blockers overlap; do not sum them
  into a count of excluded roots. Included files can appear under multiple roots.
- The denominator is the evaluated semantic edge inventory. Unresolved graph
  expressions may conceal dependencies that cannot be counted. Standalone
  fragments are explicitly excluded as roots; their included uses still count.
- `root_excluded`, `special_target`, `pattern_declaration`, and unsupported names
  are skipped work. Declared targets and visible files satisfy the static check;
  they do not establish that a recipe succeeds. `file_or_io_uncertainty` also
  includes filesystem errors other than absence.
- Possible built-in inputs and pattern producers are conservative uncertainty,
  not proof of buildability. `missing` corresponds to an MK216 finding; duplicate
  findings at the same declaration are suppressed.

Compare the same revisions and root inventory before and after a rule change.
Review new findings independently and keep GNU-verified reductions of upstream
constructs in the behavior tests. Coverage is separate from diagnostic accuracy
and from success at actually building a project.
