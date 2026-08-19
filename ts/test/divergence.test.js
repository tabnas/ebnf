/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

/* divergence.test.js — the cases where this port and the Go one were found
 * to disagree, now pinned as ALIGNED.
 *
 * `go/divergence_test.go` is the other half, and each names the other by
 * exact test name so a grep from either side lands on a real test.
 *
 * WHY THIS FILE EXISTS. `go/doc/concepts.md` claimed:
 *
 *   "a raw control character inside a double-quoted string reports
 *    `unprintable` in TypeScript and `unterminated_string` in Go. Both
 *    report the failure at the same row/column; only the Code differs."
 *
 * Those Go docs were copied wholesale from the zon repo — the same section
 * says "ZON integers like 42" and "Bare { is not a ZON opener" — and the
 * parity claim came with them. Measured 2026-08-19, both halves were false:
 *
 *   g = "a<0x01>b" ;   TS rejected (unprintable, 1:7)   Go ACCEPTED
 *   g = "abc ;         TS 1:5                           Go 1:11
 *
 * Not a code difference at all: an accept/reject split, plus a position
 * difference on the input where both DID reject.
 *
 * The first draft pinned those as divergences. That was wrong — AGENTS.md
 * rule 1 is "ts/ is canonical, go/ will track it", and both were repairable
 * in Go, whose scanner is hand-written where this port delegates to the
 * shared engine lexer. Recording a divergence you can fix turns a defect
 * into a CI requirement. Go was aligned instead; these pin the agreement.
 */

const { describe, it } = require('node:test')
const assert = require('node:assert')

const { ebnfConvert } = require('../dist/ebnf.js')

// Built rather than written: a literal control character in a source file is
// invisible and survives an edit badly.
const CTRL = String.fromCharCode(1)
const DEL = String.fromCharCode(127)

const failure = (src) => {
  try {
    ebnfConvert(src)
    return null
  }
  catch (err) {
    return String(err.message)
  }
}


describe('divergence', () => {

  // Go mirror: TestAlignedControlCharInStringIsRejected.
  it('a control character in a string is rejected in both ports', () => {
    const msg = failure('g = "a' + CTRL + 'b" ;')

    assert.ok(msg, 'control character accepted here')
    assert.match(msg, /unprintable/, 'rejected, but not as unprintable: ' + msg)
    assert.match(msg, /line 1, column 7/,
      'reported at a different column: ' + msg +
      '\n  Go also reports column 7 — the character\'s own position.')
  })

  // Go mirror: TestAlignedStringBodyBoundary.
  //
  // The CONTROL. Without it, "reject control characters" could tighten into
  // "reject anything unusual" and stay green.
  it('space and DEL remain legal string body in both ports', () => {
    assert.equal(failure('g = "a b" ;'), null, 'space must stay legal')
    assert.equal(failure('g = "a' + DEL + 'b" ;'), null, 'DEL must stay legal')
  })

  // Go mirror: TestAlignedUnterminatedStringColumn.
  it('an unterminated string reports the opening quote in both ports', () => {
    const msg = failure('g = "abc ;')

    assert.ok(msg, 'unterminated string is no longer an error at all')
    assert.match(msg, /line 1, column 5/,
      'reported at a different column: ' + msg +
      '\n  Go now reports column 5 too; it used to report 11, which is EOF.')
  })

  // Go mirror: TestAlignedSyntaxErrorColumn.
  //
  // The third case measured that day, and the one that already agreed. Kept
  // so a wholesale position change cannot hide inside the two repaired ones.
  it('a syntax error reports the same column in both ports', () => {
    const msg = failure('g = ;')

    assert.ok(msg, 'empty alternative is no longer an error at all')
    assert.match(msg, /line 1, column 5/, 'syntax error column moved: ' + msg)
  })
})
