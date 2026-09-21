# Mirrors the repository-root Makefile from inside ts/, so a contributor
# working in the package can still reach the other runtimes.
.PHONY: all build test clean reset build-rs test-rs clean-rs

all: build test

build:
	npm run build

test:
	npm test

clean:
	rm -rf dist dist-test

reset:
	npm run reset

# --- Rust (crate in ../rs) ---
build-rs:
	cd ../rs && cargo build --all-targets

test-rs:
	cd ../rs && cargo test --all-targets && cargo test --doc
	cd ../rs && cargo clippy --all-targets --all-features -- -D warnings

clean-rs:
	cd ../rs && cargo clean
