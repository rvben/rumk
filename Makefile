.PHONY: all build test lint fmt fmt-check clean install run check-examples
.PHONY: msrv-check dependency-check check-gnu-fixtures check-corpus fuzz check-fuzz
.PHONY: release-check benchmark help check-comparison check-semantic
.PHONY: ci ci-tools ci-quality ci-test ci-compat ci-package check-hooks check-scripts
.PHONY: release-tools release-validate release-build release-sdist release-checksums
.PHONY: release-draft release-publish-crate release-pypi-dist release-finalize release-move-action-tag
.PHONY: vscode-deps vscode-bundle vscode-test vscode-integration vscode-package vscode-verify-packages vscode-publish

# Configuration
CARGO = cargo
MSRV = 1.85.0
INSTALL_PREFIX = /usr/local
BINARY_NAME = rumk
VSCODE = editors/vscode
# Platform packages downloaded for publication, relative to $(VSCODE).
VSCODE_PACKAGES ?= packages
PYTHON ?= python3
PRE_COMMIT_SPEC = pre-commit==4.6.0
MATURIN_SPEC = maturin>=1.8,<2.0
# ci-tools installs the pinned Python tools here, so local runs use what CI uses.
CI_VENV = target/ci-tools
CI_BIN = $(CI_VENV)/$(if $(filter Windows_NT,$(OS)),Scripts,bin)
HOST_TARGET = $(shell rustc -vV | sed -n 's/^host: //p')
EXE = $(if $(filter Windows_NT,$(OS)),.exe,)
# Integration suites that compare Rumk against GNU Make and other linters.
COMPARISON_TESTS = comparison_corpus_test policy_test formatting_test phony_precision_test \
	rebuild_test recipe_prefixes_test
COMPAT_TESTS = gnu_make_test corpus_test missing_prerequisite_test $(COMPARISON_TESTS)
# validate-release.sh refuses uncommitted changes unless ALLOW_DIRTY=1.
ALLOW_DIRTY ?= 0

all: lint build test

build:
	$(CARGO) build --locked --release

test:
	$(CARGO) test --locked --all-targets --all-features

lint:
	$(CARGO) clippy --locked --all-targets --all-features -- -D warnings

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --all -- --check

msrv-check:
	$(CARGO) +$(MSRV) check --locked --all-targets --all-features

# Dependency updates arrive as uncommitted changes to the lockfiles.
dependency-check: ALLOW_DIRTY = 1
dependency-check: ci

# Each ci-* target is one job of the CI workflow; `make ci` runs them all.
ci: ci-quality ci-test ci-compat msrv-check ci-package

ci-tools:
	$(PYTHON) -m venv $(CI_VENV)
	$(CI_BIN)/python -m pip install --quiet "$(PRE_COMMIT_SPEC)" "$(MATURIN_SPEC)"

ci-quality: fmt-check lint check-fuzz check-hooks check-scripts

check-hooks: ci-tools
	$(CI_BIN)/python scripts/verify-hooks.py --pre-commit $(abspath $(CI_BIN))/pre-commit

check-scripts:
	$(CARGO) build --locked --bin rumk --example prerequisite-coverage
	$(PYTHON) -m unittest discover -s scripts -p 'test_*.py'

ci-test: test build
	target/release/$(BINARY_NAME)$(EXE) version
	target/release/$(BINARY_NAME)$(EXE) check Makefile
	target/release/$(BINARY_NAME)$(EXE) check examples/good.mk

ci-compat:
	$(MAKE) --version
	$(CARGO) test --locked $(addprefix --test ,$(COMPAT_TESTS))

