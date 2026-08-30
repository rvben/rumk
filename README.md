# rumk

A fast Makefile linter written in Rust, built as the Makefile sibling of
[Rumdl](https://github.com/rvben/rumdl).

Rumk is currently alpha software. Its CLI, configuration model, diagnostics, fixing behavior,
and exit codes intentionally follow Rumdl so existing Rumdl users can reuse their workflow.

## Features

- Lints individual Makefiles or entire directory trees
- Safely fixes tab-indented recipe violations
- Parses continued logical statements and nested `$(...)`/`${...}` expressions
- Models GNU Make assignment flavors, static patterns, target-specific variables, includes,
  conditionals, `define` blocks, custom recipe prefixes, and `.ONESHELL`
- Builds semantic indexes for variables, references, targets, dependencies, and includes
- Safely evaluates statically knowable variables and conditionals without running recipes,
  shell commands, or side-effecting Make functions
- Resolves expanded include graphs and reports cross-file findings at their real source paths
- Preserves LF/CRLF line endings and final newlines during fixes
- Uses Rumdl-style `check`, `fmt`, `rule`, `config`, `init`, and `explain` commands
- Discovers `.rumk.toml` upward through the project tree
- Respects `.gitignore` by default
- Supports rule selection, file globs, per-file ignores, severities, and fix allowlists
- Emits text, flat JSON, and GitHub Actions annotations

## Installation

```bash
cargo install rumk
```

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

[MK102]
enabled = true
style = "upper-case"

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
evaluation and names expected by opt-in rule `MK208`. `entry-targets` overrides the roots for
opt-in reachability rule `MK209`; without explicit entries, Rumk follows GNU Make's inferred
default goal when it can determine that goal safely.

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

### Syntax

- `MK001` — Recipes must use tab indentation (**default**, fixable)
- `MK002` — Invalid variable syntax (**default**)
- `MK003` — Malformed conditional structure (**default**)
- `MK004` — Targets must not mix single- and double-colon declarations (**default**)
- `MK005` — GNU Make special targets must stand alone (**default**)

### Style

- `MK101` — Line exceeds the configured maximum length (**default**)
- `MK102` — Variable naming convention
- `MK103` — Target naming convention

### Best practices

- `MK201` — Common non-file targets should be `.PHONY` (**default**)
- `MK202` — Avoid hardcoded absolute paths
- `MK203` — Recursive Make invocations should use `$(MAKE)` (**default**)
- `MK204` — Concrete targets should not declare multiple single-colon recipes (**default**)
- `MK205` — Explicit target dependencies must not form cycles (**default**)
- `MK206` — Required static includes must resolve (**default**)
- `MK207` — Static Makefile includes must not form cycles (**default**)
- `MK208` — Static variable references must resolve (opt-in)
- `MK209` — Targets must be reachable from configured entries or the inferred default goal (opt-in)

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
