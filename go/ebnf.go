// Copyright (c) 2026 tabnas, MIT License

// Package ebnf is the Go port of @tabnas/ebnf: an EBNF front-end for the
// tabnas parsing engine. It parses EBNF text into the grammar IR that
// github.com/tabnas/bnf/go compiles, and parses nothing else.
//
//	EBNF text --ParseEbnf--> bnf.Grammar --bnf.EmitGrammarSpec--> GrammarSpec
//
// Ebnf does both arrows in one call; ToSpec is the same thing under the
// name the TypeScript package uses, and Install adds the compiled
// grammar to a tabnas instance. Use a fresh instance per grammar:
// installing applies lexer settings as well as rules, and those are
// instance-wide. ParseEbnf stops at the IR, for a caller that wants to
// inspect or rewrite it, and EliminateLeftRecursion exposes that pass.
// A failure is a *ParseError when the EBNF itself could not be read and
// a *CompileError when the shared compiler refused the IR; both carry
// the diagnostic the canonical implementation writes, restamped to the
// ebnf: prefix.
//
// WHICH EBNF. The dialect is W3C EBNF (the notation XML, XPath and
// XQuery publish their grammars in) plus the four ISO/IEC 14977
// spellings that cannot collide with it: =, the comma, the semicolon
// and (* ... *). This is a best-effort front-end, and ts/README.md's
// supported and not-supported table is the specification both this port
// and the canonical one answer to.
//
// The TypeScript implementation in ts/ stays canonical. Where the two
// disagree, TypeScript wins, and DIVERGENCE.md records what cannot be
// repaired yet. This package's front-end is a hand-written scanner
// where the canonical one is a tabnas rule table; parser_ebnf.go's own
// header states that divergence and what it costs.
package ebnf

// VERSION is this module's version. It MUST equal ts/package.json
// "version": the release orchestrator rewrites both, and the version
// test fails the build if they drift.
const VERSION = "0.1.7"
