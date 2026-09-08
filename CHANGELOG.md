# Changelog

All notable changes to Rumk are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Native `rumk server` provides stdio LSP diagnostics, versioned quick fixes,
  fix-all actions, formatting, and symbols with UTF-16 incremental buffers,
  unsaved include overlays, cancellation, and configuration invalidation.
- `MK301` checks explicit POSIX.1-2017 and POSIX.1-2024 source-syntax profiles.
- Opt-in `MK106` safely normalizes static rule-header spacing.
- The semantic benchmark now includes 35 defect/control pairs and per-rule paired
  metrics. A pinned real-project comparison runner records startup, wall time,
  independent process peak RSS, stable output, and input integrity.
- Opt-in `MK217` detects phony prerequisites that force non-phony targets to rebuild,
  with include-aware locations and exclusions for uncertain graphs.
- Opt-in `MK218` safely removes repeated literal recipe command prefixes while
  preserving flag order and excluding `.ONESHELL` projects.
- SARIF reports include complete applicable edit sets, validated against checked
  buffers and governed by existing fix safety and allowlist settings.
- GNU Make behavior tests and shared comparison fixtures for rebuild dependencies
  and recipe-prefix fixes; SARIF consumer tests cover Unicode, BOM, CRLF, and multiple edits.

### Changed

- `global.dialect` accepts `gnu`, `posix2017` (alias `posix`), and `posix2024`.
  Unknown values, including the previously ineffective `bsd`, now fail validation.
  POSIX profiles enable MK301; the 2017 profile disables GNU `.PHONY` advice by
  default. Explicit rule configuration still overrides these defaults.

### Fixed

- Unsaved buffers resolve their existing parent directory before analysis, so
  Windows path spelling and symlink aliases cannot detach SARIF/JSON edits from
  the buffer that owns them.

## [0.0.8] - 2026-09-08

### Added

- `MK007` reports unreadable paths and invalid UTF-8 without stopping checks of other files.
- `MK211` catches unparenthesized multi-character variable references, `MK212` detects recipe
  lines that lose a directory change, and opt-in `MK213` detects repeated shell expansion.
- `MK214` reports global error suppression. Opt-in `MK104`, `MK105`, and `MK215` provide
  logical recipe-length limits, safe assignment spacing, and required project-target policies.
- Opt-in `MK216` reports missing static prerequisites when no visible file, declared target,
  built-in possibility, or supported pattern producer can satisfy them. Bounded pattern analysis
  handles terminal rules and order-only inputs conservatively; uncertain graphs remain excluded.
- `MK201.command-targets` supports explicit project command names. `MK208.external-variables`
  declares exact external names without inventing values or hiding read-before-definition errors.
- `rumk fmt` applies layout-only fixes. Stdin support checks and formats unsaved Makefiles, and
  SARIF output supports code-scanning integrations.
- Pre-commit hooks, a GNU Make behavioral benchmark with 32 defect/control pairs, pinned audits
  of 69 upstream Makefiles, prerequisite-coverage tooling, and lint/fix fuzz targets.
- `MK006` reports statements GNU Make refuses to read: missing separators (with the same hints
  GNU Make prints for eight-space indentation and `ifeq(` without whitespace), recipes before
  the first target, unterminated variable and function references, empty variable names, and
  unbalanced `define` blocks. Unterminated references in recursively expanded variables are
  reported only when GNU Make expands the variable while it holds that value.

### Changed

- Fixes distinguish safe from unsafe changes. `check --fix` applies safe fixes by default;
  behavior-changing fixes such as `.PHONY` insertion and recursive Make replacement require
  `--unsafe-fixes`. JSON diagnostics expose applicability and complete edit replacements.
- Project parsing and analysis reuse cached work, avoid redundant scans and allocations, and
  check independent inputs in parallel.
- `MK208` tracks when include and assignment references are read, with conservative handling
  of unresolved or potentially rebuilt includes and independently checked fragments.
- `rumk::parser::parse` no longer fails on a Makefile GNU Make would reject. It returns the
  parsed file with the problems collected in `Makefile::syntax_errors`, so every other rule
  still runs on it.
- An included file GNU Make would reject is loaded and linted like any other file. Its syntax
  errors are reported by `MK006` when that file is checked, no longer by `MK206` at the
  include site.
- Statements are read the way GNU Make 4.3 and later read them: an assignment is looked for
  before any keyword, so `ifdef = 1` defines a variable named `ifdef`, and a variable name ends
  at the first whitespace outside a reference, so `two words = 1` is a missing separator and
  `all: a b = 1` lists prerequisites instead of defining a target-specific variable. `MK002`
  no longer describes such names as valid.

### Fixed

- Windows prerequisite analysis compares host-generated paths using Make's slash spelling,
  preserving possible VPATH and pattern producers instead of reporting false missing inputs.
- GNU Make compatibility for inline recipes, assignment modifiers and flavors, target-specific
  bindings, escaped paths, byte order marks, nested references, and expansion limits.
- Include-producer analysis respects pattern chains, terminal rules, static pattern declarations,
  match-anything rules, and path normalization. Recipe analysis respects active `.ONESHELL` state.
- Safe fixes preserve symlink destinations and active recipe prefixes, combine interacting
  `.PHONY` edits, and report only edits actually applied.
