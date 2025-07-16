# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

rumk is a Rust-based linter for Makefiles, inspired by tools like Ruff (Python) and Rumdl (Markdown). It provides syntax validation, style checking, best practices enforcement, and auto-fixing capabilities.

## Development Commands

```bash
# Build the project
make build

# Run tests
make test

# Run linter
make lint

# Format code
make fmt

# Run all checks (lint, build, test)
make

# Install locally
make install

# Run a single test
cargo test test_parse_simple_rule

# Build and run rumk directly
cargo run -- check examples/bad.mk
cargo run -- check examples/  # Check all Makefiles in directory
cargo run -- check --fix examples/bad.mk
cargo run -- explain MK001
```

## Architecture

### Core Modules

1. **Parser** (`src/parser.rs`): Implements a custom Makefile parser that creates an AST with:
   - Rules (targets, prerequisites, recipes)
   - Variables (with assignment types: =, :=, ?=, +=)
   - Includes and exports
   - .PHONY declarations
   - Comments

2. **Rule System** (`src/rules/`): Trait-based rule architecture where each rule:
   - Implements the `Rule` trait
   - Returns `Diagnostic` objects with severity, location, and optional fixes
   - Categories: syntax, style, best_practices, security, performance

3. **Diagnostic System** (`src/diagnostic.rs`): Represents linting issues with:
   - Location information (line, column)
   - Severity levels (Error, Warning, Info)
   - Optional auto-fix information via `Fix` and `Edit` structures

4. **Configuration** (`src/config.rs`): TOML-based configuration that:
   - Looks for `.rumk.toml`, `rumk.toml`, or `.config/rumk.toml`
   - Allows enabling/disabling rules
   - Supports rule-specific options
   - Handles path and rule ignoring

5. **Fix Engine** (`src/fix.rs`): Applies automatic fixes by:
   - Sorting diagnostics in reverse order to avoid offset issues
   - Applying text edits to fix issues
   - Preserving file structure

### Adding New Rules

1. Create a new struct implementing the `Rule` trait in the appropriate module
2. Add it to `get_all_rules()` or `get_default_rules()` in `src/rules.rs`
3. Implement the `check()` method to return diagnostics
4. Optionally provide fixes by adding `Fix` objects to diagnostics

### Key Design Decisions

- Parser handles GNU Make syntax specifically
- Rules are stateless and implement a common trait
- Diagnostics include enough information for both reporting and fixing
- Configuration uses TOML for familiarity and simplicity
- CLI uses clap with subcommands (check, fix, explain)

## Testing Examples

The `examples/` directory contains:
- `bad.mk`: Makefile with various linting issues for testing
- `good.mk`: Well-formatted Makefile following best practices

## Configuration Example

```toml
[rules]
"style/line-length" = { enabled = true, options = { max = 100 } }
"style/variable-naming" = { enabled = true, options = { style = "UPPER_CASE" } }

[ignore]
paths = ["vendor/*"]
rules = ["style/line-length"]
```