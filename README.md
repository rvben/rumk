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
- Fixes recipe indentation and long static `.PHONY` declarations without changing what Make does,
  and offers style-aware missing `.PHONY` declarations and `$(MAKE)` spellings behind
  `--unsafe-fixes`
- Parses continued logical statements and nested `$(...)`/`${...}` expressions
- Models GNU Make assignment flavors, static patterns, target-specific variables, includes,
  conditionals, `define` blocks, custom recipe prefixes, and `.ONESHELL`
- Builds semantic indexes for variables, references, targets, dependencies, and includes
- Safely evaluates statically knowable variables and conditionals without running recipes,
  shell commands, or side-effecting Make functions
- Supports GNU substitution references plus common word, path, list, and lazy logical functions
- Resolves expanded include graphs and reports cross-file findings at their real source paths
- Preserves LF/CRLF line endings and final newlines during fixes
- Uses Rumdl-style `check`, `fmt`, `rule`, `config`, `init`, and `explain` commands, with `fmt`
  responsible for layout alone and `check` for what Make does with the file
- Discovers `.rumk.toml` upward through the project tree
- Respects `.gitignore` by default
- Supports rule selection, file globs, per-file ignores, severities, fix allowlists, and
  safe-versus-unsafe fix selection
- Emits text, flat JSON, and GitHub Actions annotations

## Installation

```bash
cargo install rumk --locked --version 0.0.7
```

Or install the native executable from PyPI with a Python tool manager:

```bash
uv tool install rumk==0.0.7
# or
pipx install rumk==0.0.7
```

Rumk is alpha-stage `0.0.x` software, so installation names the version explicitly. Release
archives and Python wheels cover Linux, macOS, and Windows. GitHub release assets include SHA-256
checksums.

### GitHub Actions

The repository is also a composite Action that installs a checksum-verified native Rumk release,
runs it, and leaves `rumk` on `PATH` for later steps:

```yaml
steps:
  - uses: actions/checkout@v4
  - uses: rvben/rumk@v0.0.7
    with:
      version: 0.0.7
      path: .
      report-type: annotations
```

Pin both the Action ref and `version` while Rumk is alpha. The moving `v0` Action tag is available
for users who prefer automatic `0.x` Action updates. Supported commands are `check`, `fmt-check`,
and `fmt`; `install-only: true` only installs Rumk. The Action also accepts `config`, `args`,
`fail-on-error`, and `output-file`, and exposes `rumk-version` and `rumk-path` outputs.

## Quick start

