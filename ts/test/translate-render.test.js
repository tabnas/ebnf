/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The render, run: a grammar spec written as W3C EBNF by the render the
// package carries (`translate().render`), through the `alchemy` command
// (tabnas/alchemy-cli), which this package does not depend on. Set
// TABNAS_ALCHEMY to the command's path to run these tests; without it
// they skip.
//
// Each spec is one the compiler writes, changed where a test says, so
// what the render reads is a tree a host can hand it. The render writes
// what W3C EBNF can say, and the text compiles back to the spec it was
// written from; the rest it refuses with TARGET_VALUE_UNREPRESENTABLE,
// naming what, and never writes a grammar that recognises another
// language.

const Assert = require('node:assert/strict')
const Fs = require('node:fs')
const Os = require('node:os')
const Path = require('node:path')
const { spawnSync } = require('node:child_process')
const { after, test } = require('node:test')

const { compileSpec } = require('@tabnas/bnf')
const { ebnfConvert, translate } = require('..')

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

const compileText = (src) =>
  compileSpec(ebnfConvert(src, { builtins: true }), { recognition: false, strict: true })
const compile = (src) => JSON.parse(compileText(src))

let dir = null
after(() => {
  if (dir) Fs.rmSync(dir, { recursive: true, force: true })
})

// The render over a spec: `{ text }` when it writes one, and the failure
// the command reports (`{ code, message }`) when it does not.
function render (spec) {
  if (null == dir) {
    dir = Fs.mkdtempSync(Path.join(Os.tmpdir(), 'ebnf-render-'))
    Fs.writeFileSync(Path.join(dir, 'render.alc'),
      translate().render.source + '\ndef export [input] (ebnf-render input)\n')
  }
  const input = Path.join(dir, 'spec.json')
  Fs.writeFileSync(input, JSON.stringify(spec, null, 2))
  const run = spawnSync(ALCHEMY, ['run', Path.join(dir, 'render.alc'), input],
    { encoding: 'utf8', timeout: 120000 })
  if (0 === run.status) return { text: run.stdout }
  try {
    return JSON.parse(run.stderr)
  } catch (e) {
    return { code: 'NOT_JSON', message: `${run.stderr} ${run.error ?? ''}` }
  }
}

function writes (src, text, change = (spec) => spec) {
  const out = render(change(compile(src)))
  Assert.equal(out.text, text, JSON.stringify(out))
  Assert.equal(compileText(out.text), compileText(src))
}

function refuses (src, change, what) {
  const out = render(change(compile(src)))
  Assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  Assert.match(out.message, what)
}

const KEYWORD = 'Start ::= "if" Word\nWord ::= [a-z]+\n'
const CLASS = '#RX___U0061__U007A'
const alt = (spec, rule = 'Start') => spec.rule[rule].open[0]

test('the render writes a compiled grammar back as the text it compiles from', { skip }, () => {
  writes(KEYWORD, KEYWORD)
})

// This front end reads whitespace between terminals, so a literal is
// written as one terminal or not at all: one that would take a run of
// them would match the literal with whitespace inside.
test('the render refuses a literal no one W3C EBNF terminal spells', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    delete spec.options.fixed.token['#IF']
    spec.options.match.token['#IF'] = '@~/^if/i'
    return spec
  }, /the case-insensitive literal "if" is no one W3C EBNF terminal: it would be written as \[iI\] \[fF\]/)
  refuses(KEYWORD, (spec) => {
    spec.options.fixed.token['#IF'] = 'i\tf'
    return spec
  }, /the literal "i\\tf" is no one W3C EBNF terminal: it would be written as "i" #x9 "f"/)
})

// The engine reads `s` as a string of token names or as a list of them,
// one place each; the two are the same sequence.
test('the render reads the list form of the tokens an alternate matches', { skip }, () => {
  writes(KEYWORD, KEYWORD, (spec) => {
    alt(spec).s = ['#IF']
    return spec
  })
})

test('the render refuses a set of tokens at one place', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    alt(spec).s = ['#IF ' + CLASS]
    return spec
  }, /rule Start has an alternate whose s holds, at one place, a set of tokens/)
})

test('the render refuses a rule name EBNF cannot spell, the empty name among them', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.rule[''] = spec.rule.Word
    return spec
  }, /the rule "" has a name EBNF cannot spell/)
})

// A class pattern is one class: `[`, its members, and `]` at the end.
test('the render refuses a pattern that only begins with a class', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.options.match.token[CLASS] = '@~/^[\\u0061-\\u007a]+/u'
    return spec
  }, /the token #RX___U0061__U007A is the pattern .*, which is not one class the render reads/)
})

test('the render refuses a class whose flags change what it matches', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.options.match.token[CLASS] = '@~/^[\\u0061-\\u007a]/i'
    return spec
  }, /whose flags \(i\) change what it matches/)
})

// A case-insensitive literal's pattern (ABNF's) escapes every character
// a pattern reads otherwise; one that does not is a pattern, not a
// literal.
test('the render refuses a case-insensitive pattern that is not an escaped literal', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.options.match.token[CLASS] = '@~/^if+/i'
    return spec
  }, /the token #RX___U0061__U007A is the pattern .*, which is not a literal the render reads/)
})

test('the render refuses the form that edits a rule already installed', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.rule.Word.open = { alts: spec.rule.Word.open, inject: { append: true } }
    return spec
  }, /rule Word gives its open alternates as alts and an inject/)
})

test('the render refuses an error generator and an alternate modifier', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    alt(spec).e = '@stop'
    return spec
  }, /rule Start has an alternate carrying e \(an error generator\)/)
  refuses(KEYWORD, (spec) => {
    alt(spec).h = '@change'
    return spec
  }, /rule Start has an alternate carrying h \(an alternate modifier\)/)
})

test('the render refuses a function reference where a rule or a count is due', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    alt(spec).p = '@choose'
    return spec
  }, /rule Start has an alternate whose p is the function reference @choose/)
  refuses(KEYWORD, (spec) => {
    alt(spec).b = '@back'
    return spec
  }, /rule Start has an alternate whose b is not a count/)
})

// A repetition's loop guards and counts with `c` and `n`; anywhere else
// they would make an alternate conditional.
test('the render refuses a condition or a counter outside a repetition\'s loop', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    alt(spec).c = { 'n.rep': 0 }
    return spec
  }, /rule Start carries a condition or a counter outside a repetition's loop/)
  refuses(KEYWORD, (spec) => {
    alt(spec).n = { rep: 1 }
    return spec
  }, /rule Start carries a condition or a counter outside a repetition's loop/)
})

// Go writes a spec's keys in name order and its match tokens' order in a
// list of its own, so the rules' order, which ranks the tokens and so
// decides which of two that both match wins, is gone.
test('the render refuses a spec that gives its match tokens\' order as a list', { skip }, () => {
  refuses(KEYWORD, (spec) => {
    spec.options.match.token['#RX___U0030__U0039'] = '@~/^[\\u0030-\\u0039]/'
    spec.options.match.tokenOrder = Object.keys(spec.options.match.token)
    return spec
  }, /tokenOrder/)
})
