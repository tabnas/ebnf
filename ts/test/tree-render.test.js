/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

// The render of the tree a host translates, run: a grammar-rules tree
// written as W3C EBNF by the render the package carries
// (`translate().render`, alchemy/render.alc), through the `dmog` command
// (tabnas/dmog), which this package does not depend on. Set
// TABNAS_ALCHEMY to the command's path to run these tests; without it
// they skip, as `translate-render.test.js` does.
//
// The contract is the round trip: the tree of an EBNF document, written,
// reads back as that tree, and writes as the same text again. A tree of
// another notation is written as EBNF spells it, and what EBNF cannot say
// is refused with TARGET_VALUE_UNREPRESENTABLE, naming the rule and what
// it met, as is a tree that is not the schema's.

const Assert = require('node:assert/strict')
const Fs = require('node:fs')
const Os = require('node:os')
const Path = require('node:path')
const { spawn } = require('node:child_process')
const { after, test } = require('node:test')

const { Tabnas } = require('@tabnas/parser')

const { ebnf: ebnfPlugin, parseEbnfTree, translate } = require('..')

const ALCHEMY = process.env.TABNAS_ALCHEMY
const skip = ALCHEMY ? false : 'TABNAS_ALCHEMY does not name the alchemy command'

// Every transient task reports progress at least every 30 seconds
// (AGENTS.md), so the command runs asynchronously, and while a run is out
// a line names it and the time it has taken, every BEAT_MS. A run takes
// a second or two; LIMIT_MS only stops a command that has hung.
const BEAT_MS = 25000
const LIMIT_MS = 120000

// The most a run may write to either stream: 1 MiB, the default maxBuffer
// spawnSync had. A command that floods its output fails its case rather
// than filling the runner's heap.
const MAX_OUTPUT = 1024 * 1024

let dir = null
let runs = 0
let current = null
let beat = null
after(() => {
  if (beat) clearInterval(beat)
  if (dir) Fs.rmSync(dir, { recursive: true, force: true })
})

// A text short enough to name a run by, on one line.
function brief (text) {
  const s = String(text).replace(/\s+/g, ' ').trim()
  return 48 < s.length ? s.slice(0, 47) + '…' : s
}

// The render over a tree: `{ text }` when it writes one, and the failure
// the command reports (`{ code, message }`) when it does not. `what` names
// the run in the progress line.
function render (what, tree) {
  if (null == dir) {
    dir = Fs.mkdtempSync(Path.join(Os.tmpdir(), 'ebnf-tree-render-'))
    Fs.writeFileSync(Path.join(dir, 'render.alc'),
      translate().render.source + '\ndef export [input] (ebnf-render input)\n')
  }
  if (null == beat) {
    beat = setInterval(() => {
      if (current) {
        console.log(`render ${current.n}, ${current.what}: ` +
          `${Math.round((Date.now() - current.start) / 1000)}s so far, percentage unknown`)
      }
    }, BEAT_MS)
    beat.unref()
  }
  const input = Path.join(dir, 'tree.json')
  Fs.writeFileSync(input, JSON.stringify(tree, null, 2))
  const now = { n: ++runs, what, start: Date.now() }
  current = now
  const run = spawn(ALCHEMY, ['run', Path.join(dir, 'render.alc'), input],
    { stdio: ['ignore', 'pipe', 'pipe'] })
  // A run is stopped by killing the command and closing its output, as
  // spawnSync stopped one at its timeout and its maxBuffer: a command that
  // started others would leave them holding the output open.
  const kill = () => {
    run.kill('SIGKILL')
    run.stdout.destroy()
    run.stderr.destroy()
  }
  let flooded = false
  const collect = (stream) => {
    const chunks = []
    let bytes = 0
    stream.on('data', (d) => {
      chunks.push(d)
      bytes += d.length
      if (MAX_OUTPUT < bytes && !flooded) {
        flooded = true
        kill()
      }
    })
    return () => Buffer.concat(chunks).toString('utf8')
  }
  const stdout = collect(run.stdout)
  const stderr = collect(run.stderr)
  let stopped = false
  const stop = setTimeout(() => {
    stopped = true
    kill()
  }, LIMIT_MS)
  return new Promise((resolve) => {
    const done = (out) => {
      clearTimeout(stop)
      if (current === now) current = null
      resolve(out)
    }
    run.on('error', (e) => done({ code: 'NOT_RUN', message: String(e) }))
    run.on('close', (status, signal) => {
      if (flooded) {
        return done({ code: 'OUTPUT_LIMIT',
          message: `${what}: wrote more than ${MAX_OUTPUT / (1024 * 1024)} MiB` })
      }
      if (0 === status) return done({ text: stdout() })
      if (stopped) {
        return done({ code: 'STOPPED', message: `${what}: stopped after ${LIMIT_MS / 1000}s` })
      }
      try {
        done(JSON.parse(stderr()))
      } catch (e) {
        done({ code: 'NOT_JSON', message: `${stderr()} ${signal ?? ''}` })
      }
    })
  })
}

