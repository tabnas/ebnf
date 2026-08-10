/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

/*  converter.ts
 *  EBNF -> tabnas grammar spec converter: the EBNF FRONT-END.
 *
 *  This file parses EBNF text into the notation-neutral grammar IR that
 *  `@tabnas/bnf` compiles. Everything downstream of that IR — desugaring,
 *  left-recursion elimination, tail repeats, probe dispatch, literal
 *  lifting, token allocation, first-set analysis, chain emission — lives
 *  in `@tabnas/bnf` and is shared with the ABNF and GBNF front-ends.
 *
 *    EBNF text ──parseEbnf──▶ Grammar ──bnf.emitGrammarSpec──▶ GrammarSpec
 *
 *  WHICH EBNF. "EBNF" names a family, not a language: ISO/IEC 14977,
 *  W3C, Wirth and a long tail of per-tool dialects disagree on the
 *  definition operator, the terminator, the comment syntax, and — worst
 *  — on what brackets mean. The primary dialect here is **W3C EBNF**,
 *  the notation the XML, XPath and XQuery specifications publish their
 *  grammars in, because it has a real published corpus and because its
 *  operators (postfix `?` `*` `+`, `|`, `( … )`, `[…]` character
 *  classes) map one-for-one onto the IR.
 *
 *  A few ISO/IEC 14977 spellings are accepted on top, but ONLY the ones
 *  that cannot collide with the W3C reading: `=` as a definition
 *  operator, `,` as an explicit concatenation separator, `;` as a
 *  production terminator, and `(* … *)` comments. ISO's `{ … }`
 *  repetition and `[ … ]` option are NOT accepted, because `[ … ]` is
 *  a W3C character class — the two readings of `[a-z]` are a set of
 *  three characters and an optional three-element sequence, and no
 *  parser can be right about both. That collision is the reason the
 *  dialect has to be a documented choice rather than a guess.
 *
 *  What is genuinely EBNF, and therefore lives here:
 *
 *    - `ebnfRules`, the tabnas grammar that reads EBNF syntax itself,
 *      and `getEbnfParser`, which installs it on a fresh instance;
 *    - W3C character classes (`[a-z]`, `[^<&]`, `[#x20-#x7E]`) and the
 *      standalone `#xNN` code-point form;
 *    - case-SENSITIVE quoted strings, the W3C default (unlike ABNF);
 *    - postfix repetition (`?`, `*`, `+`) and `( … )` grouping;
 *    - the named rejections for constructs the IR cannot express:
 *      subtraction (`A - B`), special sequences (`? … ?`), and ISO
 *      bracket repetition.
 *
 *  This is a BEST-EFFORT front-end. See the README's "What is and is
 *  not supported" for the itemised list; the short version is that a
 *  grammar using only the constructs above compiles, and anything else
 *  raises a named error rather than silently compiling to the wrong
 *  language.
 */

import type { GrammarSpec, Rule } from '@tabnas/parser'

import {
  emitGrammarSpec,
  eliminateLeftRecursion,
} from '@tabnas/bnf'

import type {
  ConvertOptions,
  Element,
  Sequence,
  Production,
  Grammar,
} from '@tabnas/bnf'

// The IR types are re-exported under this package's own names so a
// caller can talk about the AST without importing `@tabnas/bnf`.
type EbnfConvertOptions = ConvertOptions
type EbnfElement = Element
type EbnfSequence = Sequence
type EbnfProduction = Production
type EbnfGrammar = Grammar


// Error raised when the EBNF source itself cannot be read: a syntax
// error, or a construct this front-end deliberately refuses (see the
// `fail*` helpers below). `line` / `column` come from the offending
// token where one is available.
class EbnfParseError extends Error {
  readonly line?: number
  readonly column?: number
  readonly cause?: unknown
  constructor(
    message: string,
    location?: { line?: number; column?: number },
    cause?: unknown,
  ) {
    super(message)
    this.name = 'EbnfParseError'
    this.line = location?.line
    this.column = location?.column
    this.cause = cause
  }
}


// Error raised when the EBNF parsed cleanly but `@tabnas/bnf` could not
// compile the resulting IR — an unknown rule reference, a purely
// left-recursive rule, an ambiguous FIRST set. The shared compiler's
// message is kept verbatim (it names the offending rule); only its
// package prefix is restamped to `ebnf:`.
class EbnfCompileError extends Error {
  readonly cause?: unknown
  constructor(message: string, cause?: unknown) {
    super(message)
    this.name = 'EbnfCompileError'
    this.cause = cause
  }
}


