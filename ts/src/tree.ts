/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

/*  tree.ts
 *  An EBNF document read as the schema `grammar-rules`: its rules as the
 *  text writes them, uncompiled and in source order (tabnas/bnf's
 *  docs/grammar-rules.md, and section 13.1 of alchemy's
 *  docs/streaming-plan.md). This is the tree a host translates: the part,
 *  alchemy/transduce.alc, makes the same tree's events from the rule
 *  passes of `make()`, and the render, alchemy/render.alc, writes it back
 *  as W3C EBNF.
 *
 *    { "type": "grammar", "rules": [
 *      { "type": "rule", "name": "A",
 *        "alternatives": [ [ element, ... ], ... ] }, ... ] }
 *
 *  The tree parser reads EBNF with the meta-grammar `ebnfRules` reads it
 *  with, its lexer and every alternate as they are, and actions of its own
 *  (TREE below) in place of the ones that build the compiler's IR, so the
 *  rule passes a part reads are the ones the meta-grammar makes. A token
 *  whose text needs reading (a class, a code point) is given what it reads
 *  as its value by the action that reads it, since a part reads a token's
 *  value and never the work an action does with it.
 *
 *  An element is an object whose first member names its type:
 *
 *    Name             { type: 'reference', name }
 *    "ab", 'ab'       { type: 'literal', text, caseSensitive: true }
 *    #x41             { type: 'char', value }
 *    [a-z], [^<&]     { type: 'class', negated, members: [{ from, to }] }
 *    ( ... )          { type: 'group', alternatives }
 *
 *  and its postfix operators are its last member, `repeat`, in the order
 *  they are written, the innermost first: `A*?` repeats `A` from 0 up and
 *  that from 0 to 1. A string's text is its characters, since W3C EBNF has
 *  no escapes; a class's members are code points, `#x20` or the
 *  character, a lone character a member whose bounds are one, and a `-`
 *  that joins no two members a member of its own. The ISO 14977 spellings
 *  this package reads (`=`, a comma between items, a closing `;`, and
 *  `(* ... *)` comments) leave nothing in the tree, and neither do
 *  comments. The tree holds the rules the text writes, a second
 *  definition of a name included, which the compiler refuses.
 *
 *  The tree holds what the schema allows, and EBNF what this package
 *  reads, so the parser refuses the rest, each with a code of its own: a
 *  name that is not one (ebnf_name), an empty string (ebnf_empty_literal),
 *  a code point past #x10FFFF or with no digits (ebnf_code_point), a class
 *  range that ends below where it starts (ebnf_class_range), an empty
 *  class (ebnf_empty_class), a stray bracket (ebnf_char_class), an empty
 *  alternative (ebnf_empty_alternative), a leading comma
 *  (ebnf_leading_comma), a subtraction (ebnf_subtraction), a special
 *  sequence or a `?` where an element is due (ebnf_special_sequence), ISO
 *  bracket repetition (ebnf_iso_repetition), a `*` or `+` where an element
 *  is due (ebnf_dangling_postfix) and a group left open (ebnf_unclosed).
 */

import type { Rule } from '@tabnas/parser'

import {
  ebnfRules,
  ebnfOptions,
  installEbnfRules,
  NAME_RE,
  EbnfParseError,
} from './converter'


// An element, a rule and a grammar of the schema, as this reader builds
// them: tabnas/bnf's `RulesElement`, `RulesRule` and `GrammarRules`.
type EbnfTreeRepeat = { min: number; max?: number }
type EbnfTreeElement = { type: string; [member: string]: any; repeat?: EbnfTreeRepeat[] }
type EbnfTreeRule = { type: 'rule'; name: string; alternatives: EbnfTreeElement[][] }
type EbnfTree = { type: 'grammar'; rules: EbnfTreeRule[] }


