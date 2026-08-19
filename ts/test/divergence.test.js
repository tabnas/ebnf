/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

/* divergence.test.js — the places this port and the Go one DISAGREE,
 * asserted rather than described.
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
 * Both halves of that are false, and the section it sits in describes ZON
 * — those Go docs were copied wholesale from the zon repo, down to "ZON
 * integers like 42" and "Bare { is not a ZON opener". The parity claim came
 * with them.
 *
 * Measured 2026-08-19:
 *
 *   g = "a<0x01>b" ;   TS rejects (unprintable, 1:7)   Go ACCEPTS
 *   g = "abc ;         TS 1:5                          Go 1:11
 *
 * So the control character is not a code difference at all — it is an
 * accept/reject split, which is a strictly worse thing to have been
 * describing as harmless. And the positions differ on the case where both
 * DO reject.
 *
 * A prose claim cannot fail. These can.
 */

const { describe, it } = require('node:test')
const assert = require('node:assert')

const { ebnfConvert } = require('../dist/ebnf.js')

// A raw control character, built rather than written: a literal one in a
// source file is invisible and survives an edit badly.
const CTRL = String.fromCharCode(1)

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

  // Go mirror: TestDivergenceControlCharInStringIsAccepted.
  it('a control character in a string is rejected here and accepted in Go',
    () => {
      const msg = failure('g = "a' + CTRL + 'b" ;')

      assert.ok(msg,
        'control character now ACCEPTED here. If this port was repaired to ' +
        'match Go, delete this test and its Go mirror — but note the ' +
        'direction: Go accepting it is the LOOSER behaviour, so aligning ' +
        'that way loses a rejection.')
      assert.match(msg, /unprintable/,
        'still rejected, but no longer as unprintable: ' + msg)
    })

  // Go mirror: TestDivergenceUnterminatedStringColumn.
  it('an unterminated string reports a different column in each port', () => {
    const msg = failure('g = "abc ;')

    assert.ok(msg, 'unterminated string is no longer an error at all')
    assert.match(msg, /line 1, column 5/,
      'unterminated string reported at a different column: ' + msg +
      '\n  Go reports column 11. If both now agree, delete this test and ' +
      'its Go mirror.')
  })

  // Go mirror: TestDivergenceSyntaxErrorColumnAgrees.
  //
  // The CONTROL. Without it the two tests above pin only that the ports
  // disagree, and a change that made every position disagree would look
  // like more of the same.
  it('a syntax error reports the same column in both ports', () => {
    const msg = failure('g = ;')

    assert.ok(msg, 'empty alternative is no longer an error at all')
    assert.match(msg, /line 1, column 5/,
      'syntax error column moved: ' + msg +
      '\n  Go also reports column 5; this control exists so a wholesale ' +
      'position change cannot hide inside the divergences above.')
  })
})