async function roundTrip (src) {
  const tree = parseEbnfTree(src)
  const out = await render(`the round trip of ${brief(src)}`, tree)
  Assert.ok(undefined !== out.text, JSON.stringify(out))
  Assert.equal(JSON.stringify(parseEbnfTree(out.text)), JSON.stringify(tree), out.text)
  const again = await render(`the text again of ${brief(src)}`, parseEbnfTree(out.text))
  Assert.equal(again.text, out.text)
  return out.text
}

async function writes (tree, text) {
  const out = await render(`the text of ${brief(JSON.stringify(tree))}`, tree)
  Assert.equal(out.text, text, JSON.stringify(out))
}

async function refuses (tree, what) {
  const out = await render(`the refusal ${brief(what)}`, tree)
  Assert.equal(out.code, 'TARGET_VALUE_UNREPRESENTABLE', JSON.stringify(out))
  Assert.match(out.message, what)
}

// What a grammar text recognises, compiled by this package: each sample
// it accepts.
function recognises (text, samples) {
  const tn = new Tabnas({ plugins: [ebnfPlugin] })
  return samples.filter((sample) => {
    const j = tn.make()
    j.ebnf(text)
    try {
      j.parse(sample)
      return true
    } catch {
      return false
    }
  })
}

// A grammar of one rule `a`, its one alternative the elements given.
const one = (...elements) => ({
  type: 'grammar',
  rules: [{ type: 'rule', name: 'a', alternatives: [elements] }],
})
const ref = (name, repeat) => (repeat ? { type: 'reference', name, repeat } : { type: 'reference', name })

// The grammars of the fixtures, this repository's example grammars and
// the oracle's sources that the reader reads.
function sources () {
  const out = []
  for (const line of Fs.readFileSync(Path.join(__dirname, '..', '..', 'test', 'spec', 'tree.tsv'), 'utf8').split('\n').slice(1)) {
    if ('' === line || line.startsWith('#') || line.split('\t')[1].startsWith('ERROR:')) continue
    out.push(line.split('\t')[0]
      .replace(/\\(.)/g, (m, c) => ({ n: '\n', r: '\r', t: '\t', '\\': '\\' })[c] ?? m))
  }
  const grammars = Path.join(__dirname, 'grammar')
  for (const file of Fs.readdirSync(grammars).filter((f) => f.endsWith('.ebnf'))) {
    out.push(Fs.readFileSync(Path.join(grammars, file), 'utf8'))
  }
  const oracle = JSON.parse(Fs.readFileSync(Path.join(__dirname, '..', '..', 'rs', 'tests', 'oracle', 'ebnf-ir.json'), 'utf8'))
  for (const c of oracle) {
    try {
      parseEbnfTree(c.src)
      out.push(c.src)
    } catch { /* a source the reader refuses */ }
  }
  return out
}

test('the render writes every fixture grammar back as its tree', { skip }, async () => {
  for (const src of sources()) await roundTrip(src)
})

