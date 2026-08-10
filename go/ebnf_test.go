// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The Go front-end parses EBNF by hand where the TypeScript one drives
// the engine over a rule table, so shape-for-shape internal parity is
// not available. What IS held in common is the accepted language: these
// cases mirror ts/test/ebnf.test.js and the supported/not-supported
// table in the README, which is the specification both answer to.
package ebnf

import (
	"strings"
	"testing"

	bnf "github.com/tabnas/bnf/go"
)

func mustParse(t *testing.T, src string) *bnf.Grammar {
	t.Helper()
	g, err := ParseEbnf(src)
	if err != nil {
		t.Fatalf("ParseEbnf(%q) failed: %v", src, err)
	}
	return g
}

func TestParsesTermsRefsAndSequences(t *testing.T) {
	g := mustParse(t, `A ::= "x" B`+"\n"+`B ::= "y"`)
	if len(g.Productions) != 2 {
		t.Fatalf("expected 2 productions, got %d", len(g.Productions))
	}
	alt := g.Productions[0].Alts[0]
	if len(alt) != 2 {
		t.Fatalf("expected 2 elements, got %d", len(alt))
	}
	if alt[0].Kind != bnf.KindTerm || alt[0].Literal != "x" {
		t.Errorf("expected term \"x\", got %+v", alt[0])
	}
	// W3C literals are case-SENSITIVE — the opposite of RFC 5234.
	if !alt[0].CaseSensitive {
		t.Error("W3C literals must be case-sensitive")
	}
	if alt[1].Kind != bnf.KindRef || alt[1].Name != "B" {
		t.Errorf("expected ref B, got %+v", alt[1])
	}
}

func TestPostfixOperators(t *testing.T) {
	g := mustParse(t, `A ::= "x"? "y"* "z"+`)
	want := []bnf.ElemKind{bnf.KindOpt, bnf.KindStar, bnf.KindPlus}
	alt := g.Productions[0].Alts[0]
	for i, k := range want {
		if alt[i].Kind != k {
			t.Errorf("element %d: got %v, want %v", i, alt[i].Kind, k)
		}
	}
}

func TestStacksPostfixOutermostLast(t *testing.T) {
	g := mustParse(t, `A ::= ( "x" )*?`)
	el := g.Productions[0].Alts[0][0]
	if el.Kind != bnf.KindOpt || el.Inner.Kind != bnf.KindStar {
		t.Errorf("expected opt(star(...)), got %v(%v)", el.Kind, el.Inner.Kind)
	}
}

func TestGroupsAndAlternation(t *testing.T) {
	g := mustParse(t, `A ::= ( "x" | "y" ) "z"`)
	alt := g.Productions[0].Alts[0]
	if alt[0].Kind != bnf.KindGroup || len(alt[0].Alts) != 2 {
		t.Fatalf("expected a 2-branch group, got %+v", alt[0])
	}
	if alt[1].Kind != bnf.KindTerm {
		t.Errorf("expected a term after the group, got %v", alt[1].Kind)
	}
}

func TestCharacterClasses(t *testing.T) {
	// Patterns are emitted with \uXXXX escapes, byte-for-byte as the
	// TypeScript front-end emits them — that identity is the point of
	// the parity.
	cases := []struct{ src, pattern, flags string }{
		{`A ::= [a-z]`, `[\u0061-\u007a]`, ""},
		// A hyphen that is not between two members is a literal member;
		// the W3C grammars rely on this for classes like `[-+]`.
		{`A ::= [-+]`, `[\u002d\u002b]`, ""},
		{`A ::= [a-]`, `[\u0061\u002d]`, ""},
		{`A ::= [#x20-#x7E]`, `[\u0020-\u007e]`, ""},
		{`A ::= [#x9#xA#xD]`, `[\u0009\u000a\u000d]`, ""},
		// Above the BMP the escape and the flag both have to change.
		{`A ::= [#x10000-#x10FFFF]`, `[\u{10000}-\u{10ffff}]`, "u"},
		// A negated class matches the COMPLEMENT of its members, which
		// always contains every astral code point — so it needs `u` even
		// with nothing astral written.
		{`A ::= [^<&]`, `[^\u003c\u0026]`, "u"},
	}
	for _, c := range cases {
		g := mustParse(t, c.src)
		el := g.Productions[0].Alts[0][0]
		if el.Kind != bnf.KindRegex {
			t.Errorf("%s: expected a regex element, got %v", c.src, el.Kind)
			continue
		}
		if el.Pattern != c.pattern {
			t.Errorf("%s: pattern = %q, want %q", c.src, el.Pattern, c.pattern)
		}
		if el.Flags != c.flags {
			t.Errorf("%s: flags = %q, want %q", c.src, el.Flags, c.flags)
		}
	}
}

func TestStandaloneCodePoint(t *testing.T) {
	g := mustParse(t, `A ::= #x41`)
	el := g.Productions[0].Alts[0][0]
	if el.Kind != bnf.KindTerm || el.Literal != "A" {
		t.Errorf("expected term \"A\", got %+v", el)
	}
}

func TestBothQuoteStyles(t *testing.T) {
	g := mustParse(t, `A ::= 'a"b' | "c'd"`)
	if got := g.Productions[0].Alts[0][0].Literal; got != `a"b` {
		t.Errorf("got %q", got)
	}
	if got := g.Productions[0].Alts[1][0].Literal; got != "c'd" {
		t.Errorf("got %q", got)
	}
}

