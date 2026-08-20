// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package ebnf

// divergence_test.go — the cases where this port and the TypeScript one were
// found to disagree, now pinned as ALIGNED.
//
// ts/test/divergence.test.js is the other half, and each names the other by
// exact test name so a grep from either side lands on a real test.
//
// WHY THIS FILE EXISTS. go/doc/concepts.md claimed:
//
//   "a raw control character inside a double-quoted string reports
//    `unprintable` in TypeScript and `unterminated_string` in Go. Both
//    report the failure at the same row/column; only the Code differs."
//
// These Go docs were copied wholesale from the zon repo — the same section
// says "ZON integers like 42" and "Bare { is not a ZON opener" — and the
// parity claim came with them. Measured 2026-08-19, both halves were false:
//
//   g = "a<0x01>b" ;   TS rejected (unprintable, 1:7)   Go ACCEPTED
//   g = "abc ;         TS 1:5                           Go 1:11
//
// So it was not a code difference at all. It was an accept/reject split,
// plus a position difference on the input where both DID reject.
//
// FIRST DRAFT OF THIS FILE PINNED THOSE AS DIVERGENCES. That was wrong, and
// review said so: AGENTS.md rule 1 is "ts/ is canonical, go/ will track it",
// and both were repairable HERE — Go's scanner is hand-written where
// TypeScript delegates to the shared engine lexer, which is why it accepted
// what the engine rejects. Recording a divergence you can fix turns a defect
// into a CI requirement.
//
// So Go was aligned instead, and what these tests now pin is the agreement:
//   - a control character below 0x20 is rejected, at its own column
//   - an unterminated string is reported at the OPENING QUOTE, not at EOF

import (
	"strings"
	"testing"
)

// TestAlignedControlCharInStringIsRejected pins that a raw control character
// inside a string literal is rejected by BOTH ports, at the character's own
// column.
//
// Go accepted it until 2026-08-19. TypeScript rejects it through the engine
// lexer as `unprintable`; this port now rejects it in its own scanner with
// the same boundary — below 0x20, so 0x20 (space) and 0x7F (DEL) stay legal
// string body, matching the canonical port exactly.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'a control character in a string is rejected in both ports'.
func TestAlignedControlCharInStringIsRejected(t *testing.T) {
	_, err := Ebnf("g = \"a"+string(rune(1))+"b\" ;", nil)
	if nil == err {
		t.Fatal("control character accepted again: TypeScript rejects it, " +
			"and this port accepting it is an accept/reject split")
	}
	if !strings.Contains(err.Error(), "column 7") {
		t.Errorf("reported at a different column: %v\n"+
			"  TypeScript reports column 7 — the character's own position.",
			err)
	}
}

// TestAlignedStringBodyBoundary is the CONTROL for the test above.
//
// Without it, "reject control characters" could tighten into "reject
// anything unusual" and stay green. Space and DEL are legal string body in
// TypeScript, so they must stay legal here.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'space and DEL remain legal string body in both ports'.
func TestAlignedStringBodyBoundary(t *testing.T) {
	for name, src := range map[string]string{
		"space": "g = \"a b\" ;",
		"del":   "g = \"a" + string(rune(127)) + "b\" ;",
	} {
		if _, err := Ebnf(src, nil); nil != err {
			t.Errorf("%s is legal string body in TypeScript, rejected "+
				"here: %v", name, err)
		}
	}
}

// TestAlignedUnterminatedStringColumn pins that an unterminated string is
// reported at the OPENING QUOTE in both ports.
//
// This port reported EOF — column 11 for `g = "abc ;`, where TypeScript says
// 5. The canonical port underlines what the author wrote wrong, not where
// the scanner gave up.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'an unterminated string reports the opening quote in both ports'.
func TestAlignedUnterminatedStringColumn(t *testing.T) {
	_, err := Ebnf("g = \"abc ;", nil)
	if nil == err {
		t.Fatal("unterminated string is no longer an error at all")
	}
	if !strings.Contains(err.Error(), "column 5") {
		t.Errorf("reported at a different column: %v\n"+
			"  TypeScript reports column 5 — the unclosed quote. Column 11 "+
			"is EOF, which is where this port used to point.", err)
	}
}

// TestAlignedSyntaxErrorColumn is the third case measured that day, and the
// one that already agreed. Kept so a wholesale position change cannot hide
// inside the two that were repaired.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'a syntax error reports the same column in both ports'.
func TestAlignedSyntaxErrorColumn(t *testing.T) {
	_, err := Ebnf("g = ;", nil)
	if nil == err {
		t.Fatal("empty alternative is no longer an error at all")
	}
	if !strings.Contains(err.Error(), "column 5") {
		t.Errorf("syntax error column moved: %v", err)
	}
}