test('the render writes a rule a line, its alternatives and elements spaced', { skip }, async () => {
  Assert.equal(await roundTrip('a ::= b c | d\n'), 'a ::= b c | d\n')
  // The ISO 14977 spellings leave nothing in the tree.
  Assert.equal(await roundTrip('a = b , c ;\nd = e ;\n'), 'a ::= b c\nd ::= e\n')
  Assert.equal(await roundTrip(''), '')
})

test('the render writes EBNF\'s terminals as W3C EBNF spells them', { skip }, async () => {
  Assert.equal(await roundTrip('a ::= "x" \'y"z\' #x41 [a-z_] [^<&] [#x20-#x7E] "\\"\n'),
    'a ::= "x" \'y"z\' #x41 [a-z_] [^<&] [ -~] "\\"\n')
  // A class that writes any code point writes a hexadecimal digit as one
  // too, which W3C EBNF would read as more of the code point before it.
  Assert.equal(await roundTrip('a ::= [-a#x2D] [#x5Cb] [abc#x7F]\n'),
    'a ::= [#x2D#x61#x2D] [#x5CB] [#x61#x62#x63#x7F]\n')
  Assert.equal(await roundTrip('a ::= b* c+ d? ( e f )*? g+*\n'), 'a ::= b* c+ d? ( e f )*? g+*\n')
})

test('the render writes another notation\'s elements as EBNF spells them', { skip }, async () => {
  await writes(one({ type: 'class', negated: false, members: [{ from: 97, to: 122 }, { from: 95, to: 95 }] }),
    'a ::= [a-z_]\n')
  await writes(one({ type: 'range', from: 97, to: 122 }), 'a ::= [a-z]\n')
  await writes(one({ type: 'any' }), 'a ::= [#x00-#x10FFFF]\n')
  await writes(one({ type: 'literal', text: '\n', caseSensitive: true }), 'a ::= #x0A\n')
  await writes(one({ type: 'literal', text: 'a', caseSensitive: false }), 'a ::= [aA]\n')
  await writes(one({ type: 'literal', text: '1-2', caseSensitive: false }), 'a ::= "1-2"\n')
  await writes(one(ref('b', [{ min: 2, max: 4 }])), 'a ::= b b ( b b? )?\n')
  await writes(one(ref('b', [{ min: 3 }])), 'a ::= b b b+\n')
  await writes(one(ref('b', [{ min: 1, max: 1 }])), 'a ::= b\n')
  await writes(one(ref('b', [{ min: 0 }, { min: 2, max: 3 }])), 'a ::= b* b* b*?\n')
  await writes({ type: 'grammar', rules: [{ type: 'rule', name: 'a', alternatives: [[ref('b')], []] }] },
    'a ::= ( b )?\n')
  await writes(one({ type: 'group', alternatives: [[ref('b')], []] }), 'a ::= ( b )?\n')
  await writes(one(ref('DIGIT'), ref('SP'), ref('DQUOTE')),
    'a ::= DIGIT SP DQUOTE\nDIGIT ::= [0-9]\nDQUOTE ::= \'"\'\nSP ::= " "\n')
})

test('the render refuses what EBNF cannot say, naming the rule', { skip }, async () => {
  await refuses(one({ type: 'token', negated: false, text: 'think' }), /the rule "a" holds the token terminal "think"/)
  await refuses(one({ type: 'prose', text: 'a > b' }), /the rule "a" holds the prose "a > b"/)
  await refuses({ type: 'grammar', rules: [{ type: 'rule', name: 'a', alternatives: [[ref('b')]] }, { type: 'rule', name: 'a', incremental: true, alternatives: [[ref('c')]] }] },
    /the rule "a" adds alternatives to an earlier rule/)
  await refuses({ type: 'grammar', rules: [{ type: 'rule', name: '1a', alternatives: [[ref('b')]] }] },
    /the rule "1a" has a name EBNF cannot spell/)
  await refuses(one(ref('a b')), /the rule "a" references "a b", a name EBNF cannot spell/)
  await refuses(one({ type: 'class', negated: false, members: [] }), /the rule "a" holds an empty class/)
  await refuses(one({ type: 'literal', text: '', caseSensitive: true }), /the rule "a" holds the empty literal/)
  await refuses(one({ type: 'literal', text: 'a"b\'c', caseSensitive: true }), /holds both quotes/)
  await refuses(one({ type: 'literal', text: 'a\nb', caseSensitive: true }), /holds a control character beside other characters/)
  await refuses(one({ type: 'literal', text: 'ab', caseSensitive: false }), /holds a letter beside another character/)
  await refuses(one({ type: 'literal', text: '\u00e9', caseSensitive: false }), /holds a character past ASCII that may have cases/)
  await refuses(one(ref('b', [{ min: 0, max: 0 }])), /a repetition of no copies/)
  await refuses(one(ref('b', [{ min: 0, max: 65 }])), /a repetition of more than 64 copies past its least/)
  await refuses({ type: 'grammar', rules: [{ type: 'rule', name: 'a', alternatives: [[]] }] },
    /the rule "a" holds alternatives that are all empty/)
})

