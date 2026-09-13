// Copyright (c) 2026 tabnas, MIT License

// The code in go/doc/*.md and go/README.md, run. A documented example
// that no longer compiles is a defect in the documentation, and the
// prose gate cannot see it: Vale and ts/test/docs.test.js both strip
// fenced blocks before they look.
package ebnf_test

import (
	"errors"
	"strings"
	"testing"

	bnf "github.com/tabnas/bnf/go"
	ebnf "github.com/tabnas/ebnf/go"
	tabnas "github.com/tabnas/parser/go"
)

const docGrammar = `
  Expr   ::= Term ( ( "+" | "-" ) Term )*
  Term   ::= Factor ( ( "*" | "/" ) Factor )*
  Factor ::= NR | "(" Expr ")"
`

// README.md and tutorial.md steps 2 and 3.
func TestDocInstallAndParse(t *testing.T) {
	j := tabnas.Make()
	spec, err := ebnf.Install(j, docGrammar, &ebnf.ConvertOptions{Start: "Expr"})
	if err != nil {
		t.Fatalf("install: %v", err)
	}
	if 51 != len(spec.Rule) {
		t.Errorf("the pages say three productions give 51 rules, got %d",
			len(spec.Rule))
	}
	out, err := j.Parse("1 + 2 * 3")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	m, ok := out.(map[string]any)
	if !ok {
		t.Fatalf("want map[string]any, got %T", out)
	}
	if "Expr" != m["rule"] || "1+2*3" != m["src"] {
		t.Fatalf("want Expr / 1+2*3, got %v / %v", m["rule"], m["src"])
	}
	// The tutorial says the multiplication bound tighter: the single
	// child is a Term covering 2*3.
	kids, _ := m["kids"].([]any)
	if 1 != len(kids) {
		t.Fatalf("want one child, got %d", len(kids))
	}
	child := kids[0].(map[string]any)
	if "Term" != child["rule"] || "2*3" != child["src"] {
		t.Errorf("want Term / 2*3, got %v / %v", child["rule"], child["src"])
	}
}

// tutorial.md step 5 and guide.md: the two names for one conversion.
func TestDocToSpecAndEbnf(t *testing.T) {
	a, err := ebnf.ToSpec(docGrammar, &ebnf.ConvertOptions{Start: "Expr"})
	if err != nil {
		t.Fatalf("ToSpec: %v", err)
	}
	b, err := ebnf.Ebnf(docGrammar, &ebnf.ConvertOptions{Start: "Expr"})
	if err != nil {
		t.Fatalf("Ebnf: %v", err)
	}
	if len(a.Rule) != len(b.Rule) {
		t.Errorf("the pages say these are the same function: %d vs %d rules",
			len(a.Rule), len(b.Rule))
	}
}

// guide.md and reference.md: nil options are accepted everywhere.
func TestDocNilOptions(t *testing.T) {
	if _, err := ebnf.ToSpec(`A ::= "x"`, nil); err != nil {
		t.Errorf("ToSpec with nil options: %v", err)
	}
	j := tabnas.Make()
	if _, err := ebnf.Install(j, `A ::= "x"`, nil); err != nil {
		t.Errorf("Install with nil options: %v", err)
	}
	if _, err := j.Parse("x"); err != nil {
		t.Errorf("parse: %v", err)
	}
}

// guide.md: the IR boundary, and the aliases.
func TestDocParseEbnfIR(t *testing.T) {
	grammar, err := ebnf.ParseEbnf(docGrammar)
	if err != nil {
		t.Fatalf("ParseEbnf: %v", err)
	}
	names := []string{}
	for _, p := range grammar.Productions {
		names = append(names, p.Name)
	}
	want := "Expr Term Factor"
	if want != strings.Join(names, " ") {
		t.Errorf("want %q, got %q", want, strings.Join(names, " "))
	}
	// The pages say the exported IR types are aliases, not copies.
	var g *ebnf.EbnfGrammar = grammar
	var p *ebnf.EbnfProduction = grammar.Productions[0]
	var s ebnf.EbnfSequence = p.Alts[0]
	var e *ebnf.EbnfElement = s[0]
	if nil == g || nil == p || nil == e {
		t.Error("the aliases do not line up with the compiler's types")
	}
	var direct *bnf.Grammar = g
	if nil == direct {
		t.Error("EbnfGrammar is not bnf.Grammar")
	}
}