// A production name (W3C calls it a symbol). Deliberately narrower than
// "whatever the text matcher produced": the lexer's bareword token runs
// to the next delimiter, so without this check a typo like `Foo! ::= …`
// would become a rule genuinely named `Foo!` and the mistake would only
// surface much later as an unknown-rule reference.
const NAME_RE = /^[A-Za-z_][A-Za-z0-9_.-]*$/


// Declarative definition of the EBNF grammar itself, expressed as
// tabnas rules. Each rule names its `open`/`close` alt list and, where
// necessary, a `bo`/`bc` state hook for AST assembly.
//
// Token vocabulary:
//   #DEF   `::=` (W3C definition operator)
//   #DEFE  `=`   (ISO definition operator)
//   #DEFOP tokenSet of the two above
//   #ALT   `|`   (alternation)
//   #LP    `(`   #RP `)`      grouping
//   #STAR  `*`   #PLUS `+`   #QM `?`   postfix repetition
//   #CA    `,`   (ISO concatenation — accepted and ignored)
//   #SC    `;`   (ISO terminator — accepted and ignored)
//   #OS    `[`   #CS `]`   only ever seen when a character class is
//                          malformed; the well-formed case is #CC
//   #OB    `{`   #CB `}`   only ever seen in ISO repetition, which is
//                          rejected by name
//   #CC    `[…]` a complete W3C character class (match.token, eager)
//   #HX    `#xNN` a standalone code point (match.token, eager)
//   #SUB   `-`   the subtraction operator (match.token, eager)
//   #CM    `(* … *)` ISO comment (match.token, eager; skipped by the
//                    parser like any other #CM)
//   #TX    bare identifier (tabnas default text token)
//   #ST    quoted string literal (tabnas default string token)
//   #ZZ    end-of-source
//
// Grammar:
//   ebnf ::= prod*
//   prod ::= NAME ('::=' | '=') alts ';'?
//   alts ::= seq ('|' seq)*
//   seq  ::= (','? elem)*
//   elem ::= atom post
//   post ::= ('?' | '*' | '+')*
//   atom ::= NAME | STRING | CHARCLASS | HEX | '(' alts ')'
const ebnfRules: Record<
  string,
  {
    bo?: (r: Rule) => void
    bc?: (r: Rule) => void
    open?: any[]
    close?: any[]
  }
