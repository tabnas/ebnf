// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package ebnf

// divergence_test.go — the places this port and the TypeScript one DISAGREE,
// asserted rather than described.
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
// Both halves of that are false here, and the section it sits in describes
// ZON — these Go docs were copied wholesale from the zon repo, down to
// "ZON integers like 42" and "Bare { is not a ZON opener". The parity claim
// came with them.
//
// Measured 2026-08-19:
//
//   g = "a<0x01>b" ;   TS rejects (unprintable, 1:7)   Go ACCEPTS
//   g = "abc ;         TS 1:5                          Go 1:11
//
// So the control character is not a code difference at all — it is an
// accept/reject split, which is a strictly worse thing to have been
// describing as harmless. And the positions differ on the case where both
// DO reject.
//
// A prose claim cannot fail. These can.

import (
	"strings"
	"testing"
)

// TestDivergenceControlCharInStringIsAccepted pins: a raw control character
// inside a double-quoted EBNF string literal is ACCEPTED by this port and
// REJECTED by TypeScript.
//
// Not a code difference — an accept/reject split. If this port ever grows
// the rejection, this test fails, and the divergence entry it exists for
// must go with it. That is the intended signal, not a nuisance.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'a control character in a string is rejected here and accepted in Go'.
func TestDivergenceControlCharInStringIsAccepted(t *testing.T) {
	src := "g = \"a" + string(rune(1)) + "b\" ;"

	if _, err := Ebnf(src, nil); nil != err {
		t.Errorf("control character now REJECTED: %v\n"+
			"  If this port was repaired to match TypeScript, delete this "+
			"test and its TypeScript mirror — the divergence is closed.", err)
	}
}

// TestDivergenceUnterminatedStringColumn pins the COLUMN, which is where
// the two ports differ on an input both reject.
//
// The doc this replaces said "Both report the failure at the same
// row/column". They do not: TypeScript reports 1:5, this port 1:11. The
// codes agree; the position does not, which is the reverse of what was
// written.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'an unterminated string reports a different column in each port'.
func TestDivergenceUnterminatedStringColumn(t *testing.T) {
	_, err := Ebnf("g = \"abc ;", nil)
	if nil == err {
		t.Fatal("unterminated string is no longer an error at all")
	}

	if !strings.Contains(err.Error(), "column 11") {
		t.Errorf("unterminated string reported at a different column: %v\n"+
			"  TypeScript reports column 5. If both now agree, delete this "+
			"test and its TypeScript mirror.", err)
	}
}

// TestDivergenceSyntaxErrorColumnAgrees is the CONTROL.
//
// Without it, the two tests above pin only that the ports disagree, and a
// change that made every position disagree would look like more of the
// same. This one fails if the ports stop agreeing where they currently do.
//
// TypeScript mirror: ts/test/divergence.test.js
// 'a syntax error reports the same column in both ports'.
func TestDivergenceSyntaxErrorColumnAgrees(t *testing.T) {
	_, err := Ebnf("g = ;", nil)
	if nil == err {
		t.Fatal("empty alternative is no longer an error at all")
	}

	if !strings.Contains(err.Error(), "column 5") {
		t.Errorf("syntax error column moved: %v\n"+
			"  TypeScript also reports column 5; this control exists so a "+
			"wholesale position change cannot hide inside the divergences "+
			"above.", err)
	}
}
