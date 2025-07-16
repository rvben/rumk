# rumk

A fast linter for Makefiles written in Rust, inspired by tools like Ruff (Python) and Rumdl (Markdown).

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

# Check all Makefiles in a directory
rumk check path/to/directory/

# Auto-fix issues
rumk check --fix

# Explain a specific rule
rumk explain MK001
```

### Configuration

Create a `.rumk.toml` file in your project:

```toml
[rules]
"MK101" = { enabled = true, options = { max = 100 } }
"MK102" = { enabled = true, options = { style = "UPPER_CASE" } }
"MK201" = { enabled = true }

[ignore]
paths = ["vendor/*", "third_party/*"]
rules = ["MK101"]
```

## Rules

### Syntax Rules (MK000-MK099)
- `MK001` - Recipes must use tab indentation
- `MK002` - Invalid variable syntax

### Style Rules (MK100-MK199)
- `MK101` - Line exceeds maximum length
- `MK102` - Variable naming convention
- `MK103` - Target naming convention

### Best Practice Rules (MK200-MK299)
- `MK201` - Non-file targets should be .PHONY
- `MK202` - Avoid hardcoded absolute paths

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
Makefile:2:1: [MK001] Recipe must be indented with tab, not spaces [*]
Makefile:4:7: [MK202] Variable 'FOO' contains hardcoded absolute path
Makefile:1:1: [MK201] Target 'clean' should be declared .PHONY
Makefile:6:1: [MK201] Target 'test' should be declared .PHONY

Found 4 issues in 1 file (1 file checked)
Run with --fix to automatically fix issues
```

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

MIT License - see [LICENSE](LICENSE) for details.