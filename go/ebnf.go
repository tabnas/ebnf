// Copyright (c) 2026 tabnas, MIT License

// Package ebnf is the Go port of @tabnas/ebnf: an EBNF front-end for the
// tabnas parsing engine. It parses EBNF text into the grammar IR that
// github.com/tabnas/bnf/go compiles.
//
// PORT STATUS: not yet implemented. The TypeScript implementation is
// canonical and lands first by design; this package currently exposes
// only VERSION so the module builds and the release tooling has
// something to check. The front-end — the meta-grammar, character-class
// and code-point decoders, and the named rejections for constructs the
// IR cannot express — is ported in a later change, mirroring
// ts/src/converter.ts.
//
// The dialect and its documented limits are described in the repository
// README; that document is the contract this port will be held to.
package ebnf

// VERSION is this module's version. It MUST equal ts/package.json
// "version": the release orchestrator rewrites both, and the version
// test fails the build if they drift.
const VERSION = "0.1.6"