// The codes the tree parser refuses a source with, beside the engine's:
// the `error` and `hint` options `make()` gives the parser.
const TREE_OPTIONS = {
  error: {
    ebnf_name:
      'the name {src} is not a symbol name: one starts with a letter or an underscore, and goes on with letters, digits, _, . and -',
    ebnf_empty_literal:
      'the string {src} is empty, and EBNF has no epsilon terminal',
    ebnf_code_point:
      'the code point in {src} has no hex digits, or names no Unicode code point (the largest is #x10FFFF)',
    ebnf_class_range:
      'the character class {src} has a range that ends below where it starts',
    ebnf_empty_class:
      'the character class {src} is empty, and matches nothing',
    ebnf_char_class:
      'the {src} opens or closes no character class: a class opens and closes on one line, and holds no ]',
    ebnf_empty_alternative:
      'an alternative is empty, and EBNF has no epsilon terminal',
    ebnf_leading_comma:
      'the comma separates nothing: ISO 14977 writes it between two items',
    ebnf_subtraction:
      'the subtraction {src} is not supported: the tree has no difference of two languages',
    ebnf_special_sequence:
      'the {src} is where an element is due: a postfix ? follows an element, and ISO 14977 special sequences are not supported',
    ebnf_iso_repetition:
      'the bracket {src} is ISO 14977 repetition, which this dialect does not read',
    ebnf_dangling_postfix:
      'the {src} is where an element is due: repetition in this dialect is postfix',
    ebnf_unclosed:
      'a group is not closed',
  },
  hint: {
    ebnf_name:
      'Spell a name as a letter or an underscore, then letters, digits, _, . and -.',
    ebnf_empty_literal:
      'Write A? to make A optional.',
    ebnf_code_point:
      'A code point is #x and its hex digits, no larger than #x10FFFF.',
    ebnf_class_range:
      'Write a range from its lower end to its higher, as [a-z].',
    ebnf_empty_class:
      'A class holds a character or a range at least.',
    ebnf_char_class:
      'Write a literal ] as the string "]", and the ISO option [ A ] as A?.',
    ebnf_empty_alternative:
      'Write A? rather than A |.',
    ebnf_leading_comma:
      'Write A , B, with an item on each side of the comma.',
    ebnf_subtraction:
      'Write a difference of single characters as a negated class, as [^abc].',
    ebnf_special_sequence:
      'Write A? after the element it repeats.',
    ebnf_iso_repetition:
      'Write A* for { A }, and A? for the ISO [ A ].',
    ebnf_dangling_postfix:
      'Write A* and A+, after the element they repeat.',
    ebnf_unclosed:
      'Close the group with ).',
  },
}


// A code point's hex digits as the number they name, or -1 where they
// name none.
function codePointOf(hex: string): number {
  const n = parseInt(hex, 16)
  return Number.isFinite(n) && 0 <= n && n <= 0x10ffff ? n : -1
}


// A class's members, read as the IR parser reads them: `^` first negates
// it, `#xNN` is a code point, `a-b` is a range, and any other `-` is a
// member of its own; or the code the class is refused with.
function classValue(src: string): { negated: boolean; members: { from: number; to: number }[] } | string {
  const negated = '^' === src[1]
  const body = src.slice(negated ? 2 : 1, -1)
  if ('' === body) return 'ebnf_empty_class'

  const items: (number | '-')[] = []
  let i = 0
  while (i < body.length) {
    const c = body[i]
    if ('#' === c && 'x' === body[i + 1]) {
      let j = i + 2
      while (j < body.length && /[0-9a-fA-F]/.test(body[j])) j++
      const cp = j === i + 2 ? -1 : codePointOf(body.slice(i + 2, j))
      if (cp < 0) return 'ebnf_code_point'
      items.push(cp)
      i = j
      continue
    }
    if ('-' === c) {
      items.push('-')
      i++
      continue
    }
    const cp = body.codePointAt(i) as number
    items.push(cp)
    i += String.fromCodePoint(cp).length
  }

  const members: { from: number; to: number }[] = []
  for (let k = 0; k < items.length; k++) {
    const it = items[k]
    if ('-' === it) {
      members.push({ from: 0x2d, to: 0x2d })
      continue
    }
    if (k + 2 < items.length && '-' === items[k + 1] && 'number' === typeof items[k + 2]) {
      const hi = items[k + 2] as number
      if (hi < it) return 'ebnf_class_range'
      members.push({ from: it, to: hi })
      k += 2
      continue
    }
    members.push({ from: it, to: it })
  }
  return { negated, members }
}


// An action, as the meta-grammar's alternates and states hold one.
type Act = (r: Rule, ctx: any) => any

// The refusal of the token an alternate matched first, with a code: in
// an open state the rule's first, in a close state its close state's.
const refuse = (code: string): Act => (r) => r.o[0].bad(code)
const refuseClose = (code: string): Act => (r) => r.c[0].bad(code)

