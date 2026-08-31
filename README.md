<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rvben/rumk/main/assets/rumk-logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/rvben/rumk/main/assets/rumk-logo-light.svg">
    <img src="https://raw.githubusercontent.com/rvben/rumk/main/assets/rumk-logo-light.svg" width="520" alt="rumk">
  </picture>
</p>

<p align="center"><strong>Makefiles, built right.</strong></p>

<p align="center">
  Fast, trustworthy linting and formatting for Makefiles.
</p>

<p align="center">
  <a href="https://github.com/rvben/rumk/actions/workflows/ci.yml"><img src="https://github.com/rvben/rumk/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/rumk"><img src="https://img.shields.io/crates/v/rumk.svg" alt="crates.io"></a>
  <a href="https://pypi.org/project/rumk/"><img src="https://img.shields.io/pypi/v/rumk.svg" alt="PyPI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/crates/l/rumk.svg" alt="License"></a>
</p>

> [!WARNING]
> **Alpha software under active development.** Rumk is useful today, but its rules, CLI,
> configuration, diagnostics, and autofixes may change between `0.0.x` releases. Pin the version
> in automation and review autofix diffs before committing them.

Rumk is the Makefile sibling of [Rumdl](https://github.com/rvben/rumdl).

Its CLI, configuration model, diagnostics, fixing behavior, and exit codes intentionally follow
Rumdl so existing users can reuse their workflow.

## Features

- Lints individual Makefiles or entire directory trees
- Safely fixes recipe indentation, long static and style-aware missing `.PHONY` declarations,
  and recursive Make invocations
- Parses continued logical statements and nested `$(...)`/`${...}` expressions
- Models GNU Make assignment flavors, static patterns, target-specific variables, includes,
  conditionals, `define` blocks, custom recipe prefixes, and `.ONESHELL`
- Builds semantic indexes for variables, references, targets, dependencies, and includes
- Safely evaluates statically knowable variables and conditionals without running recipes,
  shell commands, or side-effecting Make functions
- Supports GNU substitution references plus common word, path, list, and lazy logical functions
- Resolves expanded include graphs and reports cross-file findings at their real source paths
- Preserves LF/CRLF line endings and final newlines during fixes
- Uses Rumdl-style `check`, `fmt`, `rule`, `config`, `init`, and `explain` commands
- Discovers `.rumk.toml` upward through the project tree
- Respects `.gitignore` by default
- Supports rule selection, file globs, per-file ignores, severities, and fix allowlists
- Emits text, flat JSON, and GitHub Actions annotations

## Installation

```bash
cargo install rumk --locked --version 0.0.6
```

Or install the native executable from PyPI with a Python tool manager:

```bash
uv tool install rumk==0.0.6
# or
pipx install rumk==0.0.6
```

Rumk is alpha-stage `0.0.x` software, so installation names the version explicitly. Release
archives and Python wheels cover Linux, macOS, and Windows. GitHub release assets include SHA-256
checksums.

## Quick start

```bash
# Check Makefiles below the current directory
rumk check

# Check specific files or directories
rumk check Makefile build/

# Apply safe fixes, then fail only if violations remain
rumk check --fix

# Format files with formatter-style exit behavior
rumk fmt

# Preview formatting changes
rumk fmt --diff

# Fail when formatting changes are required
rumk fmt --check

# Inspect rules and effective configuration
rumk rule
rumk rule MK101
rumk config
rumk config get MK101.line-length
rumk config file
```

Run `rumk --help` or `rumk <command> --help` for all options.

## Configuration

Create a `.rumk.toml` file manually or run `rumk init`:

```toml
[global]
dialect = "gnu"
respect-gitignore = true
exclude = ["vendor/**", "generated/**"]
disable = ["MK101"]
fixable = ["MK001"]
include-paths = ["mk"]
predefined-variables = { FROM_CLI = "yes" }
entry-targets = ["all"]

[MK101]
enabled = true
severity = "warning"
line-length = 100
ignore-comments = true
ignore-recipes = true

[MK102]
enabled = true
style = "upper-case"

[MK201]
placement = "auto"

[per-file-ignores]
"vendor/**/*.mk" = ["MK202"]
```

Configuration discovery checks `.rumk.toml`, `rumk.toml`, and `.config/rumk.toml` while walking
upward, stopping at a Git project boundary. Use `--config <PATH>` for an explicit file or
`--no-config`/`--isolated` for built-in defaults.

Configurations can inherit another file with `extends = "../.rumk.toml"`; nested tables are
merged, child values win, relative paths resolve from the extending file, and cycles are rejected.

Rules can be suppressed in Make comments without changing project configuration:

```makefile
# rumk-disable MK202
INSTALL_PREFIX := /usr/local
# rumk-enable MK202

# rumk-disable-next-line MK201
clean:
	rm -rf build
```

`rumk-disable`, `rumk-enable`, `rumk-disable-line`, and `rumk-disable-next-line` are supported.
Recipe shell comments are not interpreted as Rumk directives.

The original Rumk `[rules]` and `[ignore]` configuration sections remain accepted for migration.

`include-paths` models GNU Make's `-I` search directories and resolves relative entries from the
configuration directory. `predefined-variables` supplies command-line-style values to safe
evaluation and names expected by opt-in rule `MK208`. That rule intentionally ignores references
inside recipes and deferred macro bodies, where command-line parameters and shell values are
normal. `entry-targets` supplies the roots for opt-in reachability rule `MK209`; the rule stays
silent without explicit roots because any Make target may be invoked directly from the command
line.

### Rule and file selection

Rumk follows Rumdl's selection vocabulary:

```bash
rumk check --enable MK001,MK002 .
rumk check --disable MK101 .
rumk check --extend-enable MK202 .
rumk check --exclude "vendor/**,generated/**" .
rumk check --include "src/**" .
rumk check --respect-gitignore=false .
rumk fmt --fixable MK001 .
```

### Exit codes

- `0`: success, or all selected violations were fixed
- `1`: lint violations, or `fmt --check` found required changes
- `2`: configuration, file access, or other tool error

`rumk check` fails on any diagnostic by default. Use `--fail-on warning`, `--fail-on error`, or
`--fail-on never` to change that policy. `rumk fmt` exits successfully after formatting even if
non-fixable lint diagnostics remain.

## Output

Text diagnostics follow Rumdl's familiar form:

```text
Makefile:2:1: [MK001] Recipe must be indented with tab, not spaces [*]
```

JSON output is a flat array collected across all files:

```bash
rumk check --output-format json .
```

```json
[
  {
    "file": "Makefile",
    "line": 2,
    "column": 1,
    "end_line": 2,
    "end_column": 1,
    "rule": "MK001",
    "message": "Recipe must be indented with tab, not spaces",
    "severity": "error",
    "fixable": true,
    "fix": {
      "range": { "start": 7, "end": 11 },
      "replacement": "\t"
    }
  }
]
```

The legacy `--format` spelling remains an alias for `--output-format`.

## Rules

Rules marked **default** run without configuration.
Each rule page documents its behavior, configuration, fixes, edge cases, and
the GNU Make, POSIX, or Rumk convention on which it is based.

### Syntax

- [`MK001`](https://github.com/rvben/rumk/blob/main/docs/mk001.md) — Recipes must use tab
  indentation (**default**, fixable)
- [`MK002`](https://github.com/rvben/rumk/blob/main/docs/mk002.md) — Invalid variable syntax
  (**default**)
- [`MK003`](https://github.com/rvben/rumk/blob/main/docs/mk003.md) — Malformed conditional
  structure (**default**)
- [`MK004`](https://github.com/rvben/rumk/blob/main/docs/mk004.md) — Targets must not mix
  single- and double-colon declarations (**default**)
- [`MK005`](https://github.com/rvben/rumk/blob/main/docs/mk005.md) — GNU Make special targets
  must stand alone (**default**)

### Style

- [`MK101`](https://github.com/rvben/rumk/blob/main/docs/mk101.md) — Declarative line exceeds
  the configured maximum length; comments and recipes are ignored by default, and static
  `.PHONY` declarations can be wrapped safely (**default**, partially fixable)
- [`MK102`](https://github.com/rvben/rumk/blob/main/docs/mk102.md) — Variable naming convention
- [`MK103`](https://github.com/rvben/rumk/blob/main/docs/mk103.md) — Target naming convention

### Best practices

- [`MK201`](https://github.com/rvben/rumk/blob/main/docs/mk201.md) — Conventional
  non-file targets should be `.PHONY`;
  fixes consolidate canonical groups, preserve per-section style, and wrap long
  declarations (**default**, fixable)
- [`MK202`](https://github.com/rvben/rumk/blob/main/docs/mk202.md) — Avoid hardcoded absolute
  paths (opt-in)
- [`MK203`](https://github.com/rvben/rumk/blob/main/docs/mk203.md) — Recursive Make invocations
  should use `$(MAKE)` (**default**, fixable)
- [`MK204`](https://github.com/rvben/rumk/blob/main/docs/mk204.md) — Concrete targets should not
  declare multiple single-colon recipes (**default**)
- [`MK205`](https://github.com/rvben/rumk/blob/main/docs/mk205.md) — Explicit target dependencies
  must not form cycles (**default**)
- [`MK206`](https://github.com/rvben/rumk/blob/main/docs/mk206.md) — Required static includes must
  resolve (**default**)
- [`MK207`](https://github.com/rvben/rumk/blob/main/docs/mk207.md) — Static Makefile includes must
  not form cycles (**default**)
- [`MK208`](https://github.com/rvben/rumk/blob/main/docs/mk208.md) — Static graph-level variable
  references must resolve (opt-in)
- [`MK209`](https://github.com/rvben/rumk/blob/main/docs/mk209.md) — Targets must be reachable
  from explicitly configured entries (opt-in)
- [`MK210`](https://github.com/rvben/rumk/blob/main/docs/mk210.md) — Explain include expressions
  blocked by safe evaluation (opt-in)

## Development

```bash
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build --release
make check-gnu-fixtures
```

The product-level compatibility contract is documented in
[`docs/rumdl-compatibility.md`](docs/rumdl-compatibility.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE).