// tutorial.md step 6 and guide.md: a refused ISO construct, with its
// position.
func TestDocRefusedIsoRepetition(t *testing.T) {
	_, err := ebnf.ToSpec(`A ::= { B }`, nil)
	if nil == err {
		t.Fatal("the pages say ISO bracket repetition is refused")
	}
	for _, want := range []string{
		"ISO 14977 bracket repetition", "line 1, column 7"} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("want %q in %q", want, err.Error())
		}
	}
	var pe *ebnf.ParseError
	if !errors.As(err, &pe) {
		t.Fatalf("want *ParseError, got %T", err)
	}
	if 1 != pe.Line || 7 != pe.Column {
		t.Errorf("want 1:7, got %d:%d", pe.Line, pe.Column)
	}
}

// reference.md: `[ A ]` is a character class, not an option, and not an
// error either. This is the claim the TypeScript README used to get
// wrong.
func TestDocIsoOptionIsACharacterClass(t *testing.T) {
	grammar, err := ebnf.ParseEbnf("A ::= [ B ] \"x\"\nB ::= \"b\"")
	if err != nil {
		t.Fatalf("the reference says this is accepted: %v", err)
	}
	first := grammar.Productions[0].Alts[0][0]
	if bnf.KindRegex != first.Kind {
		t.Errorf("want a regex (character class), got %v", first.Kind)
	}
}

// tutorial.md step 7 and guide.md: the two error types.
func TestDocErrorTypes(t *testing.T) {
	_, perr := ebnf.ToSpec(`A ::= B - C`, nil)
	var pe *ebnf.ParseError
	if !errors.As(perr, &pe) {
		t.Fatalf("want *ParseError, got %T", perr)
	}

	_, cerr := ebnf.ToSpec(`A ::= B`, nil)
	var ce *ebnf.CompileError
	if !errors.As(cerr, &ce) {
		t.Fatalf("want *CompileError, got %T", cerr)
	}
	if "ebnf: rule 'A' references unknown rule 'B'" != cerr.Error() {
		t.Errorf("got %q", cerr.Error())
	}
}

// reference.md quotes these three compiler diagnostics.
func TestDocCompilerDiagnostics(t *testing.T) {
	for src, want := range map[string]string{
		`A ::= B`:       "ebnf: rule 'A' references unknown rule 'B'",
		`A ::= A "x"`:   "ebnf: rule 'A' is purely left-recursive (no seed alternative); cannot eliminate",
		"A ::= \"x\"\nA ::= \"y\"": "ebnf: rule 'A' is defined more than once; EBNF has no incremental-alternatives operator",
	} {
		_, err := ebnf.ToSpec(src, nil)
		if nil == err || want != err.Error() {
			t.Errorf("for %q want %q, got %v", src, want, err)
		}
	}
}

// guide.md: left recursion compiles, and the rewrite is available alone.
func TestDocLeftRecursion(t *testing.T) {
	if _, err := ebnf.ToSpec(
		"E ::= E \"+\" T | T\nT ::= NR", &ebnf.ConvertOptions{Start: "E"},
	); err != nil {
		t.Errorf("the guide says left recursion compiles: %v", err)
	}
	grammar, err := ebnf.ParseEbnf("E ::= E \"+\" T | T\nT ::= NR")
	if err != nil {
		t.Fatalf("ParseEbnf: %v", err)
	}
	if out := ebnf.EliminateLeftRecursion(grammar); out == grammar {
		t.Error("the guide says the pass returns a new grammar")
	}
}

// guide.md: the builtin lexer tokens need no definition.
func TestDocBuiltinTokens(t *testing.T) {
	j := tabnas.Make()
	if _, err := ebnf.Install(j, `A ::= NR`, &ebnf.ConvertOptions{Start: "A"}); err != nil {
		t.Fatalf("install: %v", err)
	}
	out, err := j.Parse("42")
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if m := out.(map[string]any); "42" != m["src"] {
		t.Errorf("want 42, got %v", m["src"])
	}
}

// guide.md: serialising through the shared compiler's helper.
func TestDocGoldenSerialise(t *testing.T) {
	spec, err := ebnf.ToSpec(docGrammar, &ebnf.ConvertOptions{Start: "Expr"})
	if err != nil {
		t.Fatalf("ToSpec: %v", err)
	}
	if "" == bnf.SpecToJSON(spec, 2) {
		t.Error("want JSON text")
	}
	if _, err := bnf.SpecToJSONErr(spec, 2); err != nil {
		t.Errorf("SpecToJSONErr: %v", err)
	}
}
