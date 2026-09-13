// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package ebnf

// lookahead_test.go — the bounded-lookahead limit, pinned behaviourally.
//
// ts/test/ebnf.test.js, describe('the bounded-lookahead limit'), is the
// other half; each test below names its TypeScript counterpart by exact
// string so a grep from either side lands on a real test.
//
// Every other Go test in this package stops at the IR or the
// GrammarSpec. That is enough for what the front-end accepts, but the
// limit is not a property of the front-end: it is where the shared
// compiler's reach runs out, and that only shows when input is actually
// parsed. So these install the grammar on an engine and parse, which is
// what makes them worth having separately.
//
// AGENTS.md, "The dialect decision", requires that a change in the
// compiler's reach moves these tests and all four documents in the same
// change. Measured 2026-09-13: every case below agrees with TypeScript,
// rejections included.

import (
	"strings"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

// engine installs src on a fresh instance. Fresh per grammar, not
// shared: installing applies lexer settings as well as rules, and those
// are instance-wide.
func engine(t *testing.T, src string) *tabnas.Tabnas {
	t.Helper()
	j := tabnas.Make(tabnas.Options{})
	if _, err := Install(j, src, nil); err != nil {
		t.Fatalf("Install(%q) failed: %v", src, err)
	}
	return j
}

func accepts(t *testing.T, j *tabnas.Tabnas, input string) {
	t.Helper()
	if _, err := j.Parse(input); err != nil {
		t.Fatalf("Parse(%q): want accept, got %v", input, err)
	}
}

// rejectsUnexpected pins the reason, not just the refusal. A grammar
// that failed to compile, or one that refused every input, would
// satisfy a bare "it errored" assertion while proving nothing about
// lookahead.
func rejectsUnexpected(t *testing.T, j *tabnas.Tabnas, input string) {
	t.Helper()
	_, err := j.Parse(input)
	if err == nil {
		t.Fatalf("Parse(%q): want reject, got accept", input)
	}
	if !strings.Contains(err.Error(), "unexpected") {
		t.Fatalf("Parse(%q): want an `unexpected` rejection, got %v", input, err)
	}
}

// TS: 'an unbounded shared prefix is left-factored automatically'
func TestUnboundedSharedPrefixIsLeftFactored(t *testing.T) {
	j := engine(t, "S ::= L \"x\" | L \"y\"\nL ::= \"a\"+")
	accepts(t, j, "a x")
	accepts(t, j, "a y")
	accepts(t, j, "a a x")
	// The `y` branch needs a decision past an unbounded run of `a`.
	accepts(t, j, "a a y")
}

// TS: 'left-factoring by hand works the same'
func TestLeftFactoringByHandWorksTheSame(t *testing.T) {
	j := engine(t, "S ::= L ( \"x\" | \"y\" )\nL ::= \"a\"+")
	accepts(t, j, "a a a y")
}

// TS: 'a shared prefix behind distinct recursive rules is still the limit'
//
// Left factoring is structural, so two distinct rules spelling the same
// unbounded prefix cannot merge. The limit that leaves is asymmetric,
// and that asymmetry is the part worth pinning: A and B are the same
// shape, spelled the same way, so what separates them is only which
// alternative each sits behind.
func TestSharedPrefixBehindDistinctRecursiveRulesIsStillTheLimit(t *testing.T) {
	j := engine(t, "S ::= A \"x\" | B \"y\"\nA ::= \"a\" A | \"a\"\nB ::= \"a\" B | \"a\"")
	accepts(t, j, "a x")
	accepts(t, j, "a y")
	// The first alternative carries any depth.
	accepts(t, j, "a a x")
	accepts(t, j, "a a a a a x")
	// The second does not, at any depth past one.
	rejectsUnexpected(t, j, "a a y")
	rejectsUnexpected(t, j, "a a a a a y")
}
