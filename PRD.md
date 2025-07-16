# Product Requirements Document: Makefile Linter

## Product Overview

### Vision
A fast and user-friendly linter for Makefiles written in Rust, inspired by the design philosophy and user experience of tools like Ruff (Python) and Rumdl (Markdown).

### Product Name
**makelint** (or **makecheck**, **mkl**, **ruffle** - final name TBD)

### Problem Statement
Makefiles are critical build automation tools used across countless projects, but they suffer from:
- Inconsistent formatting and style across projects
- Common errors that are hard to detect (missing dependencies, circular dependencies)
- Portability issues between different Make implementations
- Lack of modern tooling for static analysis
- No standardized best practices enforcement

### Target Users
- Software developers using Make for build automation
- DevOps engineers maintaining CI/CD pipelines
- Open source project maintainers
- Teams wanting to enforce Makefile standards

## Core Features

### 1. Syntax Validation
- **Parser**: Full Makefile parser supporting GNU Make syntax
- **Error Detection**:
  - Syntax errors
  - Invalid variable references
  - Malformed rules
  - Tab vs space issues in recipes

### 2. Style Checking
- **Configurable Rules**:
  - Indentation (tabs in recipes, spaces elsewhere)
  - Line length limits
  - Variable naming conventions (UPPER_CASE, lower_case, etc.)
  - Target naming conventions
  - Comment formatting
  - Whitespace consistency

### 3. Best Practices Enforcement
- **Security**:
  - Detect hardcoded secrets/credentials
  - Unsafe shell command patterns
  - Missing `.PHONY` declarations
- **Performance**:
  - Detect inefficient patterns
  - Suggest parallel execution opportunities
  - Identify redundant rules
- **Portability**:
  - GNU Make vs POSIX Make compatibility
  - Shell compatibility issues
  - Platform-specific command usage

### 4. Static Analysis
- **Dependency Analysis**:
  - Circular dependency detection
  - Missing dependency detection
  - Unused targets/variables
  - Unreachable targets
- **Variable Analysis**:
  - Undefined variable usage
  - Unused variable detection
  - Variable shadowing
- **Include Analysis**:
  - Missing include files
  - Circular includes
  - Include path validation

### 5. Auto-fixing
- **Safe Fixes**:
  - Whitespace normalization
  - Tab/space corrections
  - Variable style fixes
  - Comment formatting
- **Suggested Fixes**:
  - Best practice violations
  - Performance improvements
  - Security issues

## Technical Requirements

### Performance Goals
- Process 1000+ line Makefile in <100ms
- Minimal memory footprint
- Zero dependencies for core functionality
- Native binary, no runtime required

### Architecture
- **Language**: Rust
- **Parser**: Custom recursive descent parser or nom-based parser
- **Configuration**: TOML-based config files
- **Rules**: All rules compiled into the binary for maximum performance

### CLI Interface
```bash
# Basic usage
makelint check Makefile

# Auto-fix issues
makelint fix Makefile

# Check with custom config
makelint check --config .makelint.toml

# Output formats
makelint check --format json
makelint check --format github  # GitHub Actions annotations

# Watch mode
makelint watch

# Explain a rule
makelint explain no-hardcoded-paths
```

### Configuration
```toml
# .makelint.toml
[rules]
line-length = { max = 120 }
variable-naming = { style = "UPPER_CASE" }
target-naming = { style = "lower_case" }
indent-style = { recipes = "tab", other = "space" }

[ignore]
paths = ["vendor/*", "third_party/*"]
rules = ["line-length"]

# Additional rule configurations can be added here
```

### Editor Integration
- **LSP Server**: Full Language Server Protocol support
- **Extensions**:
  - VS Code extension
  - Vim/Neovim plugin
  - Emacs package
  - Sublime Text package

### CI/CD Integration
- **GitHub Actions**: Native action support
- **GitLab CI**: Docker image
- **Pre-commit**: Hook configuration
- **Exit Codes**: Meaningful exit codes for CI