// A symbol name, or the refusal of the token that spells it.
function named(r: Rule): string | any {
  const name = String(r.o[0].val)
  return NAME_RE.test(name) ? name : r.o[0].bad('ebnf_name')
}

// The refusals an element's place shares: what can start no element.
const noElement = (fail: (code: string) => Act): Record<string, Act> => ({
  '#QM': fail('ebnf_special_sequence'),
  '#SUB': fail('ebnf_subtraction'),
  '#OB': fail('ebnf_iso_repetition'),
  '#CB': fail('ebnf_iso_repetition'),
  '#OS': fail('ebnf_char_class'),
  '#CS': fail('ebnf_char_class'),
  '#STAR': fail('ebnf_dangling_postfix'),
  '#PLUS': fail('ebnf_dangling_postfix'),
})

// What each action of `ebnfRules` builds in the tree instead: the states'
// own (`bo`, `bc`), and the alternates', keyed by their `s` pattern, an
// alternate with none under ''. Every alternate that carries an action in
// `ebnfRules` has one here, and no other.
const TREE: Record<string, {
  bo?: Act
  bc?: Act
  open?: Record<string, Act>
  close?: Record<string, Act>
}> = {
  ebnf: {
    bo: (r) => { r.node = { type: 'grammar', rules: [] } },
  },

  prod: {
    open: {
      '#TX #DEFOP': (r) => {
        const name = named(r)
        if ('string' !== typeof name) return name
        r.u.name = name
      },
    },
    bc: (r) => {
      if (r.child && undefined !== r.child.node) {
        r.node.rules.push({ type: 'rule', name: r.u.name, alternatives: r.child.node })
      }
    },
  },

  alts: {
    bo: (r) => { r.node = [] },
    bc: (r) => {
      if (r.child && undefined !== r.child.node) r.node.push(r.child.node)
    },
  },

  seq: {
    bo: (r) => { r.node = [] },
    open: {
      '#TX #DEFOP': refuse('ebnf_empty_alternative'),
      '#ALT': refuse('ebnf_empty_alternative'),
      '#RP': refuse('ebnf_empty_alternative'),
      '#SC': refuse('ebnf_empty_alternative'),
      '#ZZ': refuse('ebnf_empty_alternative'),
      '#CA': refuse('ebnf_leading_comma'),
      ...noElement(refuse),
    },
    close: noElement(refuseClose),
  },

  item: {
    close: {
      '': (r) => {
        r.u.got = true
        r.u.atom = r.child.node
      },
    },
    // The atom, its repetitions appended, onto the alternative.
    bc: (r) => {
      if (!r.u.got || undefined === r.u.atom) return
      const el = r.u.atom
      const reps = (r.child && Array.isArray(r.child.node)) ? r.child.node : []
      if (0 < reps.length) el.repeat = reps.map((rep: EbnfTreeRepeat) => ({ ...rep }))
      r.node.push(el)
    },
  },

  // The postfix operators, innermost first, each a repetition.
  post: {
    bo: (r) => { r.node = [] },
    open: {
      '#QM': (r) => { r.node.push({ min: 0, max: 1 }) },
      '#STAR': (r) => { r.node.push({ min: 0 }) },
      '#PLUS': (r) => { r.node.push({ min: 1 }) },
    },
    bc: (r) => {
      if (r.child && Array.isArray(r.child.node)) r.node.push(...r.child.node)
    },
  },

  atom: {
    bo: (r) => { r.node = undefined; r.u.group = false },
    open: {
      '#ST': (r) => {
        const text = String(r.o[0].val)
        if ('' === text) return r.o[0].bad('ebnf_empty_literal')
        r.node = { type: 'literal', text, caseSensitive: true }
      },
      // The class's negation and members, which its token carries from
      // here.
      '#CC': (r) => {
        const read = classValue(String(r.o[0].src))
        if ('string' === typeof read) return r.o[0].bad(read)
        r.o[0].val = read
        r.node = { type: 'class', negated: read.negated, members: read.members.map((m) => ({ ...m })) }
      },
      // The code point, which its token carries from here.
      '#HX': (r) => {
        const value = codePointOf(String(r.o[0].src).slice(2))
        if (value < 0) return r.o[0].bad('ebnf_code_point')
        r.o[0].val = value
        r.node = { type: 'char', value }
      },
      '#TX': (r) => {
        const name = named(r)
        if ('string' !== typeof name) return name
        r.node = { type: 'reference', name }
      },
      '#LP': (r) => { r.u.group = true },
      '#QM': refuse('ebnf_special_sequence'),
      '#SUB': refuse('ebnf_subtraction'),
      '#OB': refuse('ebnf_iso_repetition'),
      '#CB': refuse('ebnf_iso_repetition'),
      '#OS': refuse('ebnf_char_class'),
    },
    close: {
      '#RP': (r) => { r.node = { type: 'group', alternatives: r.child.node } },
      '': refuse('ebnf_unclosed'),
    },
  },
}


