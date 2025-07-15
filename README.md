# rumk

A fast, extensible linter for Makefiles written in Rust, inspired by tools like Ruff (Python) and Rumdl (Markdown).

## Features

- **Fast**: Written in Rust for maximum performance
- **Comprehensive**: Checks syntax, style, best practices, and security issues
- **Configurable**: Customize rules via TOML configuration
- **Auto-fix**: Automatically fix common issues
- **Multiple output formats**: Text, JSON, and GitHub Actions annotations

## Installation

```bash
cargo install rumk
```

## Usage

### Basic usage

```bash
# Check a Makefile
rumk check

# Check a specific file
rumk check path/to/Makefile

# Auto-fix issues
rumk fix

# Explain a specific rule
rumk explain syntax/tab-in-recipe
```

### Configuration

Create a `.rumk.toml` file in your project:

```toml
[rules]
"style/line-length" = { enabled = true, options = { max = 100 } }
"style/variable-naming" = { enabled = true, options = { style = "UPPER_CASE" } }
"practice/missing-phony" = { enabled = true }

[ignore]
paths = ["vendor/*", "third_party/*"]
rules = ["style/line-length"]
```

## Rules

### Syntax Rules
- `syntax/tab-in-recipe` - Recipes must use tab indentation
- `syntax/invalid-variable` - Invalid variable syntax

### Style Rules
- `style/line-length` - Line exceeds maximum length
- `style/variable-naming` - Variable naming convention
- `style/target-naming` - Target naming convention

### Best Practice Rules
- `practice/missing-phony` - Non-file targets should be .PHONY
- `practice/hardcoded-path` - Avoid hardcoded absolute paths

## Example

Given this Makefile:

```makefile
clean:
    rm -rf build/  # Uses spaces instead of tab

FOO = /usr/local/bin  # Hardcoded path

test:
	pytest tests/
```

Running `rumk check` produces:

```
Makefile:2:1: error: Recipe must be indented with tab, not spaces [syntax/tab-in-recipe]
Makefile:4:7: warning: Variable 'FOO' contains hardcoded absolute path [practice/hardcoded-path]
Makefile:1:1: warning: Target 'clean' should be declared .PHONY [practice/missing-phony]
Makefile:6:1: warning: Target 'test' should be declared .PHONY [practice/missing-phony]

Found 1 error, 3 warnings
```

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

MIT License - see [LICENSE](LICENSE) for details.