```bash
# Check Makefiles below the current directory
rumk check

# Check specific files or directories
rumk check Makefile build/

# Apply safe fixes, then fail only if violations remain
rumk check --fix

# Also apply the fixes that can change what Make does
rumk check --fix --unsafe-fixes

# Lay files out, leaving what Make does with them alone
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
unsafe-fixes = false
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

For configuration completion and validation in editors that support inline TOML schemas, put this
comment at the top of the file:

```toml
#:schema https://raw.githubusercontent.com/rvben/rumk/main/rumk.schema.json
```

The versioned schema is also included in Cargo packages and native release archives.

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

Path patterns are globs against the path relative to the project: `*` and `?` stay within one
path segment, `**` crosses segments, and a leading `**/` matches in the project root as well as
below it, so `**/vendor/**` covers `vendor/a.mk` and `sub/vendor/a.mk` alike.

### Fix safety

A fix is **safe** when Make reads the fixed file the way it read the original, or when the
original was not a file Make would read at all and the fix is the only reading that makes it one.
Correcting recipe indentation ([`MK001`](https://github.com/rvben/rumk/blob/main/docs/mk001.md))
is safe: the file did not build before, and a tab is what Make needs to see there.

A fix is **unsafe** when applying it can change what Make does. Declaring a target `.PHONY`
([`MK201`](https://github.com/rvben/rumk/blob/main/docs/mk201.md)) makes its recipe run where Make
would have called the target up to date; spelling a sub-make `$(MAKE)`
([`MK203`](https://github.com/rvben/rumk/blob/main/docs/mk203.md)) passes the parent's options and
jobserver down. Both are the right change to make, and both are a change a person should agree to.

`rumk check --fix` applies only safe fixes and reports how many it withheld. Ask for the rest with
`--unsafe-fixes`, refuse them explicitly with `--no-unsafe-fixes`, or set `unsafe-fixes` under
`[global]` to choose for a project. `rumk rule MK201` says which kind a rule offers. Under
`--diff` the withheld count goes to stderr, leaving stdout a patch other tools can read.

```bash
rumk check --fix .                  # safe fixes only
rumk check --fix --unsafe-fixes .   # every fix the enabled rules offer
```

Rumdl has no equivalent setting; this is a Rumk addition, modeled on Ruff's `--unsafe-fixes`.

### Formatting

`rumk fmt` is responsible for how a Makefile is laid out, and leaves what Make does with it to
`rumk check`. Only the layout rules run:
[`MK001`](https://github.com/rvben/rumk/blob/main/docs/mk001.md) indents recipes with the prefix
Make expects there, and [`MK101`](https://github.com/rvben/rumk/blob/main/docs/mk101.md) wraps a
long static `.PHONY` declaration. A path that cannot be read is still reported as
[`MK007`](https://github.com/rvben/rumk/blob/main/docs/mk007.md), because a file that was never
read was never formatted either. `rumk rule <ID>` says which commands run a rule.

So a formatting run neither reports nor rewrites a missing `.PHONY` declaration, a hardcoded
path, or a bare `make` in a recipe: those are lint findings, they belong to `rumk check`, and a
project that formats on save should not have them appear as formatting noise. This also makes
`rumk fmt --check` usable as a CI gate on its own, since it fails only when a file's layout is
not the one Rumk writes.

Every layout fix is safe, so `rumk fmt` has no unsafe fix to withhold and `--unsafe-fixes` makes
no difference to it.

### Exit codes

- `0`: success, or all selected violations were fixed
- `1`: lint violations, `fmt --check` found required changes, or a path could not be read
- `2`: configuration error, a path that does not exist, a failed write, or another tool error

`rumk check` fails on any diagnostic by default. Use `--fail-on warning`, `--fail-on error`, or
`--fail-on never` to change that policy. `rumk fmt` exits successfully after formatting even if
a layout diagnostic it has no fix for remains. A Makefile that cannot be read, or a directory that cannot be
searched for Makefiles, is reported as
[`MK007`](https://github.com/rvben/rumk/blob/main/docs/mk007.md) and fails every command regardless
of `--fail-on`; the other files are still checked.

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
      "applicability": "safe",
      "range": { "start": 7, "end": 11 },
      "replacement": "\t"
    }
  }
]
```

`fixable` says whether this run would apply the fix; `fix.applicability` says why. A diagnostic
with `"fixable": false` and an `"unsafe"` fix is one Rumk withheld, and an editor can still offer
it as an explicit action.

The legacy `--format` spelling remains an alias for `--output-format`.

## Rules

Rules marked **default** run without configuration.
Each rule page documents its behavior, configuration, fixes, edge cases, and
the GNU Make, POSIX, or Rumk convention on which it is based.

### Syntax

- [`MK001`](https://github.com/rvben/rumk/blob/main/docs/mk001.md) - Recipes must use tab
  indentation (**default**, safe fix, run by `fmt`)
- [`MK002`](https://github.com/rvben/rumk/blob/main/docs/mk002.md) - Invalid variable syntax
  (**default**)
- [`MK003`](https://github.com/rvben/rumk/blob/main/docs/mk003.md) - Malformed conditional
  structure (**default**)
- [`MK004`](https://github.com/rvben/rumk/blob/main/docs/mk004.md) - Targets must not mix
  single- and double-colon declarations (**default**)
- [`MK005`](https://github.com/rvben/rumk/blob/main/docs/mk005.md) - GNU Make special targets
  must stand alone (**default**)
- [`MK006`](https://github.com/rvben/rumk/blob/main/docs/mk006.md) - Statement is not valid GNU
  Make syntax: missing separators, recipes before the first target, unterminated references,
  empty variable names, and unbalanced `define` blocks (**default**)
- [`MK007`](https://github.com/rvben/rumk/blob/main/docs/mk007.md) - Path could not be read: an
  unreadable file or directory is an error and a file that is not valid UTF-8 is linted with the
  invalid bytes replaced and fixes disabled, without stopping the run (**default**)

### Style

- [`MK101`](https://github.com/rvben/rumk/blob/main/docs/mk101.md) - Declarative line exceeds
  the configured maximum length; comments and recipes are ignored by default, and static
  `.PHONY` declarations can be wrapped safely (**default**, partially fixable, safe fix, run by
  `fmt`)
- [`MK102`](https://github.com/rvben/rumk/blob/main/docs/mk102.md) - Variable naming convention
- [`MK103`](https://github.com/rvben/rumk/blob/main/docs/mk103.md) - Target naming convention

### Best practices

- [`MK201`](https://github.com/rvben/rumk/blob/main/docs/mk201.md) - Conventional
  non-file targets should be `.PHONY`;
  fixes consolidate canonical groups, preserve per-section style, and wrap long
  declarations (**default**, unsafe fix)
- [`MK202`](https://github.com/rvben/rumk/blob/main/docs/mk202.md) - Avoid hardcoded absolute
  paths (opt-in)
- [`MK203`](https://github.com/rvben/rumk/blob/main/docs/mk203.md) - Recursive Make invocations
  should use `$(MAKE)` (**default**, unsafe fix)
- [`MK204`](https://github.com/rvben/rumk/blob/main/docs/mk204.md) - Concrete targets should not
  declare multiple single-colon recipes (**default**)
- [`MK205`](https://github.com/rvben/rumk/blob/main/docs/mk205.md) - Explicit target dependencies
  must not form cycles (**default**)
- [`MK206`](https://github.com/rvben/rumk/blob/main/docs/mk206.md) - Required static includes must
  resolve (**default**)
- [`MK207`](https://github.com/rvben/rumk/blob/main/docs/mk207.md) - Static Makefile includes must
  not form cycles (**default**)
- [`MK208`](https://github.com/rvben/rumk/blob/main/docs/mk208.md) - Static graph-level variable
  references must resolve, and by the time Make reads them (opt-in)
- [`MK209`](https://github.com/rvben/rumk/blob/main/docs/mk209.md) - Targets must be reachable
  from explicitly configured entries (opt-in)
- [`MK210`](https://github.com/rvben/rumk/blob/main/docs/mk210.md) - Explain include expressions
  blocked by safe evaluation (opt-in)
- [`MK211`](https://github.com/rvben/rumk/blob/main/docs/mk211.md) - Variable references longer than
  one character need parentheses (**default**)
- [`MK212`](https://github.com/rvben/rumk/blob/main/docs/mk212.md) - A recipe line must not end with
  the `cd` the lines after it need (**default**)
- [`MK213`](https://github.com/rvben/rumk/blob/main/docs/mk213.md) - `$(shell ...)` belongs in a
  variable Make expands once (opt-in)

## Development

```bash
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build --release
make check-gnu-fixtures
```

Run `make benchmark` (Python 3.9+) for deterministic generated workloads: a
1,002-file include graph, a 20,000-target Makefile, and 10,000 indentation fixes.
The runner uses temporary files, restores fix inputs before each run, and reports
medians after two warmups and seven measured runs. Timings include process startup,
file access, linting, JSON output, and writes for the fix workload; generation and
output comparison are excluded. These are warm-cache measurements.

Compare release binaries and save raw timings outside the repository:

```bash
python3 scripts/benchmark.py --baseline /tmp/rumk-before \
  --binary target/release/rumk --output /tmp/rumk-benchmarks.json
```

The runner alternates binary order and requires identical diagnostics and fixed
contents. Use `--scale 1 --runs 1 --warmups 0` for a quick smoke check. Keep compiler
settings and machine load comparable; use raw samples to assess noise rather than
enforcing a universal timing threshold. The include graph caps at 1,002 files to
stay below Rumk's default project limit.

The product-level compatibility contract is documented in
[`docs/rumdl-compatibility.md`](docs/rumdl-compatibility.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE).
