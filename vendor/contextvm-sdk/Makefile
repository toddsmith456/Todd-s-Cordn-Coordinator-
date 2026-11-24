# Makefile for releasing the ContextVM Rust SDK to crates.io.
#
# Only `contextvm-sdk` (this crate) is published to crates.io. The
# `contextvm-ffi` sibling crate ships as prebuilt native libraries via the
# `ffi.yml` workflow + GitHub Releases, so it is intentionally NOT published
# here — see contextvm-ffi/ and AGENTS.md.
#
# Usage:
#   make help          - show available targets
#   make check         - run the full CI quality gate (must pass before publish)
#   make release-dry   - quality gate + crates.io packaging dry run (no upload)
#   make release       - quality gate + upload contextvm-sdk to crates.io
#
# Release procedure (manual steps outside this file):
#   1. bump `version` in Cargo.toml and add a dated CHANGELOG.md entry
#   2. open a PR, land it on main (CI's quality gate + FFI packaging must pass)
#   3. push a `v*` tag (triggers ffi.yml to assemble native artifacts)
#   4. run `make release` from a clean checkout of that commit to publish
#
# Note: the MSRV (1.88) check is not run here because it needs a pinned
# toolchain and regenerates Cargo.lock; CI's `msrv` job covers it. Run it
# manually with `rustup run 1.88 cargo check --all-features` if needed.

# Configuration
CARGO ?= cargo
# The crate published to crates.io.
PUBLISH_CRATE ?= contextvm-sdk

# Version of the publish crate, read from its Cargo.toml (root package).
VERSION ?= $(shell sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)

# The crate(s) covered by the workspace-wide quality gate.
ALL ?= --all
ALL_FEATURES ?= --all-features

.PHONY: help fmt-check clippy check-all test test-no-default doc \
        examples check publish-dry release-dry publish release clean

help:
	@echo "ContextVM Rust SDK - release Makefile"
	@echo ""
	@echo "Targets:"
	@echo "  make help          - show this help message"
	@echo "  make check         - run the full CI quality gate"
	@echo "  make release-dry   - quality gate + cargo publish --dry-run (no upload)"
	@echo "  make release       - quality gate + upload contextvm-sdk to crates.io"
	@echo ""
	@echo "Quality-gate sub-targets (composed by 'check'):"
	@echo "  make fmt-check     - cargo fmt --all -- --check"
	@echo "  make clippy        - cargo clippy --all --all-features -- -D warnings"
	@echo "  make check-all     - cargo check --all --all-features"
	@echo "  make test          - cargo test --all --all-features"
	@echo "  make test-no-default - cargo test --no-default-features"
	@echo "  make doc           - cargo doc --no-deps --all-features"
	@echo "  make examples      - run the rmcp integration example (local, offline)"
	@echo ""
	@echo "Publish sub-targets:"
	@echo "  make publish-dry   - cargo publish --dry-run -p contextvm-sdk"
	@echo "  make publish       - cargo publish -p contextvm-sdk (uploads to crates.io)"
	@echo ""
	@echo "Other:"
	@echo "  make clean         - cargo clean"
	@echo ""
	@echo "See the header of this file for the full release procedure."

# ---------------------------------------------------------------------------
# Quality gate (mirrors .github/workflows/ci.yml, minus the MSRV/pinned-toolchain job)
# ---------------------------------------------------------------------------

fmt-check:
	@echo "==> fmt --check"
	$(CARGO) fmt --all -- --check

clippy:
	@echo "==> clippy (all features, -D warnings)"
	$(CARGO) clippy $(ALL) $(ALL_FEATURES) --all-targets -- -D warnings

check-all:
	@echo "==> check (all features)"
	$(CARGO) check $(ALL) $(ALL_FEATURES)

test:
	@echo "==> test (all features)"
	$(CARGO) test $(ALL) $(ALL_FEATURES)

test-no-default:
	@echo "==> test (no default features)"
	$(CARGO) test --no-default-features

doc:
	@echo "==> doc (all features)"
	$(CARGO) doc --no-deps $(ALL_FEATURES)

examples:
	@echo "==> rmcp integration example (local)"
	$(CARGO) run --example rmcp_integration_test --features rmcp -- local

## Full pre-merge / pre-publish quality gate. Mirrors ci.yml's lint + test jobs.
check: fmt-check clippy check-all test test-no-default doc examples
	@echo ""
	@echo "All quality-gate checks passed."

# ---------------------------------------------------------------------------
# Publish to crates.io (contextvm-sdk only)
# ---------------------------------------------------------------------------

## Verify packaging without uploading: builds the package in an isolated dir,
## compiles it against crates.io dependencies, and runs the packaging checks.
## `--allow-dirty` so you can validate a staged-but-uncommitted version bump
## before committing; the real `publish` target stays strict.
publish-dry:
	@echo "==> cargo publish --dry-run --allow-dirty -p $(PUBLISH_CRATE)"
	$(CARGO) publish --dry-run --allow-dirty -p $(PUBLISH_CRATE)

## Dry run of the whole release: quality gate + packaging dry run.
release-dry: check publish-dry
	@echo ""
	@echo "Dry run complete. Nothing was uploaded."

## Upload contextvm-sdk to crates.io. Runs the quality gate first as a safety net.
publish:
	@echo "==> cargo publish -p $(PUBLISH_CRATE) (uploading to crates.io)"
	$(CARGO) publish -p $(PUBLISH_CRATE)

## Full release: quality gate + upload to crates.io.
release: check publish
	@echo ""
	@echo "Released $(PUBLISH_CRATE) $(VERSION) to crates.io."
	@echo "Tag and push (if not already): git tag v$(VERSION) && git push origin v$(VERSION)"

clean:
	$(CARGO) clean