// The meta-grammar's rule table with the tree's actions: every alternate
// of `ebnfRules`, its `s`, `p`, `r`, `b`, `g` and conditions as they are,
// and each action replaced by the one TREE gives it.
function treeRules(): typeof ebnfRules {
  const table: any = {}
  let used = 0
  for (const name of Object.keys(ebnfRules)) {
    const ir = ebnfRules[name]
    const tree = TREE[name] ?? {}
    const bind = (alts: any[] | undefined, acts: Record<string, Act> | undefined,
      state: string) =>
      alts?.map((alt) => {
        const { a, ...rest } = alt
        if (null == a) return rest
        const act = acts?.[alt.s ?? '']
        if (null == act) {
          throw new Error(`ebnf tree: ${name}.${state} has no action for ${alt.s ?? '(no s)'}`)
        }
        used++
        return { ...rest, a: act }
      })
    table[name] = {
      bo: tree.bo,
      bc: tree.bc,
      open: bind(ir.open, tree.open, 'open'),
      close: bind(ir.close, tree.close, 'close'),
    }
  }
  // Every action declared is bound at least once: an alternate the
  // table's open and close states share binds one action twice.
  const declared = Object.entries(TREE).flatMap(([name, t]) =>
    [...Object.keys(t.open ?? {}).map((s) => `${name}.open ${s}`),
      ...Object.keys(t.close ?? {}).map((s) => `${name}.close ${s}`)])
  const bound = new Set<string>()
  for (const name of Object.keys(ebnfRules)) {
    for (const state of ['open', 'close'] as const) {
      for (const alt of (ebnfRules[name] as any)[state] ?? []) {
        if (null != alt.a) bound.add(`${name}.${state} ${alt.s ?? ''}`)
      }
    }
  }
  const unbound = declared.filter((key) => !bound.has(key))
  if (0 < unbound.length || 0 === used) {
    throw new Error(`ebnf tree: actions the meta-grammar does not take: ${unbound.join(', ')}`)
  }
  return table
}

const ebnfTreeRules = treeRules()


// A fresh parser that reads an EBNF document as the tree: the parser a
// host streams a document through with the part, whose rule passes the
// part reads, and whose value is the tree.
function make(): any {
  const { Tabnas } = require('@tabnas/parser')
  const options = ebnfOptions()
  options.error = { ...TREE_OPTIONS.error }
  options.hint = { ...TREE_OPTIONS.hint }
  const tn = new Tabnas(options)
  installEbnfRules(tn, ebnfTreeRules)
  return tn
}


let _treeParser: any = null

// An EBNF document as the tree, read whole. A source the meta-grammar
// refuses, or the tree cannot hold, fails with an EbnfParseError naming
// the line and column, its `cause` the engine's error with its code.
function parseEbnfTree(src: string): EbnfTree {
  if (null == _treeParser) _treeParser = make()
  try {
    // An empty source makes no rule pass, and the engine answers nothing
    // for it: it is the grammar of no rules, as a source of comments is.
    return (_treeParser.parse(src) ?? { type: 'grammar', rules: [] }) as EbnfTree
  } catch (e: any) {
    const line = e?.lineNumber ?? e?.row
    const column = e?.columnNumber ?? e?.col
    const loc = (line != null && column != null)
      ? ` at line ${line}, column ${column}`
      : ''
    const raw = e?.message ? String(e.message).split('\n')[0] : String(e)
    throw new EbnfParseError(`ebnf: parse error${loc}: ${raw}`, { line, column }, e)
  }
}


export { make, parseEbnfTree, ebnfTreeRules }

export type { EbnfTree, EbnfTreeRule, EbnfTreeElement, EbnfTreeRepeat }