## Rule Categories

### Syntax Rules
- `syntax/invalid-syntax`: General syntax errors
- `syntax/tab-in-recipe`: Recipes must use tabs
- `syntax/missing-colon`: Target missing colon
- `syntax/invalid-variable`: Invalid variable syntax

### Style Rules
- `style/line-length`: Line exceeds maximum length
- `style/variable-naming`: Variable naming convention
- `style/target-naming`: Target naming convention
- `style/whitespace`: Inconsistent whitespace

### Best Practice Rules
- `practice/missing-phony`: Non-file targets should be .PHONY
- `practice/recursive-variable`: Avoid recursive variables when possible
- `practice/shell-in-variable`: Avoid $(shell) in variable definitions
- `practice/hardcoded-path`: Avoid hardcoded absolute paths

### Security Rules
- `security/dangerous-rm`: Unsafe rm commands
- `security/eval-usage`: Dangerous eval usage
- `security/credential-exposure`: Possible credential in Makefile

### Performance Rules
- `perf/sequential-targets`: Targets could run in parallel
- `perf/redundant-prerequisites`: Duplicate prerequisites
- `perf/expensive-shell`: Expensive shell operations in variables

## Implementation Roadmap

### Phase 1: Core Parser & Basic Rules (MVP)
- Makefile parser
- Basic syntax validation
- Core style rules (indentation, line length)
- CLI with check command
- JSON output format

### Phase 2: Advanced Analysis
- Dependency graph analysis
- Variable tracking
- Best practice rules
- Auto-fix capability
- TOML configuration

### Phase 3: IDE Integration
- LSP server implementation
- VS Code extension
- Pre-commit hook support
- GitHub Actions integration

### Phase 4: Community & Polish
- Rule documentation generator
- Performance optimizations
- Rule contribution guidelines
- Comprehensive rule set expansion

## Success Metrics
- Adoption by 100+ projects within 6 months
- <100ms processing time for 95% of Makefiles
- 90%+ user satisfaction score
- Active contributor community (10+ contributors)

## Competitive Analysis

### Existing Tools
- **checkmake**: Go-based, limited rules, no auto-fix
- **unmake**: Python-based, slow, limited features
- **make lint** (various): Shell scripts, not comprehensive

### Our Advantages
- **Performance**: Rust-based, 10-100x faster
- **Comprehensive**: Full set of built-in rules
- **User Experience**: Inspired by modern tools (Ruff)
- **IDE Integration**: First-class LSP support
- **Auto-fix**: Safe automatic fixes

## Open Questions
1. Should we support BSD Make syntax variations?
2. How to handle Make includes and dynamic content?
3. How to handle community rule contributions?
4. Versioning strategy for rules?
5. How to handle Make functions in analysis?

## Appendix: Example Usage

### Example Makefile with Issues
```makefile
# Missing .PHONY declaration
clean:
    rm -rf build/  # Dangerous rm usage

# Inconsistent indentation
build: src/*.c
        gcc -o app $^  # Mixed tabs/spaces

# Poor variable naming
foo = bar
FOO = baz  # Variable shadowing

# Hardcoded path
INSTALL_DIR = /usr/local/bin

# Missing dependencies
test:
    pytest tests/
```

### Linter Output
```
Makefile:2:1: error: Target 'clean' should be declared .PHONY [practice/missing-phony]
Makefile:3:5: warning: Dangerous rm command without safeguards [security/dangerous-rm]
Makefile:7:1: error: Recipe must use tab indentation [syntax/tab-in-recipe]
Makefile:11:1: warning: Variable 'FOO' shadows 'foo' [style/variable-shadowing]
Makefile:14:15: warning: Hardcoded absolute path [practice/hardcoded-path]
Makefile:17:1: info: Target 'test' has no prerequisites [practice/missing-deps]

Found 2 errors, 3 warnings, 1 info
```