> = {
  // Top-level: accumulates productions into r.node.
  ebnf: {
    bo: (r) => { r.node = [] },
    open: [
      { s: '#ZZ', g: 'empty' },
      { p: 'prod' },
    ],
    close: [{ s: '#ZZ' }],
  },

  // One production per invocation; tail-recurses (r: 'prod') for the
  // next. Inherits its parent's node (the productions array) and
  // appends to it in `bc` once its `alts` child has returned.
  //
  // A production header is `NAME ::=` or `NAME =`. Two tokens of
  // lookahead are what separate "the next production starts here" from
  // "this bareword is a reference inside the current sequence"; the
  // same pattern appears in `seq` below as the sequence terminator.
  prod: {
    open: [
      {
        s: '#TX #DEFOP',
        a: (r: Rule) => {
          r.u.name = checkName(r.o[0].val, r.o[0])
        },
        p: 'alts',
      },
    ],
    close: [
      // ISO terminator, then another production: consume the `;` only.
      { s: '#SC #TX #DEFOP', b: 2, r: 'prod' },
      // ISO terminator at the end of the grammar: consume and pop.
      { s: '#SC' },
      // W3C style, no terminator: the next `NAME ::=` starts the next
      // production, so back up both tokens and re-enter.
      { s: '#TX #DEFOP', b: 2, r: 'prod' },
      { b: 1 },
    ],
    bc: (r) => {
      if (r.child && r.child.node !== undefined) {
        r.node.push({ name: r.u.name, alts: r.child.node })
      }
    },
  },

  // A list of alternative sequences separated by `|`. Owns its own
  // array (`bo` resets it) and pushes each seq result in `bc`.
  alts: {
    bo: (r) => { r.node = [] },
    open: [{ p: 'seq' }],
    close: [
      { s: '#ALT', p: 'seq' },
      { b: 1 },
    ],
    bc: (r) => {
      if (r.child && r.child.node !== undefined) {
        r.node.push(r.child.node)
      }
    },
  },

  // A (possibly empty) sequence of elements. `open` and `close` share
  // one alt list — the decision is the same whether this is the first
  // element or the tenth — built by `seqAlts()` below.
  seq: {
    bo: (r) => { r.node = [] },
    open: seqAlts(),
    close: seqAlts(),
  },

  // One element: an atom followed by a (possibly empty) run of postfix
  // repetition operators. The atom is parsed by `atom` and stashed on
  // `r.u`; `post` then collects the operators, and `bc` wraps the atom
  // in them outermost-last before appending to the parent seq's array.
  //
  // The `c:` guard is what makes the two-child sequence work: a close
  // state is re-entered every time a child returns, so without it the
  // `post` push would repeat forever.
  elem: {
    open: [{ p: 'atom' }],
    close: [
      {
        c: (r: Rule) => !r.u.got,
        a: (r: Rule) => {
          r.u.got = true
          r.u.atom = r.child.node
        },
        p: 'post',
      },
      { b: 1 },
    ],
    bc: (r) => {
      if (!r.u.got || undefined === r.u.atom) return
      let el: EbnfElement = r.u.atom
      const ops = (r.child && Array.isArray(r.child.node)) ? r.child.node : []
      for (const op of ops) {
        el = { kind: op, inner: el } as EbnfElement
      }
      r.node.push(el)
    },
  },

  // Zero or more postfix operators, innermost first. Tail-recurses
  // through `p:` so a stacked `(A)*?` is read left to right; W3C EBNF
  // never stacks them, but reading a stack is strictly more permissive
  // than failing on one.
  post: {
    bo: (r) => { r.node = [] },
    open: [
      { s: '#QM', a: (r: Rule) => { r.node.push('opt') }, p: 'post' },
      { s: '#STAR', a: (r: Rule) => { r.node.push('star') }, p: 'post' },
      { s: '#PLUS', a: (r: Rule) => { r.node.push('plus') }, p: 'post' },
      { b: 1, g: 'empty' },
    ],
    close: [{ b: 1 }],
    bc: (r) => {
      if (r.child && Array.isArray(r.child.node)) {
        r.node.push(...r.child.node)
      }
    },
  },

  // The atom body — a bareword reference, a quoted terminal, a
  // character class, a code point, or a parenthesised group. Sets its
  // OWN r.node so the enclosing `elem` can read it from `r.child.node`.
  atom: {
    bo: (r) => { r.node = undefined; r.u.group = false },
    open: [
      {
        s: '#ST',
        a: (r: Rule) => { r.node = stringTerm(r.o[0]) },
      },
      {
        s: '#CC',
        a: (r: Rule) => { r.node = parseCharClass(r.o[0].src as string, r.o[0]) },
      },
      {
        s: '#HX',
        a: (r: Rule) => { r.node = hexTerm(r.o[0].src as string, r.o[0]) },
      },
      {
        s: '#TX',
        a: (r: Rule) => {
          r.node = { kind: 'ref', name: checkName(r.o[0].val, r.o[0]) }
        },
      },
      {
        s: '#LP',
        a: (r: Rule) => { r.u.group = true },
        p: 'alts',
      },
      // Named rejections. Reached when one of these tokens turns up
      // where an atom was expected — see the `fail*` helpers.
      { s: '#QM', a: (r: Rule) => failSpecialSequence(r.o[0]) },
      { s: '#SUB', a: (r: Rule) => failSubtraction(r.o[0]) },
      { s: '#OB', a: (r: Rule) => failIsoRepetition(r.o[0]) },
      { s: '#CB', a: (r: Rule) => failIsoRepetition(r.o[0]) },
      { s: '#OS', a: (r: Rule) => failCharClass(r.o[0]) },
    ],
    close: [
      {
        s: '#RP',
        c: (r: Rule) => r.u.group,
        a: (r: Rule) => {
          r.node = { kind: 'group', alts: r.child.node }
        },
      },
      // A group that never closed: report it here, where the opening
      // `(` is still the thing being parsed, rather than letting the
      // failure surface as a stray token further up.
      {
        c: (r: Rule) => r.u.group,
        a: (r: Rule) => failUnclosedGroup(r.o[0]),
      },
      // Simple atoms set r.node in open; pop without consuming.
      { b: 1 },
    ],
  },
}


