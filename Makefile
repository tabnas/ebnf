# Build and test all three implementations. ts/ is canonical; go/ and
# rs/ are ports of it.
#
# Local build/test resolve the unpublished @tabnas siblings via
# node_modules symlinks, wired by scripts/link.sh in the sibling
# tabnas/admin checkout. The declared dependency is an ordinary `*`
# version range, so without those symlinks a plain `npm install` gets
# the published package instead (see AGENTS.md).
#
# CI has built and tested go/ since the port landed — the shared
# polyglot-ci.yml caller detects go/ and runs a Go job per platform. Only this
# Makefile had not caught up, so `make test` covered one of the two runtimes
# that CI gates.

.PHONY: all build test clean build-ts test-ts clean-ts publish-ts reset \
        build-go test-go \
        build-rs test-rs clean-rs version-rs \
        prose prose-counts

all: build test

build: build-ts build-go build-rs

test: test-ts test-go test-rs

clean: clean-ts clean-rs

# --- TypeScript (package in ts/) ---
build-ts:
	cd ts && npm run build

test-ts:
	cd ts && npm test

clean-ts:
	rm -rf ts/dist ts/dist-test

# --- Go (module in go/) ---
build-go:
	cd go && go build ./...

test-go:
	cd go && go test ./...

# --- Rust (crate in rs/) ---
#
# The engine and the shared compiler are PATH dependencies on sibling
# checkouts (tabnas/parser and tabnas/bnf), and tabnas/support is a
# dev-dependency; neither is published, so clone all three beside this
# repository first. `ci/rust/run.sh` is the full gate.
build-rs:
	cd rs && cargo build --all-targets

# `--all-targets` does NOT include doctests -- cargo documents the
# selector as "Test all targets (does not include doctests)" -- and
# rs/README.md is doctested, so both commands are needed.
test-rs:
	cd rs && cargo test --all-targets && cargo test --doc
	cd rs && cargo clippy --all-targets --all-features -- -D warnings

clean-rs:
	cd rs && cargo clean

# Set the Rust crate version: make version-rs V=x.y.z
#
# Rewrites the two sites in the crate and then runs cargo once so it
# refreshes rs/Cargo.lock, which ci/rust/run.sh holds to rs/Cargo.toml.
# The three other version sites (ts/package.json, ts/src/ebnf.ts,
# go/ebnf.go) are the release orchestrator's, and
# rs/tests/version_test.rs fails the build when any of the five drift.
version-rs:
	@test -n "$(V)" || (echo "Usage: make version-rs V=x.y.z" && exit 1)
	sed -i.bak 's/^version = ".*"/version = "$(V)"/' rs/Cargo.toml
	sed -i.bak 's/^pub const VERSION: &str = ".*";/pub const VERSION: \&str = "$(V)";/' rs/src/lib.rs
	rm -f rs/Cargo.toml.bak rs/src/lib.rs.bak
	cd rs && cargo metadata --format-version 1 --offline >/dev/null

# Publish the TypeScript package at its current package.json version.
publish-ts: test-ts
	cd ts && npm publish --access public

reset:
	cd ts && npm run reset

# The prose gate (see docs/STYLE-GUIDE.md). Vale over the reader-facing
# pages, at the levels set in .vale.ini, on the same file list
# ts/test/docs.test.js reads. Requires `vale` on PATH and one
# `vale sync`. Warnings are advisory, errors fail.
prose:
	vale --minAlertLevel=error $$(node ts/scripts/gated-docs.cjs)
	node ts/scripts/vale-counts.cjs

# Re-measure what .vale.ini and the style guide record, after
# a change to the pages or to the rules moves the numbers.
prose-counts:
	node ts/scripts/vale-counts.cjs --write