- `MK216` accepts resolved `.PHONY` lists and excludes unknown assignments used only by recipes
  from its graph-level uncertainty checks, expanding audited coverage without new corpus warnings.
- A `;` inside a trailing comment on a rule line is no longer parsed as an inline recipe.
- Recipe-prefixed lines that GNU Make reads as ordinary statements when no rule is open, such as
  an indented assignment, are parsed as those statements instead of being dropped.

## [0.0.7] - 2026-08-31

### Added

- `MK101` safely wraps long static `.PHONY` declarations at the configured line length while
  preserving target order, inline comments, line endings, and fix idempotence.
- A first-party composite GitHub Action installs checksum-verified native releases, supports
  checks and formatting, emits annotations, exposes the installed binary, and runs on Linux,
  macOS, and Windows.
- `rumk.schema.json` provides editor completion and validation for canonical and legacy
  configuration, including every rule-specific option.

### Changed

- Scheduled dependency maintenance now uses upd's pinned reusable workflow for Rust and GitHub
  Actions updates in one validated rolling pull request, replacing Dependabot and the custom
  updater job.
- Dependency proposals must pass Rumk's Rust 1.82 compatibility gate before publication, and the
  Clap requirement remains on its compatible 4.5 minor line until the MSRV is raised.

## [0.0.6] - 2026-08-31

### Changed

- Release jobs use Node.js 24 artifact actions and avoid caching transient Cargo package trees,
  eliminating deprecation and missing-directory annotations.
- Release reruns preserve the assets of an existing public GitHub release while safely skipping
  versions already present on Cargo and PyPI.
- Release automation now publishes native wheels and the source distribution to PyPI through
  short-lived Trusted Publishing credentials.
- The README now gives alpha-stage stability and autofix guidance before the project overview.
- GitHub artifact attestations run only where the repository visibility supports them.
- GitHub Actions now uses the Node.js 24-based checkout action with an immutable revision pin.

### Fixed

- Windows diagnostics compare canonical source, configuration, and working-directory paths,
  keeping project-relative JSON paths and per-file ignores consistent across platforms.

## [0.0.5] - 2026-08-30

### Added

- `MK201.placement` controls where autofixes add missing `.PHONY` declarations:
  `auto` preserves the established file style, `top` groups names in the
  earliest declaration, and `adjacent` places declarations beside their rules.
- Complete reference pages for every rule, including examples, configuration,
  autofix behavior, special cases, and the source of each convention.
- `rumk rule <RULE>` now shows category, default state, fixability, analysis
  scope, rule-option defaults, and a documentation link.
- Native PyPI wheels for supported Linux, macOS, and Windows platforms.

### Fixed

- `MK002` accepts GNU Make variable names containing valid punctuation or
  internal whitespace and conservatively accepts computed variable names.
- Required includes no longer treat trailing Make comments as filenames.
- Space-indented conditional directives after a rule are no longer mistaken
  for malformed recipes, preventing a project-analysis panic.

## [0.0.4] - 2026-08-30

### Changed

- `MK201` now emits one coordinated fix per source file, extends existing canonical `.PHONY`
  groups, preserves established per-section declarations, wraps long groups safely, retains inline
  comments and line endings, and avoids conditional declarations.

## [0.0.3] - 2026-08-30

### Changed

- `MK101` now ignores full-line comments and recipe bodies by default, with configuration switches
  for projects that want strict line-length enforcement in those regions.
- `MK208` now focuses on graph-level Make references and excludes recipe and deferred-macro
  parameters, avoiding false positives for normal command-line inputs.
- `MK209` now requires explicit `entry-targets`, because every Make target can otherwise be a
  legitimate command-line entry point.

## [0.0.2] - 2026-08-30

### Added

- Rumdl-shaped `check`, `fmt`, `rule`, `config`, `init`, and `explain` commands.
- Lossless Makefile syntax and continuation-aware logical parsing.
- Cross-file semantic analysis for variables, targets, dependencies, includes, and references.
- Safe, side-effect-free partial GNU Make evaluation with provenance and three-valued conditions.
- Static expansion of includes, targets, prerequisites, substitution references, and common pure
  Make functions.
- Project rules for separator conflicts, duplicate recipes, dependency and include cycles,
  unresolved includes, undefined references, and unreachable targets.
- Rumdl-compatible configuration discovery, inheritance, inline suppressions, per-file ignores,
  severities, output formats, and exit behavior.
- Controlled GNU Make parity fixtures and a production-style regression corpus.
- Conservative autofixes for missing `.PHONY` declarations and direct recursive Make invocations.

### Changed

- Include graphs are evaluated in GNU Make statement order, including repeated include sites.
- Reachability uses GNU Make's inferred default goal when explicit entry targets are absent.
- Predefined variables behave like protected command-line assignments unless `override` is used.

### Security

- Recipes, shell assignments, and side-effecting Make functions are never executed during linting.
- Release packages use an explicit source allowlist that excludes private planning documents.
- Release artifacts are checksummed and prepared for GitHub build-provenance attestations.

[Unreleased]: https://github.com/rvben/rumk/commits/main
[0.0.8]: https://github.com/rvben/rumk/releases/tag/v0.0.8
[0.0.7]: https://github.com/rvben/rumk/releases/tag/v0.0.7
[0.0.6]: https://github.com/rvben/rumk/releases/tag/v0.0.6