// The alternatives shared by `seq.open` and `seq.close`. A sequence
// ends at `|`, `)`, `;`, end-of-source, or the two-token `NAME ::=`
// header of the next production. Everything else either starts an
// element, is the ignorable ISO comma, or is a construct rejected by
// name.
function seqAlts(): any[] {
  return [
    // Sequence terminators — matched, then backed out so the enclosing
    // rule sees the token.
    { s: '#TX #DEFOP', b: 2, g: 'end' },
    { s: '#ALT', b: 1, g: 'end' },
    { s: '#RP', b: 1, g: 'end' },
    { s: '#SC', b: 1, g: 'end' },
    { s: '#ZZ', b: 1, g: 'end' },

    // ISO 14977 writes concatenation explicitly. Consume the comma and
    // carry on: in this dialect juxtaposition already means sequence,
    // so the separator adds nothing but is harmless to accept.
    { s: '#CA', p: 'elem' },

    // Element starters.
    { s: '#ST', b: 1, p: 'elem' },
    { s: '#CC', b: 1, p: 'elem' },
    { s: '#HX', b: 1, p: 'elem' },
    { s: '#TX', b: 1, p: 'elem' },
    { s: '#LP', b: 1, p: 'elem' },

    // Named rejections, checked before the catch-all so the diagnostic
    // names the construct instead of "unexpected token".
    { s: '#QM', a: (r: Rule) => failSpecialSequence(r.o[0]) },
    { s: '#SUB', a: (r: Rule) => failSubtraction(r.o[0]) },
    { s: '#OB', a: (r: Rule) => failIsoRepetition(r.o[0]) },
    { s: '#CB', a: (r: Rule) => failIsoRepetition(r.o[0]) },
    { s: '#OS', a: (r: Rule) => failCharClass(r.o[0]) },
    { s: '#CS', a: (r: Rule) => failCharClass(r.o[0]) },
    { s: '#STAR', a: (r: Rule) => failDanglingPostfix(r.o[0]) },
    { s: '#PLUS', a: (r: Rule) => failDanglingPostfix(r.o[0]) },

    { b: 1 },
  ]
}


// Location of a token, for error reporting. Tokens carry 1-based row
// (`rI`) and column (`cI`).
function tokenLoc(tkn: any): { line?: number; column?: number } {
  return { line: tkn?.rI, column: tkn?.cI }
}

function at(tkn: any): string {
  const loc = tokenLoc(tkn)
  return (null != loc.line && null != loc.column)
    ? ` at line ${loc.line}, column ${loc.column}`
    : ''
}


// ISO/IEC 14977 exception (`A - B`) and the same operator in W3C EBNF
// (`Char - ']'`). The IR has no difference operator, and there is no
// general way to synthesise one: subtracting an arbitrary language from
// another is not a regular operation over the `Element` kinds, and
// faking it with a negated character class only works for the special
// case where both sides are single characters.
function failSubtraction(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: subtraction ('-')${at(tkn)} is not supported. The grammar IR ` +
    `has no difference operator, so 'A - B' cannot be compiled. Where ` +
    `both sides are single characters, write the difference as a negated ` +
    `character class instead — '[^abc]' rather than 'Char - [abc]'.`,
    tokenLoc(tkn),
  )
}


// ISO/IEC 14977 special sequences — `? anything at all ?` — are an
// escape hatch for prose, deliberately undefined by the standard.
// There is nothing to compile. This also catches a `?` used as a
// postfix operator with no element in front of it.
function failSpecialSequence(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: unexpected '?'${at(tkn)}. A postfix '?' must follow an ` +
    `element, and ISO 14977 special sequences ('? … ?') are not ` +
    `supported: their content is undefined by the standard, so there is ` +
    `nothing to compile.`,
    tokenLoc(tkn),
  )
}


// ISO/IEC 14977 bracket repetition (`{ A }`) and option (`[ A ]`).
// `[ … ]` is a character class in this dialect, so accepting the ISO
// reading is impossible; `{ … }` could be accepted in principle, but
// supporting half of a bracket pair invites grammars that are ISO
// everywhere except where they silently are not.
function failIsoRepetition(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: ISO 14977 bracket repetition '{ … }'${at(tkn)} is not ` +
    `supported. This front-end reads W3C EBNF, where repetition is ` +
    `postfix: write 'A*' for '{ A }' and 'A?' for the ISO '[ A ]'. ` +
    `('[ … ]' is a character class here, which is why the ISO reading ` +
    `cannot also be offered.)`,
    tokenLoc(tkn),
  )
}


// A `[` or `]` that the character-class matcher did not claim. The
// matcher takes a complete `[…]` in one bite, so a bare bracket means
// the class never closed on the same line — or that the grammar meant
// the ISO option bracket.
function failCharClass(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: stray '${tkn?.src}'${at(tkn)}. A character class must open ` +
    `and close on one line, contain no ']' (write a literal ']' as the ` +
    `string "]"), and is written '[a-z]', '[^<&]' or '[#x20-#x7E]'. ` +
    `ISO 14977's optional '[ A ]' is not supported — write 'A?'.`,
    tokenLoc(tkn),
  )
}


// `*` or `+` with nothing in front of it. Worth its own message
// because ABNF (and therefore half the BNF-family muscle memory) puts
// repetition in front of the element rather than after it.
function failDanglingPostfix(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: unexpected '${tkn?.src}'${at(tkn)}. Repetition in this ` +
    `dialect is postfix — 'A*' and 'A+', not ABNF's '*A' and '1*A'.`,
    tokenLoc(tkn),
  )
}


