.PHONY: all build test lint fmt fmt-check clean install run check-examples
.PHONY: msrv-check dependency-check check-gnu-fixtures check-corpus release-check help

# Configuration
CARGO = cargo
MSRV = 1.82.0
INSTALL_PREFIX = /usr/local
BINARY_NAME = rumk

all: lint build test

build:
	$(CARGO) build --release

test:
	$(CARGO) test --all-targets --all-features

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --all -- --check

msrv-check:
	$(CARGO) +$(MSRV) check --locked --all-targets --all-features

dependency-check: fmt-check all msrv-check

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

check-corpus:
	$(CARGO) test --test corpus_test

release-check: fmt-check lint test check-gnu-fixtures check-corpus
	ALLOW_DIRTY=1 ./scripts/validate-release.sh

help:
	@echo "Available targets:"
	@echo "  all     - Run lint, build, and test"
	@echo "  build   - Build release binary"
	@echo "  test    - Run tests"
	@echo "  lint    - Run clippy linter"
	@echo "  fmt     - Format code"
	@echo "  fmt-check - Verify Rust formatting without changing files"
	@echo "  msrv-check - Verify compatibility with Rust $(MSRV)"
	@echo "  dependency-check - Run every dependency-update validation gate"
	@echo "  clean   - Clean build artifacts"
	@echo "  install - Install binary to $(INSTALL_PREFIX)/bin"
	@echo "  run     - Run rumk on this Makefile"
	@echo "  check-examples - Check example Makefiles"
	@echo "  check-gnu-fixtures - Verify parser fixtures with GNU Make"
	@echo "  check-corpus - Verify production-style Makefile projects"
	@echo "  release-check - Run every local release gate and package dry run"
