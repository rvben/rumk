# Changelog

All notable changes to Rumk are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/rvben/rumk/compare/v0.0.8...v0.1.0) - 2026-09-28

### Added

- Per-Makefile configuration discovery for CLI linting, formatting, and coverage,
  with local directory filters and command-line overrides applied to nested configs.
- `rumk config file <PATH> --explain` reports the selected, inherited, and shadowed
  configurations; CLI discovery warns once per selected config about shadowed files.
- `rumk init --pyproject` safely adds settings to existing Python project files,
  preserving comments, formatting, permissions, and other tools' settings.
- Configuration errors identify inherited files and use namespaced pyproject settings.

- Configuration discovery and explicit loading support `[tool.rumk]` in
  `pyproject.toml`, with existing precedence, inheritance, and project boundaries.
  `rumk init --output pyproject.toml` creates namespaced settings and atomically
  refuses to overwrite an existing file or symlink.

- `rumk coverage` reports MK216 blockers and dependency outcomes using the current
  configuration, without executing Make or requiring the rule to be enabled.

- MK216 follows static selective `vpath` searches, preserving directive expansion
  timing, include order, repeated patterns, and clearing behavior.

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

- **analysis**: check static pattern, suffix rule and recipe-only roots in MK216 ([f45ab68](https://github.com/rvben/rumk/commit/f45ab68cf6154c57f693e66dcb00ff6d9f7516c7))
- **project**: read wildcard includes and expand include lists as Make does ([c6b5b67](https://github.com/rvben/rumk/commit/c6b5b67fa964e4bc7884006d50b770dd04744561))
- **cli**: generate shell completion scripts ([6000d55](https://github.com/rvben/rumk/commit/6000d55ca81a5a41f4b6fd7c01009829ecdedce3))
- **output**: report the range each finding covers ([5883728](https://github.com/rvben/rumk/commit/5883728f115c821e68a9e57a153a87b9e720c7c7))
- **cli**: show default status and fix kind in the rule list ([6529d44](https://github.com/rvben/rumk/commit/6529d445a9158392ce2a6888cf323bc50737dd9f))
- **analysis**: localize unresolved explicit prerequisites ([5ff7354](https://github.com/rvben/rumk/commit/5ff73543789d18ee4be96d85acc5cbf5add59182))
- **vscode**: add bundled Makefile extension and preview releases ([27ca740](https://github.com/rvben/rumk/commit/27ca740f41109cf345b3e6a6935a2799a028fd58))

### Changed

- MK216 indexes declared targets and possible implicit input names per analysis,
  avoiding repeated full-directory scans while preserving uncertainty. The
  generated benchmark now includes missing-prerequisite fan-out and working controls.

- `global.dialect` accepts `gnu`, `posix2017` (alias `posix`), and `posix2024`.
  Unknown values, including the previously ineffective `bsd`, now fail validation.
  POSIX profiles enable MK301; the 2017 profile disables GNU `.PHONY` advice by
  default. Explicit rule configuration still overrides these defaults.

### Fixed

- The language server recovers from malformed JSON in complete frames while
  retaining open buffers. Malformed requests and misclassified lifecycle messages
  are rejected before changing server state; valid client responses remain silent.

- MK301 accepts portable whitespace in the entry `.POSIX:` marker and reports
  forbidden commands and prerequisites on standard special targets. Assignment
  spacing checks use indexed logical statements, including continuations,
  instead of rescanning preceding physical lines for every assignment.

- Editor analysis no longer adds file-local project-rule findings that contradict
  included declarations. Quick fixes honor selected edit spans and reject invalid
  ranges. Fix-all stabilizes overlapping edits while retaining unsaved includes
  and the configured safety policy.

- MK216 no longer treats reuse of the same implicit rule within a chain as a
  possible producer for that rule’s missing input.

- Unsaved buffers resolve their existing parent directory before analysis, so
  Windows path spelling and symlink aliases cannot detach SARIF/JSON edits from
  the buffer that owns them.

- **project**: name wildcard include matches as the pattern spells the directory ([3c96e91](https://github.com/rvben/rumk/commit/3c96e9172fb5510312a115608b5b93456e482f5f))
- **rules**: do not ask MK201 to mark a directory target phony ([b562868](https://github.com/rvben/rumk/commit/b562868313e79877d51fad969d07daa0542bf7de))
- **rules**: count a target's value in MK211 only where Make can see it ([98b84bd](https://github.com/rvben/rumk/commit/98b84bdfc4276aefd62caba1fc6cff0d6d550b4b))
- **cli**: check a file named on the command line whatever its name ([b1483bb](https://github.com/rvben/rumk/commit/b1483bb4712ede6d1addb416316df5c2a7bb734b))
- **lsp**: keep an invalid inline directive to the file it is written in ([4ede305](https://github.com/rvben/rumk/commit/4ede3051582458fdf954fbdfe15b1a8d593f3a74))
- **cli**: suggest a fix command that names the files and options of the run ([f42b71a](https://github.com/rvben/rumk/commit/f42b71a3fece35a70f9fa47788909ed6316e8435))
- **rules**: accept a shell variable captured by the next assignment in MK213 ([50faca3](https://github.com/rvben/rumk/commit/50faca376374a9613fe7e53386e21e07ba6ffd67))
- **rules**: read shell comments in recipe commands ([264f914](https://github.com/rvben/rumk/commit/264f9148c7d8b2a49df2e957e1c7069145261fec))
- **parser**: keep the command a recipe comment is continued onto ([c7037dd](https://github.com/rvben/rumk/commit/c7037ddfbea6d8a74773a9317e7b56d89e649500))
- **rules**: see a quoted or continued make invocation in MK203 ([8a889ce](https://github.com/rvben/rumk/commit/8a889cee3ec223ea5ad72fa9982ad01d203d0f3a))
- **portability**: read a '%' only in the replacement as suffix substitution ([407452a](https://github.com/rvben/rumk/commit/407452a60e322b36de9109360352a287db780a8b))
- **parser**: keep a ';' after a target variable assignment in its value ([d9159c5](https://github.com/rvben/rumk/commit/d9159c5c87b3fe81885b6bd8f08410bfa7f85360))
- **lsp**: keep requests and diagnostics across disk-only changes ([08e6ac3](https://github.com/rvben/rumk/commit/08e6ac3cb60e8f10b47ed6a66e39412f5e4cae70))
- **eval**: expand each recursive variable once per expansion ([e0f5403](https://github.com/rvben/rumk/commit/e0f540308fe38468a8338c60b5cb0c7a11a4c7f5))
- **cli**: say a missing path does not exist ([5eb0586](https://github.com/rvben/rumk/commit/5eb058679db5347f508f3e888ccfee80418f9727))
- **cli**: summarize a text check of stdin ([bfbfbc2](https://github.com/rvben/rumk/commit/bfbfbc2e3229fe49e76b8e01c32de22c280d65c1))
- **cli**: make explain show the full rule detail for any rule code case ([567a718](https://github.com/rvben/rumk/commit/567a718fca137e809867cec1be156f78dc7089f6))
- **output**: stop emitting an empty color span after unfixable findings ([e038ea4](https://github.com/rvben/rumk/commit/e038ea4f49893f4270353b0c11567c97cc82f7ca))
- **output**: name the rule in GitHub Actions annotations ([17f5a03](https://github.com/rvben/rumk/commit/17f5a03594e9f150eff103fed0a1e9a6c85e4a2f))
- **output**: list a file's project findings in line order ([3645150](https://github.com/rvben/rumk/commit/3645150990fba64658d3aa02c3fba9e791c31c68))
- **analysis**: distinguish inert text from function exclusions ([2f54a4f](https://github.com/rvben/rumk/commit/2f54a4f17adc046baf762617ba8c49d3e3a7079a))
- **analysis**: distinguish relative files from suffix rules ([43eda56](https://github.com/rvben/rumk/commit/43eda56a7d79b19e3d3d4d3352b233a3367b5d3e))
- **phony**: preserve explicit stamp-file targets ([1867f9f](https://github.com/rvben/rumk/commit/1867f9fe98ed5c2ddc060495b4c5d54f2b5e8714))
- **syntax**: honor evaluated conditional activity ([9d5563a](https://github.com/rvben/rumk/commit/9d5563a746a84bd2a4554a1c0e80881320a8d8e7))
- **lsp**: reject conflicting configuration flags ([569380e](https://github.com/rvben/rumk/commit/569380e736d2b1491885823a03f8ee0afab4a1d0))
- **bench**: reject changing executables and invalid timings ([416adcf](https://github.com/rvben/rumk/commit/416adcf9b8cc563300749d06b5aff981aa16456f))

### Performance

- **project**: look up parsed records by line while loading ([b0fbe43](https://github.com/rvben/rumk/commit/b0fbe433463ac0fd21e0b5439a979bba32cb6157))

## [0.0.8](https://github.com/rvben/rumk/releases/tag/v0.0.8) - 2026-09-08

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

## [0.0.7](https://github.com/rvben/rumk/releases/tag/v0.0.7) - 2026-08-31

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

## [0.0.6](https://github.com/rvben/rumk/releases/tag/v0.0.6) - 2026-08-31

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