# Builds into its own directory so the smoke test installs this build's wheel.
CI_PACKAGE_DIR = target/ci-package
ci-package: ci-tools
	ALLOW_DIRTY=$(ALLOW_DIRTY) ./scripts/validate-release.sh
	rm -rf $(CI_PACKAGE_DIR)
	$(CI_BIN)/maturin build --locked --release --sdist --out $(CI_PACKAGE_DIR)/dist
	$(PYTHON) -m venv $(CI_PACKAGE_DIR)/venv
	$(CI_PACKAGE_DIR)/venv/bin/python -m pip install $(CI_PACKAGE_DIR)/dist/*.whl
	$(CI_PACKAGE_DIR)/venv/bin/$(BINARY_NAME) version

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
	$(CARGO) test --locked --test gnu_make_test

check-comparison:
	$(CARGO) test --locked $(addprefix --test ,$(COMPARISON_TESTS))

check-semantic:
	$(CARGO) build --bin rumk
	$(CARGO) test --test missing_prerequisite_test
	python3 -m unittest discover -s scripts -p 'test_semantic_benchmark.py'

check-corpus:
	$(CARGO) test --locked --test corpus_test

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
FUZZ_TARGET ?= $(HOST_TARGET)
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
	$(CARGO) check --locked --manifest-path fuzz/Cargo.toml --bins

release-check: fmt-check lint test check-gnu-fixtures check-corpus
	ALLOW_DIRTY=1 ./scripts/validate-release.sh

# Release targets, run by the Release workflow in this order: release-validate,
# release-build per platform plus release-sdist, release-checksums over the
# collected RELEASE_DIST, then the publication steps for a pushed tag.
# pkgid ends in #VERSION or #NAME@VERSION; keep what follows the last separator.
RELEASE_VERSION ?= $(shell $(CARGO) pkgid --locked | sed 's/.*[^[:alnum:].+-]//')
RELEASE_TAG ?= v$(RELEASE_VERSION)
# 0.0.x and suffixed versions are published as GitHub prereleases.
RELEASE_PRERELEASE ?= $(if $(or $(filter 0.0.%,$(RELEASE_VERSION)),$(findstring -,$(RELEASE_VERSION))),true,false)
RELEASE_TARGET ?= $(HOST_TARGET)
RELEASE_FORMAT ?= $(if $(findstring windows,$(RELEASE_TARGET)),zip,tar.gz)
RELEASE_EXE = $(if $(findstring windows,$(RELEASE_TARGET)),.exe,)
RELEASE_DIST ?= dist
# Linux wheels link against an old glibc through zig so they install broadly.
# maturin runs zig as `python3 -m ziglang`, so the tools' environment goes first on PATH.
RELEASE_WHEEL_ARGS = $(if $(findstring linux,$(RELEASE_TARGET)),--compatibility manylinux2014 --zig,)
RELEASE_WHEEL_ENV = $(if $(findstring linux,$(RELEASE_TARGET)),PATH="$(abspath $(CI_BIN)):$$PATH",)
RELEASE_MATURIN_SPEC = $(subst maturin,maturin$(if $(findstring linux,$(HOST_TARGET)),[zig],),$(MATURIN_SPEC))

# Installs into the ci-tools environment; runner Pythons may refuse global installs.
release-tools:
	$(PYTHON) -m venv $(CI_VENV)
	$(CI_BIN)/python -m pip install --quiet "$(RELEASE_MATURIN_SPEC)"

# Pass RELEASE_EXPECTED=<tag> to require that Cargo.toml names that version.
RELEASE_EXPECTED ?=
release-validate:
	./scripts/validate-release.sh "$(RELEASE_EXPECTED)"

release-build:
	$(CARGO) build --locked --release --target $(RELEASE_TARGET)
	target/$(RELEASE_TARGET)/release/$(BINARY_NAME)$(RELEASE_EXE) version
	target/$(RELEASE_TARGET)/release/$(BINARY_NAME)$(RELEASE_EXE) check Makefile
	./scripts/package-release.sh $(RELEASE_TARGET) $(RELEASE_VERSION) $(RELEASE_FORMAT)
	$(RELEASE_WHEEL_ENV) $(CI_BIN)/maturin build --locked --release --target $(RELEASE_TARGET) $(RELEASE_WHEEL_ARGS) \
		--out $(RELEASE_DIST)

release-sdist:
	$(CI_BIN)/maturin sdist --out $(RELEASE_DIST)

release-checksums:
	./scripts/release-checksums.sh $(RELEASE_DIST)

RELEASE_PUBLISH = ./scripts/release-publish.sh
RELEASE_PUBLISH_ARGS = $(RELEASE_TAG) $(RELEASE_VERSION) $(RELEASE_PRERELEASE) $(RELEASE_DIST)

# Needs GH_TOKEN.
release-draft:
	$(RELEASE_PUBLISH) draft $(RELEASE_PUBLISH_ARGS)

# Needs CARGO_REGISTRY_TOKEN.
release-publish-crate:
	$(RELEASE_PUBLISH) crate $(RELEASE_PUBLISH_ARGS)

release-pypi-dist:
	$(RELEASE_PUBLISH) pypi-dist $(RELEASE_PUBLISH_ARGS)

# Needs GH_TOKEN.
release-finalize:
	$(RELEASE_PUBLISH) finalize $(RELEASE_PUBLISH_ARGS)

# Needs GH_TOKEN.
release-move-action-tag:
	./scripts/move-action-tag.sh $(RELEASE_VERSION)

# The VS Code extension bundles a server built from this checkout for the
# host platform, so each platform package is built on its own runner.
vscode-deps:
	cd $(VSCODE) && npm ci

vscode-bundle:
	cd $(VSCODE) && npm run bundle

vscode-test:
	cd $(VSCODE) && npm test

# Needs a display; on Linux run it under xvfb-run.
vscode-integration:
	cd $(VSCODE) && npm run test:integration

vscode-package:
	cd $(VSCODE) && npm run package:pre-release

# Checks the downloaded platform packages form one complete set built from
# REVISION, the commit being released.
REVISION ?= $(shell git rev-parse HEAD)
vscode-verify-packages:
	cd $(VSCODE) && python3 scripts/verify-vsix.py $(VSCODE_PACKAGES) --revision $(REVISION)

# Needs VSCE_PAT. Publishes only the targets the Marketplace lacks for this version.
vscode-publish:
	@test -n "$$VSCE_PAT" || { echo 'VSCE_PAT is not set; add the Marketplace token before publishing.'; exit 1; }
	cd $(VSCODE) && python3 scripts/publish-vsix.py $(VSCODE_PACKAGES)

help:
	@echo "Available targets:"
	@echo "  all     - Run lint, build, and test"
	@echo "  build   - Build release binary"
	@echo "  test    - Run tests"
	@echo "  lint    - Run clippy linter"
	@echo "  fmt     - Format code"
	@echo "  fmt-check - Verify Rust formatting without changing files"
	@echo "  msrv-check - Verify compatibility with Rust $(MSRV)"
	@echo "  dependency-check - Run make ci on a tree with uncommitted dependency updates"
	@echo "  ci      - Run every CI job: ci-quality ci-test ci-compat msrv-check ci-package"
	@echo "  ci-tools - Install the pinned pre-commit and maturin into $(CI_VENV)"
	@echo "  clean   - Clean build artifacts"
	@echo "  install - Install binary to $(INSTALL_PREFIX)/bin"
	@echo "  run     - Run rumk on this Makefile"
	@echo "  check-examples - Check example Makefiles"
	@echo "  check-gnu-fixtures - Verify parser fixtures with GNU Make"
	@echo "  check-corpus - Verify production-style Makefile projects"
	@echo "  check-comparison - Verify linter comparison and formatting regressions"
	@echo "  check-semantic - Verify authored defects and safe fixes against GNU Make"
	@echo "  benchmark - Measure include graphs, large files, and bulk fixes"
	@echo "  fuzz    - Fuzz every target for FUZZ_TIME seconds (needs nightly)"
	@echo "  check-fuzz - Type-check the fuzz targets against the library"
	@echo "  release-check - Run every local release gate and package dry run"
	@echo "  release-build - Build, smoke-test, and package RELEASE_TARGET into $(RELEASE_DIST)"
	@echo "  release-sdist - Build the Python source distribution into $(RELEASE_DIST)"
	@echo "  release-checksums - Write and verify SHA256SUMS for $(RELEASE_DIST)"
	@echo "  vscode-deps - Install the VS Code extension dependencies"
	@echo "  vscode-bundle - Build the server bundled into the extension"
	@echo "  vscode-test - Type-check and unit-test the extension"
	@echo "  vscode-integration - Run the extension in VS Code against the bundled server"
	@echo "  vscode-package - Package the extension for this platform"
	@echo "  vscode-verify-packages - Verify the complete set of platform packages"
	@echo "  vscode-publish - Publish missing platform packages to the Marketplace"