function failUnclosedGroup(tkn: any): never {
  throw new EbnfParseError(
    `ebnf: unclosed group — '(' has no matching ')'${at(tkn)}.`,
    tokenLoc(tkn),
  )
}


// Validate a symbol name at the point it is read, so the diagnostic can
// point at the source position.
function checkName(name: string, tkn: any): string {
  if (!NAME_RE.test(name)) {
    throw new EbnfParseError(
      `ebnf: '${name}'${at(tkn)} is not a valid symbol name. A name ` +
      `starts with a letter or underscore and continues with letters, ` +
      `digits, '_', '.' or '-'.`,
      tokenLoc(tkn),
    )
  }
  return name
}


// A quoted terminal. W3C EBNF literals are case-SENSITIVE (unlike RFC
// 5234's, which are not), so the flag is set explicitly rather than
// left to the IR default. Single and double quotes are interchangeable
// — the spec grammars use both, and the choice is only ever about
// which quote character the literal itself contains.
function stringTerm(tkn: any): EbnfElement {
  const literal = tkn.val as string
  if ('' === literal) {
    throw new EbnfParseError(
      `ebnf: empty string literal${at(tkn)} matches nothing. W3C EBNF ` +
      `has no epsilon terminal; to make a construct optional write 'A?'.`,
      tokenLoc(tkn),
    )
  }
  return { kind: 'term', literal, caseSensitive: true }
}


// A standalone `#xNN` code point — `Char ::= #x9 | #xA | #xD | …`.
function hexTerm(src: string, tkn: any): EbnfElement {
  const cp = codePoint(src.slice(2), src, tkn)
  return { kind: 'term', literal: String.fromCodePoint(cp), caseSensitive: true }
}


// Decode a hex code-point body, rejecting anything Unicode cannot
// represent. Without the ceiling `String.fromCodePoint` throws a bare
// `RangeError` with no hint about which grammar line caused it.
function codePoint(hex: string, src: string, tkn: any): number {
  const n = parseInt(hex, 16)
  if (!Number.isFinite(n) || n < 0 || 0x10FFFF < n) {
    throw new EbnfParseError(
      `ebnf: '${src}'${at(tkn)} is not a Unicode code point ` +
      `(the maximum is #x10FFFF).`,
      tokenLoc(tkn),
    )
  }
  return n
}


// Decode a W3C character class into an IR `regex` element.
//
//   [a-z]           => [a-z]
//   [abc]           => [abc]
//   [^<&]           => [^<&]
//   [#x20-#x7E]     => the printable ASCII range
//   [#x9#xA#xD]     => tab, newline, or carriage return
//
// Every member is emitted as an explicit escape rather than as its raw
// character, so nothing inside the class can be re-read as regex
// syntax — a class containing `]`, `^`, `\` or `-` needs no special
// casing on the way out.
//
// There are NO escape sequences: W3C EBNF does not define any, so a
// backslash is the backslash character, exactly as it is in a quoted
// literal. That is also why `]` cannot appear inside a class — the
// matcher ends the class at the first one. Write it as the string
// literal `"]"` instead.
function parseCharClass(src: string, tkn: any): EbnfElement {
  const negated = '^' === src[1]
  const body = src.slice(negated ? 2 : 1, -1)

  if ('' === body) {
    throw new EbnfParseError(
      `ebnf: empty character class '${src}'${at(tkn)} matches nothing.`,
      tokenLoc(tkn),
    )
  }

  // Members, in source order. A `-` is kept as a marker rather than a
  // code point so the range fold below can tell `a-z` (a range) from
  // `[-a]` (a literal hyphen and an `a`).
  const items: (number | '-')[] = []
  let i = 0
  while (i < body.length) {
    const c = body[i]

    // `#xNN` — the W3C spelling of a code point inside a class.
    if ('#' === c && ('x' === body[i + 1] || 'X' === body[i + 1])) {
      let j = i + 2
      while (j < body.length && /[0-9a-fA-F]/.test(body[j])) j++
      if (j === i + 2) {
        throw new EbnfParseError(
          `ebnf: '#x' with no hex digits in character class ` +
          `'${src}'${at(tkn)}.`,
          tokenLoc(tkn),
        )
      }
      items.push(codePoint(body.slice(i + 2, j), body.slice(i, j), tkn))
      i = j
      continue
    }

    if ('-' === c) {
      items.push('-')
      i++
      continue
    }

    // Iterate by code point, not by UTF-16 unit, so an astral character
    // written literally in the class survives as one member.
    const cp = body.codePointAt(i) as number
    items.push(cp)
    i += String.fromCodePoint(cp).length
  }

  // Fold `lo - hi` triples into ranges; any other `-` is a literal
  // hyphen (the W3C grammars rely on this for classes like `[-+]`).
  type Part = { lo: number; hi: number }
  const parts: Part[] = []
  for (let k = 0; k < items.length; k++) {
    const it = items[k]
    if ('-' === it) {
      parts.push({ lo: 0x2D, hi: 0x2D })
      continue
    }
    if (k + 2 < items.length &&
      '-' === items[k + 1] &&
      'number' === typeof items[k + 2]) {
      const hi = items[k + 2] as number
      if (hi < it) {
        throw new EbnfParseError(
          `ebnf: reversed range in character class '${src}'${at(tkn)} — ` +
          `the low end must not be greater than the high end.`,
          tokenLoc(tkn),
        )
      }
      parts.push({ lo: it, hi })
      k += 2
      continue
    }
    parts.push({ lo: it, hi: it })
  }

  // Above the BMP a `\uXXXX` escape is not enough — `\u{…}` is, but it
  // needs the `u` flag, which changes how the whole pattern is read.
  // Switch the whole class over together so the two spellings never mix.
  const astral = parts.some((p) => 0xFFFF < p.lo || 0xFFFF < p.hi)
  const esc = (n: number) => astral
    ? '\\u{' + n.toString(16) + '}'
    : '\\u' + n.toString(16).padStart(4, '0')

  const pattern = '[' + (negated ? '^' : '') +
    parts.map((p) => p.lo === p.hi ? esc(p.lo) : esc(p.lo) + '-' + esc(p.hi))
      .join('') +
    ']'

  return { kind: 'regex', pattern, flags: astral ? 'u' : '' }
}


