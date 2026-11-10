/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The tree a host translates, `parseEbnfTree`: an EBNF document read as the
// schema grammar-rules (tabnas/bnf's docs/grammar-rules.md), its rules as
// the text writes them, uncompiled and in source order. `test/spec/tree.tsv`
// holds each grammar to its tree's JSON text, member order included, which
// the Go and Rust runners (`go/tree_test.go`, `rs/tests/tree_test.rs`) hold
// their ports to; the order is part of the contract, since a host streams
// the members in that order and the transducer part makes the same events
// from the rule passes of `make()`, the parser whose value the tree is.
//
// Repetition is replacement (AGENTS.md): the rules of a grammar, the
// alternatives of a rule or a group and the elements of an alternative add
// no rule depth however many there are.

const Assert = require('node:assert/strict')
const Fs = require('node:fs')
const Path = require('node:path')
const { test } = require('node:test')

const { checkRules } = require('@tabnas/bnf')

const { make, parseEbnfTree, ebnfTreeRules, ebnfRules } = require('..')

const ROOT = Path.resolve(__dirname, '..', '..')

// The fixture rows: a header, then a grammar and its expected cell to a
// line, a line starting with `#` a comment. A grammar cell's `\n`, `\r`,
// `\t` and `\\` are the characters they name, so a backslash the grammar
// holds is written doubled; the expected cell is the tree's JSON text, or
// ERROR:<code>.
function rows () {
  const file = Path.join(ROOT, 'test', 'spec', 'tree.tsv')
  const out = []
  Fs.readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
    if (0 === i || '' === line || line.startsWith('#')) return
    const [grammar, expected] = line.split('\t')
    out.push({
      line: i + 1,
      grammar: grammar.replace(/\\([nrt\\])/g, (_, c) => ({ n: '\n', r: '\r', t: '\t', '\\': '\\' })[c]),
      expected,
    })
  })
  return out
}

for (const row of rows()) {
  test(`tree.tsv:${row.line} ${JSON.stringify(row.grammar).slice(0, 60)}`, () => {
    if (row.expected.startsWith('ERROR:')) {
      Assert.throws(() => parseEbnfTree(row.grammar),
        (err) => err?.cause?.code === row.expected.slice('ERROR:'.length))
      return
    }
    const tree = parseEbnfTree(row.grammar)
    checkRules(tree)
    Assert.equal(JSON.stringify(tree), row.expected)
  })
}


// The grammars a test reads as trees: the fixtures', this repository's
// example grammars and the oracle's sources.
function sources () {
  const out = rows().map((row) => row.grammar)
  const grammars = Path.join(__dirname, 'grammar')
  for (const file of Fs.readdirSync(grammars).filter((f) => f.endsWith('.ebnf'))) {
    out.push(Fs.readFileSync(Path.join(grammars, file), 'utf8'))
  }
  const oracle = JSON.parse(Fs.readFileSync(Path.join(ROOT, 'rs', 'tests', 'oracle', 'ebnf-ir.json'), 'utf8'))
  for (const c of oracle) out.push(c.src)
  return out
}

// Every grammar of the fixtures and the corpora that the tree parser reads
// is a tree of the schema.
test('every tree the reader builds is a grammar-rules tree', () => {
  let read = 0
  for (const src of sources()) {
    let tree
    try { tree = parseEbnfTree(src) } catch { continue }
    checkRules(tree)
    read++
  }
  Assert.ok(60 < read, `${read} trees read`)
})


// The tree parser reads with the meta-grammar's own alternates, so its rule
// passes are the meta-grammar's: only the actions differ.
test('the tree parser has the meta-grammar\'s alternates', () => {
  // An installed table's alternates are normalized in place (`s` and `g`
  // become lists), so each is read back to its written form.
  const text = (v) => (Array.isArray(v) ? v.join(' ') : v) || undefined
  const shape = (table) => JSON.stringify(Object.fromEntries(
    Object.entries(table).map(([name, spec]) => [name, ['open', 'close'].map((state) =>
      (spec[state] ?? []).map((alt) => ({
        s: text(alt.s), p: alt.p ?? undefined, r: alt.r ?? undefined, b: alt.b ?? undefined,
        g: text(alt.g), a: null != alt.a, c: null != alt.c,
      })))])))
  Assert.equal(shape(ebnfTreeRules), shape(ebnfRules))
  // The Rust and Go tree parsers install the same table as a grammar
  // document, rs/src/parser_ebnf.rs's GRAMMAR_TEXT, which go/tree_test.go
  // holds go/tree.go's copy to: its alternates are these.
  const rust = Fs.readFileSync(Path.join(ROOT, 'rs', 'src', 'parser_ebnf.rs'), 'utf8')
  const document = /const GRAMMAR_TEXT: &str = r##"([\s\S]*?)"##;/.exec(rust)
  Assert.ok(document, 'rs/src/parser_ebnf.rs holds no GRAMMAR_TEXT')
  Assert.equal(shape(JSON.parse(document[1]).rule), shape(ebnfRules))
})


test('make() builds a fresh parser whose value is the tree', () => {
  const a = make()
  const b = make()
  Assert.notEqual(a, b)
  Assert.deepEqual(a.parse('x ::= "y"\n'), parseEbnfTree('x ::= "y"\n'))
})


// The deepest rule a parse of `src` reaches.
function depth (src) {
  const tn = make()
  let deepest = 0
  tn.sub({
    ruleDone: (rule, _ctx, done) => {
      if (!done.probe && rule.d > deepest) deepest = rule.d
    },
  })
  tn.parse(src)
  return deepest
}

const N = 10000
const repeated = [
  ['rules of a grammar', (n) => Array.from({ length: n }, (_, i) => `r${i} ::= "x"\n`).join('')],
  ['ISO rules of a grammar', (n) => Array.from({ length: n }, (_, i) => `r${i} = "x" ;\n`).join('')],
  ['alternatives of a rule', (n) => 'a ::= ' + Array.from({ length: n }, (_, i) => `r${i}`).join(' | ') + '\n'],
  ['elements of an alternative', (n) => 'a ::= ' + Array.from({ length: n }, (_, i) => `r${i}*`).join(' ') + '\n'],
  ['ISO elements of an alternative', (n) => 'a = ' + Array.from({ length: n }, (_, i) => `r${i}`).join(' , ') + ' ;\n'],
  ['alternatives of a group', (n) => 'a ::= ( ' + Array.from({ length: n }, (_, i) => `[${String.fromCharCode(97 + (i % 26))}]`).join(' | ') + ' )\n'],
  ['elements of a group', (n) => 'a ::= ( ' + '"x" '.repeat(n) + ')?\n'],
]

for (const [what, doc] of repeated) {
  // Two items, since a list's first item can sit apart from its repeated
  // ones: the loop's own item is the second.
  test(`the ${what} add no rule depth: ${N} reach what two do`, () => {
    Assert.equal(depth(doc(N)), depth(doc(2)), what)
  })
}
