# Rumdl compatibility contract

Rumk is the Makefile sibling of Rumdl. Its command grammar, configuration philosophy,
diagnostics, output formats, and exit behavior intentionally follow Rumdl unless Makefile
semantics require a different design.

## Stable conventions

- `rumk check`, `rumk fmt`, `rumk init`, `rumk rule`, `rumk explain`, and `rumk config`
  carry the same meaning as their Rumdl counterparts.
- Exit code `0` means success, `1` means violations, and `2` means a tool or configuration
  error.
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
- Rumk's analysis context contains rules, targets, prerequisites, variables, includes, recipes,
  and source-preserving syntax instead of Markdown elements.
- Fixes must preserve Make behavior and every source byte outside their declared edit ranges.

Compatibility is a product requirement. Intentional differences must be documented here.