// ISO/IEC 14977 comments, `(* … *)`.
//
// These cannot be declared as an ordinary `comment.def` entry: the
// fixed-token matcher runs before the comment matcher, so `(` is
// already a `#LP` by the time the comment matcher is offered the
// position, and `(*` never survives to be recognised. A match-token
// matcher runs FIRST of all, so the comment is claimed here instead
// and emitted as an ordinary `#CM`, which the parser skips like any
// other comment.
//
// The row/column bookkeeping is manual for the same reason: the
// generic match-matcher path advances the source index and column but
// knows nothing about embedded newlines, so a multi-line comment would
// otherwise shift every subsequent error's reported line.
function isoCommentMatcher(lex: any): any {
  const pnt = lex.pnt
  const src = lex.src as string
  if ('(' !== src[pnt.sI] || '*' !== src[pnt.sI + 1]) return undefined

  const endI = src.indexOf('*)', pnt.sI + 2)
  if (endI < 0) return undefined  // unterminated: leave it to `(` + `*`

  const stop = endI + 2
  const text = src.substring(pnt.sI, stop)
  const tkn = lex.token('#CM', text, text, pnt)

  let rI = pnt.rI
  let cI = pnt.cI
  for (let i = pnt.sI; i < stop; i++) {
    if ('\n' === src[i]) { rI++; cI = 1 } else { cI++ }
  }
  pnt.sI = stop
  pnt.rI = rI
  pnt.cI = cI

  return tkn
}
; (isoCommentMatcher as any).eager$ = true


// Every match-token matcher in this grammar is `eager$`: it fires
// wherever its pattern matches, rather than only where the current
// rule's token column already expects it. That is safe here — and it
// is what keeps the rule table honest — because each pattern starts
// with a character that has exactly one meaning in EBNF:
//
//   `[` only ever opens a character class;
//   `#` only ever opens a code point (the `#` line comment is off);
//   `::=` / `=` only ever define a production;
//   `-` only ever subtracts, since a hyphen INSIDE a name is consumed
//       by the text matcher as part of that name and never reaches a
//       position of its own.
//
// The alternative — ABNF's approach of listing every token in `s:`
// patterns to widen the token column — is needed when a matcher's
// pattern is ambiguous with a bareword. None of these are.
function eager(re: RegExp): RegExp {
  ; (re as any).eager$ = true
  return re
}


// Cached tabnas instance for the EBNF grammar above; built on first use.
let _ebnfParser: ((src: string) => EbnfProduction[]) | null = null

