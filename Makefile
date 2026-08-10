# Build and test the TypeScript (ts/) implementation. A Go port (go/)
# will follow; ts/ is canonical.
#
# Local build/test resolve the unpublished @tabnas siblings via the
# repo-set node_modules symlinks (@tabnas/bnf is a file: dependency on
# ../../bnf/ts).

.PHONY: all build test clean build-ts test-ts clean-ts publish-ts reset

all: build test

build: build-ts

test: test-ts

clean: clean-ts

# --- TypeScript (package in ts/) ---
build-ts:
	cd ts && npm run build

test-ts:
	cd ts && npm test

clean-ts:
	rm -rf ts/dist ts/dist-test

# Publish the TypeScript package at its current package.json version.
publish-ts: test-ts
	cd ts && npm publish --access public

reset:
	cd ts && npm run reset
