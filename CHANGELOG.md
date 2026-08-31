# Changelog

All notable changes to Rumk are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `MK101` safely wraps long static `.PHONY` declarations at the configured line length while
  preserving target order, inline comments, line endings, and fix idempotence.

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
[0.0.6]: https://github.com/rvben/rumk/releases/tag/v0.0.6
