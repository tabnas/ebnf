/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The translation parts: what the manifest names and what the package
// carries are the same files. The tree a host translates is the grammar's
// rules in source order (schema grammar-rules), streamed by the transducer
// part from the rule passes of make(); the compiled GrammarSpec is the
// route of `translate.spec`, with its own render.

const Assert = require('node:assert/strict')
const Fs = require('node:fs')
const Os = require('node:os')
const Path = require('node:path')
const { spawnSync } = require('node:child_process')
const { test } = require('node:test')

const { compileSpec } = require('@tabnas/bnf')
const { translate, ebnfConvert } = require('..')

const ROOT = Path.resolve(__dirname, '..', '..')
const read = (rel) => Fs.readFileSync(Path.join(ROOT, rel), 'utf8')
const spec = () => JSON.parse(translate().manifest).translate

// The parts a host sees are the package's own copies, written by
// `npm run embed`; they must be the repository's files.
test('translation parts expose the manifest, the part, the renders and their entries', () => {
  const parts = translate()
  Assert.ok(parts)
  Assert.equal(parts.manifest, read('tabnas.plugin.json'))
  Assert.equal(parts.lift, undefined)
  Assert.equal(parts.transduce?.entry, 'ebnf-transduce')
  Assert.equal(parts.transduce?.source, read('alchemy/transduce.alc'))
  Assert.equal(parts.render?.entry, 'ebnf-render')
  Assert.equal(parts.render?.source, read('alchemy/render.alc'))
  Assert.equal(parts.spec?.entry, 'ebnf-render-spec')
  Assert.equal(parts.spec?.source, read('alchemy/render-spec.alc'))
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

// The tree is the grammar's rules, an object, of the schema the three
// BNF-family notations share, streamed by the part, so nothing is read
// whole; the compiled spec's route is `spec`, of the schema grammar-spec.
test('the manifest names the grammar-rules schema, its part, and the grammar-spec route', () => {
  const t = spec()
  Assert.equal(JSON.parse(translate().manifest).languageId, 'ebnf')
  Assert.equal(t.reads, 'tree')
  Assert.equal(t.writes, 'tree')
  Assert.equal(t.root, 'object')
  Assert.equal(t.schema, 'grammar-rules')
  Assert.equal(t.transduce, 'alchemy/transduce.alc')
  Assert.equal(t.whole, undefined)
  Assert.equal(t.lift, undefined)
  Assert.equal(t.embed, undefined)
  Assert.equal(t.spec.schema, 'grammar-spec')
  Assert.equal(t.spec.render, 'alchemy/render-spec.alc')
  Assert.equal(t.spec.root, 'object')
})

// A host prints the loss lines as they are, so each is a sentence.
test('the loss is a list of sentences', () => {
  for (const loss of [spec().loss, spec().spec.loss]) {
    Assert.ok(Array.isArray(loss) && 0 < loss.length)
    for (const line of loss) {
      Assert.match(line, /^[A-Z].*\.$/, `${line} is not a sentence`)
    }
  }
})

// A host links a part with its own program and with other formats'
// parts, so every definition is named for EBNF and none is `export`, and
// the renders, which a host may link together, share no name.
test('the part and the renders are libraries named for ebnf', () => {
  const defs = (part) => part.source.split('\n')
    .filter((line) => line.startsWith('def '))
    .map((line) => line.slice(4).split(/\s/)[0])
  const parts = translate()
  for (const part of [parts.transduce, parts.render, parts.spec]) {
    const names = defs(part)
    Assert.ok(names.includes(part.entry), names.join(' '))
    for (const name of names) Assert.ok(name.startsWith('ebnf-'), `${name} is not named for ebnf`)
  }
  const render = new Set(defs(parts.render))
  for (const name of defs(parts.spec)) Assert.ok(!render.has(name), `${name} is defined twice`)
})

// What a host that takes the grammar-spec route reads an EBNF document
// as: the pure-data GrammarSpec of
// `compileSpec(ebnfConvert(src, { builtins: true }), { recognition:
// false, strict: true })`, valid JSON whose root is an object holding the
// rules and the options.
test('the grammar-spec route reads a document as the strict pure-data grammar spec', () => {
  const text = compileSpec(ebnfConvert('Doc ::= Greet\nGreet ::= "hi" | "hello"', { builtins: true }),
    { recognition: false, strict: true })
  const tree = JSON.parse(text)
  Assert.equal(typeof tree, 'object')
  Assert.deepEqual(Object.keys(tree.rule), ['Doc', 'Greet', '__start__'])
  Assert.equal(tree.options.rule.start, '__start__')
  Assert.equal(tree.meta.provenance.__start__, 'Doc')
})

// A published package is `ts/` alone, unpacked as `package/`: its build
// runs the embed script, so the package carries it, and the script keeps
// the generated `src/translate.ts` where the files it reads, above `ts/`,
// are not there.
test('a published package rebuilds without the files the embed reads', () => {
  const pkg = JSON.parse(Fs.readFileSync(Path.join(ROOT, 'ts', 'package.json'), 'utf8'))
  Assert.match(pkg.scripts.build, /embed-translate\.js/)
  Assert.ok(pkg.files.includes('embed-translate.js'),
    'the build runs embed-translate.js, which ts/package.json does not publish')
  const dir = Fs.mkdtempSync(Path.join(Os.tmpdir(), 'ebnf-package-'))
  try {
    const unpacked = Path.join(dir, 'package')
    Fs.mkdirSync(Path.join(unpacked, 'src'), { recursive: true })
    Fs.copyFileSync(Path.join(ROOT, 'ts', 'embed-translate.js'),
      Path.join(unpacked, 'embed-translate.js'))
    const published = Fs.readFileSync(Path.join(ROOT, 'ts', 'src', 'translate.ts'), 'utf8')
    Fs.writeFileSync(Path.join(unpacked, 'src', 'translate.ts'), published)
    const run = spawnSync(process.execPath, ['embed-translate.js'],
      { cwd: unpacked, encoding: 'utf8' })
    Assert.equal(run.status, 0, run.stderr)
    Assert.equal(Fs.readFileSync(Path.join(unpacked, 'src', 'translate.ts'), 'utf8'), published)
    Assert.deepEqual(Fs.readdirSync(dir), ['package'])
  } finally {
    Fs.rmSync(dir, { recursive: true, force: true })
  }
})