function getEbnfParser(): (src: string) => EbnfProduction[] {
  if (_ebnfParser) return _ebnfParser

  const { Tabnas } = require('@tabnas/parser')

  // EBNF defines its own grammar from scratch, so no grammar plugin is
  // loaded — just the bare engine with the token set retuned below.
  const j = new Tabnas({
    rule: { start: 'ebnf' },
    fixed: {
      token: {
        // `:` is not an operator on its own — `::=` is matched whole —
        // so leave colons free to appear inside names.
        '#CL': null,

        // `[` `]` `{` `}` stay declared so the text matcher treats them
        // as delimiters (without that, `A[a-z]` would lex as one long
        // bareword). A well-formed class is claimed by `#CC` before the
        // fixed matcher is reached; these tins therefore only ever
        // surface on malformed input, where they drive a named error.
        '#OS': '[',
        '#CS': ']',
        '#OB': '{',
        '#CB': '}',

        // ISO 14977 concatenation and termination. Accepted, ignored.
        '#CA': ',',
        '#SC': ';',

        '#ALT': '|',
        '#STAR': '*',
        '#PLUS': '+',
        '#QM': '?',
        '#LP': '(',
        '#RP': ')',
      },
    },
    match: {
      token: {
        // `::=` (W3C) and `=` (ISO). Fixed tokens would do the same job,
        // but as one matcher the two spellings share a single tin and
        // the rule table needs only one `#DEFOP` lookahead.
        '#DEF': eager(/^::=/),
        '#DEFE': eager(/^=/),
        // A complete character class. Bounded to one line so an
        // unclosed `[` fails at the bracket instead of swallowing the
        // rest of the grammar.
        '#CC': eager(/^\[\^?[^\]\n]*\]/),
        // A standalone code point: `#x41`, `#xD7FF`.
        '#HX': eager(/^#x[0-9a-fA-F]+/i),
        // The subtraction operator. Only reachable when `-` starts a
        // token; inside a name the text matcher has already eaten it.
        '#SUB': eager(/^-/),
        // ISO `(* … *)`, claimed before the fixed matcher sees `(`.
        '#CM': isoCommentMatcher,
      },
    },
    tokenSet: {
      // The two definition operators, so `NAME ::=` and `NAME =`
      // production headers share one lookahead pattern.
      DEFOP: ['#DEF', '#DEFE'],
    },
    value: {
      // A W3C symbol name is an ordinary word, and `true`, `false` and
      // `null` are ordinary names — several published grammars define
      // rules with exactly those names. With the engine's default
      // keyword-value lexing they would arrive as `#VL` value tokens
      // instead of `#TX` barewords and no such grammar would compile.
      lex: false,
    },
    number: {
      // EBNF has no numeric literals. Digits only ever appear inside
      // names (`Char32`), code points (`#x41`) and character classes,
      // all of which are claimed by a matcher or the text matcher.
      lex: false,
    },
    string: {
      // W3C EBNF quotes are `'` and `"`, interchangeable. Backticks
      // are not EBNF, and no literal spans lines.
      chars: '\'"',
      multiChars: '',
      // W3C EBNF defines NO escape sequences inside a literal: a
      // backslash is the backslash character. The engine has no shared
      // "escaping off" switch, so point the escape character at DEL
      // (%x7F), which no EBNF literal contains. Without this, `"\"` —
      // a one-character literal, and the one every grammar for a
      // language with escapes needs — would swallow its closing quote.
      escapeChar: '\x7F',
    },
    comment: {
      def: {
        // `#` starts a code point, not a comment.
        hash: null as any,
        // `//` is not an EBNF comment, and leaving it on would make a
        // grammar that uses `/` as a terminal harder to read than it
        // needs to be.
        slash: null as any,
        // W3C EBNF comments.
        multi: {
          line: false,
          start: '/' + '*',
          end: '*' + '/',
          lex: true,
          eatline: false,
        },
      },
    },
  })

  // Drop the default JSON rules — they would otherwise compete with
  // ours for the starting token set.
  const existing = j.rule()
  for (const name of Object.keys(existing)) {
    j.rule(name, null)
  }

  for (const name of Object.keys(ebnfRules)) {
    const spec = ebnfRules[name]
    j.rule(name, (rs: any) => {
      if (spec.bo) rs.bo(spec.bo)
      if (spec.bc) rs.bc(spec.bc)
      if (spec.open) rs.open(spec.open)
      if (spec.close) rs.close(spec.close)
    })
  }

  _ebnfParser = (src: string) => j.parse(src) as EbnfProduction[]
  return _ebnfParser
}


