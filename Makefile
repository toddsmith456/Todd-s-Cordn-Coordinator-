# cordn-rs — release helper.
#
#   make patch | minor | major
#
# Bumps the parity version in the Rust workspace and Android app, refreshes
# both lockfiles, commits, tags vX.Y.Z, and pushes the tag — which triggers
# .github/workflows/release.yml to build and publish binaries.

VERSION := $(shell sed -nE 's/^version = "([0-9.]+)"/\1/p' Cargo.toml | head -1)

.PHONY: show verify-clean patch minor major

show:
	@echo "$(VERSION)"

verify-clean:
	@if [ -n "$$(git status --porcelain)" ]; then \
		echo "working tree not clean; commit or stash first" >&2; exit 1; \
	fi

patch minor major: verify-clean
	@set -e; \
	new=$$(./scripts/bump-version.sh "$(VERSION)" "$@"); \
	echo "release $(VERSION) → $$new"; \
	sed -i.bak -E 's/^(version = ")[0-9.]+/\1'"$$new"'/' Cargo.toml apps/cordn-coordinator/src-tauri/Cargo.toml; \
	sed -i.bak -E 's/^([[:space:]]*"version": ")[0-9.]+/\1'"$$new"'/' apps/cordn-coordinator/package.json apps/cordn-coordinator/src-tauri/tauri.conf.json; \
	rm -f Cargo.toml.bak apps/cordn-coordinator/src-tauri/Cargo.toml.bak apps/cordn-coordinator/package.json.bak apps/cordn-coordinator/src-tauri/tauri.conf.json.bak; \
	cargo check --quiet; \
	cargo metadata --quiet --manifest-path apps/cordn-coordinator/src-tauri/Cargo.toml --no-deps --format-version 1 >/dev/null; \
	git add Cargo.toml Cargo.lock apps/cordn-coordinator/package.json apps/cordn-coordinator/src-tauri/Cargo.toml apps/cordn-coordinator/src-tauri/Cargo.lock apps/cordn-coordinator/src-tauri/tauri.conf.json; \
	git commit -q -m "release v$$new"; \
	git tag "v$$new"; \
	git push origin "v$$new" HEAD; \
	echo "✓ pushed v$$new — release workflow triggered"
