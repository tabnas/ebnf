# Build and test both implementations. ts/ is canonical; go/ is a port of it.
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
        build-go test-go

all: build test

build: build-ts build-go

test: test-ts test-go

clean: clean-ts

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

# Publish the TypeScript package at its current package.json version.
publish-ts: test-ts
	cd ts && npm publish --access public

reset:
	cd ts && npm run reset