test('the render refuses a tree that is not the schema\'s', { skip }, async () => {
  await refuses({ type: 'rules', rules: [] }, /the root's type is not grammar/)
  await refuses(one({ name: 'x', type: 'reference' }), /first member is not its type/)
  await refuses(one({ type: 'thing' }), /an element of the type "thing", which no element has/)
  await refuses(one({ type: 'reference', name: 1 }), /a reference's name is not of its kind/)
  await refuses(one({ type: 'literal', text: 'x' }), /a literal with no caseSensitive/)
  await refuses(one(ref('x', [{ min: 2, max: 1 }])), /a repetition whose max is below its min/)
  await refuses({ type: 'grammar', start: 1, rules: [] }, /a grammar's start is not text/)
})

// GBNF starts from the rule named root, wherever it stands, and its tree
// names it as the start: GBNF's reader reads `digit ::= [0-9]`,
// `num ::= digit+` and `root ::= num ("," num)*` as this tree, which
// tabnas/gbnf's tests hold. EBNF starts from its first rule, so the
// render writes root first, holding the rules before it until it has, and
// the grammar compiled from the text recognises what GBNF's does.
const NUMBERS = {
  type: 'grammar',
  start: 'root',
  rules: [
    { type: 'rule', name: 'digit', alternatives: [[{ type: 'class', negated: false, members: [{ from: 48, to: 57 }] }]] },
    { type: 'rule', name: 'num', alternatives: [[ref('digit', [{ min: 1 }])]] },
    { type: 'rule', name: 'root', alternatives: [[ref('num'), { type: 'group', alternatives: [[{ type: 'literal', text: ',', caseSensitive: true }, ref('num')]], repeat: [{ min: 0 }] }]] },
  ],
}
const ACCEPTED = ['1', '12', '1,2', '12,345,6']
const REFUSED = ['', 'a', '1,', ',1', '1,,2', '1;2']

test('the render writes the start rule first, holding the rules before it', { skip }, async () => {
  const text = 'root ::= num ( "," num )*\ndigit ::= [0-9]\nnum ::= digit+\n'
  await writes(NUMBERS, text)
  Assert.deepEqual(recognises(text, [...ACCEPTED, ...REFUSED]), ACCEPTED)
  // Nothing is held where the start rule comes first, and the rules
  // between it and the one before stay in their order.
  const [digit, num, root] = NUMBERS.rules
  await writes({ ...NUMBERS, rules: [root, digit, num] }, text)
  await writes({ ...NUMBERS, rules: [digit, root, num] }, text)
  // A tree with no start starts from its first rule, and is written in
  // its order.
  await writes({ type: 'grammar', rules: NUMBERS.rules }, 'digit ::= [0-9]\nnum ::= digit+\nroot ::= num ( "," num )*\n')
})

test('the render refuses a start the grammar does not define', { skip }, async () => {
  await refuses({ ...NUMBERS, start: 'top' }, /the grammar starts from the rule "top", which it does not define/)
  // A grammar of no rules starts nowhere, and is no text.
  await writes({ type: 'grammar', start: 'root', rules: [] }, '')
})