func TestIsoSpellings(t *testing.T) {
	// The four ISO 14977 spellings that cannot collide with the W3C
	// reading: `=`, `,`, `;`, `(* … *)`.
	g := mustParse(t, `A = "x" , "y" ; (* a comment *)`)
	if len(g.Productions[0].Alts[0]) != 2 {
		t.Errorf("expected two concatenated items, got %d",
			len(g.Productions[0].Alts[0]))
	}
}

func TestComments(t *testing.T) {
	g := mustParse(t, "/* leading */\nA ::= \"x\" /* trailing */\n")
	if len(g.Productions) != 1 {
		t.Errorf("expected comments to be dropped, got %d productions",
			len(g.Productions))
	}
}

// Every construct the dialect refuses, with the message that names it.
// This is the not-supported table, executed.
func TestNamedRejections(t *testing.T) {
	cases := []struct{ src, want string }{
		{`A ::= B - C`, "subtraction"},
		{`A ::= ? anything ?`, "special sequences"},
		{`A ::= { "x" }`, "bracket repetition"},
		{`A ::= *"x"`, "postfix"},
		{`A ::= ""`, "empty string literal"},
		{`A ::= []`, "empty character class"},
		{`A ::= [z-a]`, "reversed range"},
		{`A ::= #x110000`, "above the Unicode maximum"},
		{"A ::= \"x\"\nA ::= \"y\"", "defined more than once"},
		// Narrowings: each of these used to be accepted.
		{`A ::= | "x"`, "empty alternative"},
		{`A ::= "x" |`, "empty alternative"},
		{`A = ;`, "empty alternative"},
		{`A ::= ()`, "empty alternative"},
		{`A = , "x";`, "leading comma"},
		{`A ::= "x"? | "y"?`, "alternatives that each match nothing"},
		{`A ::= ("x"? | "y"?) "y"`, "a group in rule 'A'"},
	}
	for _, c := range cases {
		_, err := ParseEbnf(c.src)
		if err == nil {
			t.Errorf("%q was accepted; expected a %q error", c.src, c.want)
			continue
		}
		if !strings.Contains(err.Error(), c.want) {
			t.Errorf("%q: error was %q, expected it to mention %q",
				c.src, err.Error(), c.want)
		}
	}
}

// The other direction matters as much: a narrowing that also refuses
// legitimate grammars is a worse bug than the one it fixed.
func TestStillAccepted(t *testing.T) {
	for _, src := range []string{
		`A ::= ("x"? | "y") "z"`,
		`A = "x" , "y";`,
		`A ::= #x41`,
		`A ::= #xD7FF`,
		`A ::= #xd7ff`,
		`A ::= [#x20-#x7E]`,
		`A ::= "x" | "y"`,
		`A ::= "x"? "y"`,
		`A ::= "x"* "y"`,
	} {
		if _, err := ParseEbnf(src); err != nil {
			t.Errorf("%q should compile, got: %v", src, err)
		}
	}
}

func TestOnlyLowercaseHexSpelling(t *testing.T) {
	// W3C specifies `#x`; `#X41` is not among the ISO spellings this
	// package documents accepting, so it must not be case-folded.
	if _, err := ParseEbnf(`A ::= #X41`); err == nil {
		t.Error("#X41 should not be accepted")
	}
	if _, err := ParseEbnf(`A ::= [#X20-#X7E]`); err == nil {
		t.Error("[#X20-#X7E] should not be accepted")
	}
}

func TestEmitsSpec(t *testing.T) {
	spec, err := Ebnf(`Greet ::= "hi" | "hello"`, nil)
	if err != nil {
		t.Fatalf("Ebnf failed: %v", err)
	}
	if spec.Options.Rule.Start != "__start__" {
		t.Error("expected the synthetic __start__ wrapper")
	}
	if spec.Rule["Greet"] == nil {
		t.Error("expected a Greet rule")
	}
	// W3C literals are case-sensitive, so they lift to FIXED tokens
	// rather than case-folding regex matchers (the contrast with ABNF).
	if len(spec.Options.Fixed.Token) != 2 {
		t.Errorf("expected two fixed tokens, got %d",
			len(spec.Options.Fixed.Token))
	}
}

func TestDiagnosticsSayEbnf(t *testing.T) {
	// A user who wrote EBNF should never see the name of a package they
	// did not import.
	_, err := Ebnf(`A ::= B`, nil)
	if err == nil {
		t.Fatal("expected an unknown-rule error")
	}
	if !strings.HasPrefix(err.Error(), "ebnf: ") {
		t.Errorf("expected an ebnf-prefixed diagnostic, got %q", err.Error())
	}
}

func TestPurelyLeftRecursiveIsAnErrorNotAPanic(t *testing.T) {
	// The shared compiler panics here; this front-end contains that and
	// returns an error, which is the right contract for a Go library.
	_, err := Ebnf(`A ::= A "x"`, nil)
	if err == nil {
		t.Fatal("expected a purely left-recursive rule to be refused")
	}
	if !strings.Contains(err.Error(), "left-recursive") {
		t.Errorf("expected the diagnostic to name the problem, got %q",
			err.Error())
	}
}
