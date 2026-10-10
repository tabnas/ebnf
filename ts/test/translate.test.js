/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The translation parts: what the manifest names and what the package
// carries are the same files, and the tree a host translates is the
// GrammarSpec the compiler writes.

const Assert = require('node:assert/strict')
const Fs = require('node:fs')
const Path = require('node:path')
const { test } = require('node:test')

const { compileSpec } = require('@tabnas/bnf')
const { translate, ebnfConvert } = require('..')

const ROOT = Path.resolve(__dirname, '..', '..')
const read = (rel) => Fs.readFileSync(Path.join(ROOT, rel), 'utf8')
const spec = () => JSON.parse(translate().manifest).translate

// The parts a host sees are the package's own copies, written by
// `npm run embed`; they must be the repository's files.
test('translation parts expose the manifest, the render and its entry', () => {
  const parts = translate()
  Assert.ok(parts)
  Assert.equal(parts.manifest, read('tabnas.plugin.json'))
  Assert.equal(parts.lift, undefined)
  Assert.equal(parts.render?.entry, 'ebnf-render')
  Assert.equal(parts.render?.source, read('alchemy/render.alc'))
})

// An embed takes a plain tree into a format's own schema. A grammar spec
// is no plain tree's shape, so the manifest names no embed and the
// package carries none; a manifest that named one would be held to its
// file here, as the render is above.
test('translation parts carry the embed the manifest names, and none where it names none', () => {
  const parts = translate()
  const named = JSON.parse(read('tabnas.plugin.json')).translate.embed
  if (null == named) {
    Assert.equal(parts.embed, undefined)
  } else {
    Assert.equal(parts.embed?.entry, 'ebnf-embed')
    Assert.equal(parts.embed?.source, read(named))
  }
})

// The tree is the grammar spec, an object, of the schema the three
// BNF-family notations share; reading a document is compiling it, which
// is the host's to do, so there is no lift.
test('the manifest names the grammar-spec schema, an object root and no lift', () => {
  const t = spec()
  Assert.equal(JSON.parse(translate().manifest).languageId, 'ebnf')
  Assert.equal(t.reads, 'tree')
  Assert.equal(t.writes, 'tree')
  Assert.equal(t.root, 'object')
  Assert.equal(t.schema, 'grammar-spec')
  Assert.equal(t.lift, undefined)
  Assert.equal(t.embed, undefined)
})

// A host prints the loss lines as they are, so each is a sentence.
test('the loss is a list of sentences', () => {
  const loss = spec().loss
  Assert.ok(Array.isArray(loss) && 0 < loss.length)
  for (const line of loss) {
    Assert.match(line, /^[A-Z].*\.$/, `${line} is not a sentence`)
  }
})

// A host links the render with its own program and with other formats'
// parts, so every definition is named for EBNF and none is `export`.
test('the render is a library named for ebnf', () => {
  const names = translate().render.source.split('\n')
    .filter((line) => line.startsWith('def '))
    .map((line) => line.slice(4).split(/\s/)[0])
  Assert.ok(names.includes('ebnf-render'), names.join(' '))
  for (const name of names) Assert.ok(name.startsWith('ebnf-'), `${name} is not named for ebnf`)
})

// What a host reads an EBNF document as: the pure-data GrammarSpec of
// `compileSpec(ebnfConvert(src, { builtins: true }), { recognition:
// false, strict: true })`, valid JSON whose root is an object holding the
// rules and the options.
test('a host reads a document as the strict pure-data grammar spec', () => {
  const text = compileSpec(ebnfConvert('Doc ::= Greet\nGreet ::= "hi" | "hello"', { builtins: true }),
    { recognition: false, strict: true })
  const tree = JSON.parse(text)
  Assert.equal(typeof tree, 'object')
  Assert.deepEqual(Object.keys(tree.rule), ['Doc', 'Greet', '__start__'])
  Assert.equal(tree.options.rule.start, '__start__')
  Assert.equal(tree.meta.provenance.__start__, 'Doc')
})
