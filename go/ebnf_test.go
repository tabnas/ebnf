// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The Go front-end parses EBNF by hand where the TypeScript one drives
// the engine over a rule table, so shape-for-shape internal parity is
// not available. What IS held in common is the accepted language: these
// cases mirror ts/test/ebnf.test.js and the supported/not-supported
// table in the README, which is the specification both answer to.
package ebnf

import (
	"encoding/json"
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
	// Patterns use RE2's `\x{…}` spelling. The TypeScript front-end
	// emits `\uXXXX` for the same class — the two runtimes agree on the
	// language, not on the source text, and the shared compiler converts
	// between the two spellings when serialising a spec across.
	cases := []struct{ src, pattern, flags string }{
		{`A ::= [a-z]`, `[\x{61}-\x{7a}]`, ""},
		// A hyphen that is not between two members is a literal member;
		// the W3C grammars rely on this for classes like `[-+]`.
		{`A ::= [-+]`, `[\x{2d}\x{2b}]`, ""},
		{`A ::= [a-]`, `[\x{61}\x{2d}]`, ""},
		{`A ::= [#x20-#x7E]`, `[\x{20}-\x{7e}]`, ""},
		{`A ::= [#x9#xA#xD]`, `[\x{9}\x{a}\x{d}]`, ""},
		{`A ::= [#x10000-#x10FFFF]`, `[\x{10000}-\x{10ffff}]`, "u"},
		// A negated class matches the COMPLEMENT of its members, which
		// always contains every astral code point — so it needs `u` even
		// with nothing astral written.
		{`A ::= [^<&]`, `[^\x{3c}\x{26}]`, "u"},
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

// ---- Source spans --------------------------------------------------
//
// The front-end records where each element and production came from, so
// a compile failure can carry a range and a tool can underline the
// offending text (bnf.SrcSpan). Mirrors ts/test/ebnf.test.js's
// `describe('source spans')`, over the same grammar.
//
// Every assertion below slices the ORIGINAL SOURCE with the span and
// compares the text. That is the only check worth making: an offset
// pair that is self-consistent but points at the wrong characters would
// satisfy any assertion about the numbers themselves.
//
// Note the units. These offsets and columns count BYTES, because that
// is what this scanner natively has; the TS front-end's count UTF-16
// code units. bnf.SrcSpan specifies exactly that, so the recomputation
// below is byte-based too — slicing a Go string is already byte-based,
// which is the point.

// The same grammar the TS spans suite uses, joined with "\n" and with
// no trailing newline, so the offsets line up construct for construct.
var spanSrc = strings.Join([]string{
	`doc ::= item`,
	`item ::= "hi" | ref | (alt | two)`,
	`ref ::= [a-z]`,
	`alt ::= #x41`,
	`two ::= "z"`,
}, "\n")

// spanText slices the source with a span — the check that matters.
func spanText(t *testing.T, sp *bnf.SrcSpan) string {
	t.Helper()
	if sp == nil {
		t.Fatal("expected a span, got none")
	}
	if sp.S < 0 || sp.E > len(spanSrc) || sp.S > sp.E {
		t.Fatalf("span %+v does not lie inside the %d-byte source",
			*sp, len(spanSrc))
	}
	return spanSrc[sp.S:sp.E]
}

func spanProd(t *testing.T, name string) *bnf.Production {
	t.Helper()
	for _, p := range mustParse(t, spanSrc).Productions {
		if p.Name == name {
			return p
		}
	}
	t.Fatalf("no production %q", name)
	return nil
}

func TestSpansAProductionWithItsName(t *testing.T) {
	// The name, not the body: that is what an outline entry and
	// go-to-definition want, and a body can run over many lines.
	p := spanProd(t, "item")
	if got := spanText(t, p.Sp); got != "item" {
		t.Errorf("production span = %q, want %q", got, "item")
	}
	// Row and column are 1-based, as this scanner reports them.
	if p.Sp.R != 2 {
		t.Errorf("row = %d, want 2", p.Sp.R)
	}
	if p.Sp.C != 1 {
		t.Errorf("column = %d, want 1", p.Sp.C)
	}
}

func TestSpansAStringTerminalIncludingItsQuotes(t *testing.T) {
	el := spanProd(t, "item").Alts[0][0]
	if got := spanText(t, el.Sp); got != `"hi"` {
		t.Errorf("term span = %q, want %q", got, `"hi"`)
	}
}

func TestSpansARuleReference(t *testing.T) {
	el := spanProd(t, "item").Alts[1][0]
	if el.Kind != bnf.KindRef {
		t.Fatalf("expected a ref, got %v", el.Kind)
	}
	if got := spanText(t, el.Sp); got != "ref" {
		t.Errorf("ref span = %q, want %q", got, "ref")
	}
}

func TestSpansAGroupFromOpenParenToCloseParen(t *testing.T) {
	group := spanProd(t, "item").Alts[2][0]
	if group.Kind != bnf.KindGroup {
		t.Fatalf("expected a group, got %v", group.Kind)
	}
	if got := spanText(t, group.Sp); got != "(alt | two)" {
		t.Errorf("group span = %q, want %q", got, "(alt | two)")
	}
	// ...and the elements inside carry their own, narrower spans.
	if got := spanText(t, group.Alts[0][0].Sp); got != "alt" {
		t.Errorf("first group member span = %q, want %q", got, "alt")
	}
	if got := spanText(t, group.Alts[1][0].Sp); got != "two" {
		t.Errorf("second group member span = %q, want %q", got, "two")
	}
}

func TestSpansACharacterClassAndAHexTerminal(t *testing.T) {
	if got := spanText(t, spanProd(t, "ref").Alts[0][0].Sp); got != "[a-z]" {
		t.Errorf("class span = %q, want %q", got, "[a-z]")
	}
	// The hex terminal spans what was WRITTEN, `#x41`, not the single
	// character "A" it denotes.
	if got := spanText(t, spanProd(t, "alt").Alts[0][0].Sp); got != "#x41" {
		t.Errorf("hex span = %q, want %q", got, "#x41")
	}
}

// walkSpanned visits every element under a set of alternatives.
func walkSpanned(alts []bnf.Sequence, fn func(*bnf.Element)) {
	var rec func(el *bnf.Element)
	rec = func(el *bnf.Element) {
		if el == nil {
			return
		}
		fn(el)
		rec(el.Inner)
		walkSpanned(el.Alts, fn)
	}
	for _, a := range alts {
		for _, el := range a {
			rec(el)
		}
	}
}

// rowColAt recomputes a 1-based row and column from a byte offset, the
// same units the scanner counts in.
func rowColAt(src string, off int) (row, col int) {
	before := src[:off]
	return strings.Count(before, "\n") + 1,
		off - (strings.LastIndex(before, "\n") + 1) + 1
}

func TestSpanRowAndColumnAgreeWithTheOffset(t *testing.T) {
	// A span whose offset and row/column disagree is worse than no span:
	// a consumer picking either one gets a different answer. This is the
	// test that catches the scanner's hand-maintained line/col counters
	// drifting away from its index — which is why it walks every element
	// as well as every production, where the TS suite need only check
	// productions (its positions come from engine tokens, not from
	// counters this file maintains).
	check := func(what string, sp *bnf.SrcSpan) {
		t.Helper()
		if sp == nil {
			t.Errorf("%s: expected a span", what)
			return
		}
		row, col := rowColAt(spanSrc, sp.S)
		if sp.R != row {
			t.Errorf("%s: row %d disagrees with offset %d (want %d)",
				what, sp.R, sp.S, row)
		}
		if sp.C != col {
			t.Errorf("%s: column %d disagrees with offset %d (want %d)",
				what, sp.C, sp.S, col)
		}
	}
	for _, p := range mustParse(t, spanSrc).Productions {
		check("rule "+p.Name, p.Sp)
		walkSpanned(p.Alts, func(el *bnf.Element) {
			// Postfix wrappers carry no span, by design — see parseElem.
			switch el.Kind {
			case bnf.KindOpt, bnf.KindStar, bnf.KindPlus, bnf.KindRep:
				return
			}
			check("an element of rule "+p.Name, el.Sp)
		})
	}
}

func TestSpansExcludeLeadingTrivia(t *testing.T) {
	// A risk this port has and the TS one does not. There, a span comes
	// off a token the lexer already trimmed; here it comes from a mark,
	// and the mark is only correct because parseAtom takes it AFTER
	// skipSpace. Move it one line earlier and every span silently grows
	// a comment on the front, while still slicing to *something*.
	src := "A ::= /* c */ \"x\" (* d *) B\nB ::= 'y'"
	g, err := ParseEbnf(src)
	if err != nil {
		t.Fatalf("ParseEbnf failed: %v", err)
	}
	want := []string{`"x"`, "B", "'y'"}
	var got []string
	for _, p := range g.Productions {
		walkSpanned(p.Alts, func(el *bnf.Element) {
			if el.Sp == nil {
				t.Fatalf("element %v has no span", el.Kind)
			}
			got = append(got, src[el.Sp.S:el.Sp.E])
		})
	}
	if len(got) != len(want) {
		t.Fatalf("got %d elements %q, want %d", len(got), got, len(want))
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("element %d span = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestSpansDoNotReachTheEmittedGrammar(t *testing.T) {
	// Spans are metadata: they must not change what the grammar
	// compiles to. The TS suite checks the serialised spec mentions no
	// "sp" key; Go's spec is a struct, so the sharper form of the same
	// question is available — emit twice, once from the spanned IR and
	// once from the same IR with every span stripped, and require the
	// two specs to be identical.
	spec, err := Ebnf(spanSrc, nil)
	if err != nil {
		t.Fatalf("Ebnf failed: %v", err)
	}
	spanned, err := json.Marshal(spec.Rule)
	if err != nil {
		t.Fatalf("marshalling the emitted rules failed: %v", err)
	}
	if strings.Contains(strings.ToLower(string(spanned)), `"sp"`) {
		t.Errorf("a span reached the emitted grammar: %s", spanned)
	}

	g := mustParse(t, spanSrc)
	stripSpans(g)
	bare, err := emitSafely(g, &ConvertOptions{Tag: "ebnf"})
	if err != nil {
		t.Fatalf("emitting the span-stripped grammar failed: %v", err)
	}
	stripped, err := json.Marshal(bare.Rule)
	if err != nil {
		t.Fatalf("marshalling the span-stripped rules failed: %v", err)
	}
	if string(spanned) != string(stripped) {
		t.Errorf("spans changed the emitted grammar\n with spans: %s\n"+
			" without:    %s", spanned, stripped)
	}
}

// stripSpans clears every span in a grammar, so a spanned parse can be
// compared against what the same grammar used to produce.
func stripSpans(g *bnf.Grammar) {
	for _, p := range g.Productions {
		p.Sp = nil
		walkSpanned(p.Alts, func(el *bnf.Element) { el.Sp = nil })
	}
}

func TestClassGrammarsActuallyCompile(t *testing.T) {
	// The bug this exists to catch: the front-end emitted JavaScript's
	// `\uXXXX` escape, which Go's RE2 refuses outright — so every
	// grammar with a character class failed at regexp.Compile. Nothing
	// caught it because no test had compiled a class grammar end to end.
	for _, src := range []string{
		`A ::= [a-z]+`,
		`A ::= [^<&]`,
		`A ::= [#x20-#x7E]*`,
		`A ::= [a-z] [0-9]`,
	} {
		if _, err := Ebnf(src, nil); err != nil {
			t.Errorf("%q should compile, got: %v", src, err)
		}
	}
}
