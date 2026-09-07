.PHONY: all build test lint fmt fmt-check clean install run check-examples
.PHONY: msrv-check dependency-check check-gnu-fixtures check-corpus fuzz check-fuzz
.PHONY: release-check benchmark help check-comparison

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

check-comparison:
	$(CARGO) test --test comparison_corpus_test --test policy_test --test formatting_test --test phony_precision_test

check-corpus:
	$(CARGO) test --test corpus_test

# Requires Python 3.9+. Pass BENCH_ARGS='--baseline /path/to/old/rumk'
# to verify identical output while comparing two release binaries.
BENCH_ARGS ?=
benchmark: build
	python3 scripts/benchmark.py $(BENCH_ARGS)

# Fuzzing needs nightly Rust and cargo-fuzz (cargo install cargo-fuzz).
#
# cargo-fuzz defaults --target to the triple cargo-fuzz itself was built for
# rather than the host it runs on, so a prebuilt musl-static cargo-fuzz makes
# every run die with "sanitizer is incompatible with statically linked libc"
# before it fuzzes a single input, having produced no artifacts to notice.
# Naming this host's own triple keeps that explicit, on macOS and Linux alike.
FUZZ_TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
FUZZ_TIME ?= 60
# The test fixtures go in as read-only seeds. They are small Makefiles that
# already reach constructs random bytes need a long time to arrive at, and a
# scheduled run starts from whatever corpus it is given, so what they cover is
# where the fuzzer starts rather than where it has to get to.
FUZZ_SEEDS = tests/fixtures/gnu_make tests/fixtures/corpus examples
fuzz:
	@test -n "$(FUZZ_TARGET)" || { echo "FUZZ_TARGET is empty; could not read the host triple"; exit 1; }
	@for target in $$($(CARGO) +nightly fuzz list); do \
		echo "=== fuzzing $$target for $(FUZZ_TIME)s on $(FUZZ_TARGET) ==="; \
		mkdir -p fuzz/corpus/$$target; \
		$(CARGO) +nightly fuzz run --target $(FUZZ_TARGET) $$target \
			fuzz/corpus/$$target $(FUZZ_SEEDS) -- -max_total_time=$(FUZZ_TIME) || exit 1; \
	done

# The fuzz crate is built only by the scheduled Fuzz workflow, so a change to
# the library can leave a target uncompilable until then, and `fuzz` stops at
# the first target that fails to build, which leaves every target after it
# unfuzzed. Type-checking the crate needs neither nightly nor a sanitizer, so
# every commit can afford it.
check-fuzz:
	$(CARGO) check --manifest-path fuzz/Cargo.toml --bins

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
	@echo "  check-comparison - Verify linter comparison and formatting regressions"
	@echo "  benchmark - Measure include graphs, large files, and bulk fixes"
	@echo "  fuzz    - Fuzz every target for FUZZ_TIME seconds (needs nightly)"
	@echo "  check-fuzz - Type-check the fuzz targets against the library"
	@echo "  release-check - Run every local release gate and package dry run"
