.PHONY: all build test lint fmt clean install run check-examples check-gnu-fixtures help
.PHONY: release-patch release-minor release-major

# Configuration
CARGO = cargo
INSTALL_PREFIX = /usr/local
BINARY_NAME = rumk

all: lint build test

build:
	$(CARGO) build --release

test:
	$(CARGO) test

lint:
	$(CARGO) clippy -- -D warnings

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
	rm -rf target/

install: build
	install -m 755 target/release/$(BINARY_NAME) $(INSTALL_PREFIX)/bin/

run: build
	./target/release/$(BINARY_NAME) check Makefile

check-examples: build
	./target/release/$(BINARY_NAME) check examples/good.mk
	@if ./target/release/$(BINARY_NAME) check examples/bad.mk >/dev/null 2>&1; then \
		echo "Expected examples/bad.mk to fail linting"; exit 1; \
	else \
		echo "Confirmed examples/bad.mk contains detectable violations"; \
	fi

check-gnu-fixtures:
	$(CARGO) test --test gnu_make_test

help:
	@echo "Available targets:"
	@echo "  all     - Run lint, build, and test"
	@echo "  build   - Build release binary"
	@echo "  test    - Run tests"
	@echo "  lint    - Run clippy linter"
	@echo "  fmt     - Format code"
	@echo "  clean   - Clean build artifacts"
	@echo "  install - Install binary to $(INSTALL_PREFIX)/bin"
	@echo "  run     - Run rumk on this Makefile"
	@echo "  check-examples - Check example Makefiles"
	@echo "  check-gnu-fixtures - Verify parser fixtures with GNU Make"

release-patch:
	vership bump patch

release-minor:
	vership bump minor

release-major:
	vership bump major
