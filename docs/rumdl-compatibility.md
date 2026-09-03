# Rumdl compatibility contract

Rumk is the Makefile sibling of Rumdl. Its command grammar, configuration philosophy,
diagnostics, output formats, and exit behavior intentionally follow Rumdl unless Makefile
semantics require a different design.

## Stable conventions

- `rumk check`, `rumk fmt`, `rumk init`, `rumk rule`, `rumk explain`, and `rumk config`
  carry the same meaning as their Rumdl counterparts.
- Exit code `0` means success, `1` means violations, and `2` means a tool or configuration
  error. A Makefile that cannot be read is a diagnostic (`MK007`) with exit code `1`, not a
  tool error, so one bad file does not abort a run.
- `check --fix` exits based on violations remaining after fixes. `fmt` exits successfully after
  formatting; `fmt --check` fails when formatting changes are required.
- Text diagnostics use `path:line:column: [RULE] message [*]`.
- JSON output is a flat array of diagnostics containing a `file` field.
- Public configuration uses `[global]`, top-level rule sections such as `[MK101]`, kebab-case
  keys, upward discovery, and explicit effective-configuration inspection.
- CLI rule and file selectors override configuration using Rumdl's enable/disable and
  include/exclude vocabulary.

## Make-specific equivalents

- Rumdl flavors correspond to Rumk dialects: `gnu`, `posix`, and `bsd`.
- Inline controls use Make comments such as `# rumk-disable MK202`.
- Project diagnostics retain the same flat output shape while setting `file` to the included
  Makefile that owns the finding.
- Project settings live under `[global]` as `include-paths`, `predefined-variables`, and
  `entry-targets`, following Rumdl's kebab-case configuration vocabulary.
- Static project analysis safely expands known variables, conditionals, includes, targets, and
  prerequisites. Unknown or side-effecting expressions remain unresolved and are never executed.
- Opt-in rule `MK210` explains unresolved include expressions and traces them back to contributing
  variable definitions without changing Rumdl-style diagnostic output.
- Rumk's analysis context contains rules, targets, prerequisites, variables, includes, recipes,
  and source-preserving syntax instead of Markdown elements.
- Every fix preserves every source byte outside its declared edit ranges. A fix that also leaves
  Make's reading of the file unchanged is safe; one that can change what Make does is unsafe, and
  a run applies it only when asked.

## Intentional differences

- **Fix safety.** Rumdl applies every fix its rules offer. Rumk splits them into safe and unsafe,
  applies only safe fixes by default, and gates the rest behind `check --unsafe-fixes`,
  `--no-unsafe-fixes`, and `[global] unsafe-fixes`, following Ruff's model. Makefiles make this
  necessary: declaring a target `.PHONY` or spelling a sub-make `$(MAKE)` is the right change and
  still changes what a build does, which is not a change a linter should make unasked.

Compatibility is a product requirement. Intentional differences must be documented here.