// Parse EBNF source into the grammar IR.
function parseEbnf(src: string): EbnfGrammar {
  const parser = getEbnfParser()
  let productions: EbnfProduction[]
  try {
    productions = parser(src) ?? []
  } catch (e: any) {
    // A named rejection thrown from inside a rule action is already the
    // diagnostic we want — pass it straight through.
    if (e instanceof EbnfParseError) throw e

    // TabnasError carries `lineNumber` / `columnNumber`.
    const line = e?.lineNumber ?? e?.row
    const column = e?.columnNumber ?? e?.col
    const loc = (null != line && null != column)
      ? ` at line ${line}, column ${column}`
      : ''
    const raw = e?.message ? String(e.message).split('\n')[0] : String(e)
    throw new EbnfParseError(
      `ebnf: parse error${loc}: ${raw}`,
      { line, column },
      e,
    )
  }

  if (!Array.isArray(productions) || 0 === productions.length) {
    throw new EbnfParseError('ebnf: no productions found')
  }

  checkDuplicates(productions)
  checkNullableAlts(productions)

  return { productions }
}


// EBNF has no incremental-alternative operator (ABNF's `=/`), so a
// repeated symbol is a mistake — and a silent one, since the compiler
// would take whichever definition it saw last.
function checkDuplicates(prods: EbnfProduction[]): void {
  const seen = new Set<string>()
  for (const p of prods) {
    if (seen.has(p.name)) {
      throw new EbnfParseError(
        `ebnf: rule '${p.name}' is defined more than once. EBNF has no ` +
        `incremental-alternatives operator; combine the definitions into ` +
        `one production with '|' between the alternatives.`,
      )
    }
    seen.add(p.name)
  }
}


// Reject a production with two or more alternatives that can each match
// nothing.
//
// This is the one ambiguity the front-end can rule out soundly and
// cheaply: if two alternatives both derive the empty string then the
// grammar itself is ambiguous — no amount of lookahead distinguishes
// them, because there is nothing to look at. The shared compiler emits
// a dispatch for such a rule anyway and the result mis-parses, so it is
// caught here instead.
//
// This is NOT a general ambiguity check. See the README: the engine is
// deterministic with bounded, grammar-declared lookahead plus a
// mark/rewind probe for one optional-prefix shape, and a grammar that
// exceeds that fails either in `@tabnas/bnf` (with a named error) or at
// parse time on the inputs that need the extra lookahead.
function checkNullableAlts(prods: EbnfProduction[]): void {
  const nullable = new Set<string>()

  const elNullable = (el: EbnfElement): boolean => {
    switch (el.kind) {
      case 'opt':
      case 'star':
        return true
      case 'plus':
        return elNullable(el.inner)
      case 'rep':
        return 0 === el.min
      case 'group':
        return el.alts.some((a) => a.every(elNullable))
      case 'ref':
        return nullable.has(el.name)
      default:
        return false
    }
  }
  const altNullable = (alt: EbnfSequence): boolean => alt.every(elNullable)

  // Least fixed point: a rule is nullable if any alternative is, and
  // that can only become true as more rules are found nullable.
  let changed = true
  while (changed) {
    changed = false
    for (const p of prods) {
      if (nullable.has(p.name)) continue
      if (p.alts.some(altNullable)) {
        nullable.add(p.name)
        changed = true
      }
    }
  }

  for (const p of prods) {
    const n = p.alts.filter(altNullable).length
    if (1 < n) {
      throw new EbnfParseError(
        `ebnf: rule '${p.name}' has ${n} alternatives that each match ` +
        `nothing, so the grammar is ambiguous: an empty input has more ` +
        `than one derivation and no lookahead can choose between them. ` +
        `Make at most one alternative optional — '(A | B)?' rather than ` +
        `'A? | B?'.`,
      )
    }
  }
}


// Convert EBNF source into a tabnas grammar spec: parse this notation,
// then hand the IR to the shared compiler. `tag` defaults to 'ebnf' so
// every emitted alt carries this front-end's group tag.
function ebnf(src: string, opts?: EbnfConvertOptions): GrammarSpec {
  const grammar = parseEbnf(src)
  try {
    return emitGrammarSpec(grammar, { ...opts, tag: opts?.tag ?? 'ebnf' })
  } catch (e: any) {
    if (e instanceof EbnfParseError) throw e
    // Restamp the shared compiler's package prefix so a caller sees one
    // consistent `ebnf:` on every diagnostic this package raises. Both
    // spellings are matched because the compiler's own prefix has moved
    // (`abnf:` while it lived inside `@tabnas/abnf`, `bnf:` since the
    // extraction). The rest of the message — which names the offending
    // rule — is left exactly as the compiler wrote it.
    const msg = String(e?.message ?? e)
    throw new EbnfCompileError(msg.replace(/^a?bnf: /, 'ebnf: '), e)
  }
}


export {
  ebnf,
  parseEbnf,
  emitGrammarSpec,
  eliminateLeftRecursion,
  ebnfRules,
  EbnfParseError,
  EbnfCompileError,
}

export type {
  EbnfConvertOptions,
  EbnfElement,
  EbnfSequence,
  EbnfProduction,
  EbnfGrammar,
}
