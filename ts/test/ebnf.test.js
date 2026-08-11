/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */
'use strict'

const { describe, it } = require('node:test')
const assert = require('node:assert')
const Fs = require('node:fs')
const Path = require('node:path')

const { Tabnas } = require('@tabnas/parser')
const {
  ebnf: ebnfPlugin,
  ebnfConvert: ebnf,
  toSpec,
  parseEbnf,
  eliminateLeftRecursion,
  EbnfParseError,
  EbnfCompileError,
} = require('..')

// No grammar plugin: EBNF supplies its own grammar via j.ebnf(...).
const tn = new Tabnas({ plugins: [ebnfPlugin] })

const FIXTURES = Path.join(__dirname, 'grammar')

function loadFixture(name) {
  return Fs.readFileSync(Path.join(FIXTURES, name)).toString()
}


// Strip emitter-injected action references from a spec so tests can
// assert the structural shape without pinning the action identities.
function stripActions(alt) {
  if (Array.isArray(alt)) return alt.map(stripActions)
  if (alt && typeof alt === 'object') {
    const out = {}
    for (const [k, v] of Object.entries(alt)) {
      if (k === 'a') continue
      out[k] = stripActions(v)
    }
    return out
  }
  return alt
}


describe('ebnf', () => {

  describe('converter', () => {

    it('emits spec for alternation of terminals', () => {
      const spec = ebnf('Greet ::= "hi" | "hello"')
      // A synthetic `__start__` wrapper ensures end-of-source is
      // always consumed; it pushes the user's start rule.
      assert.equal(spec.options.rule.start, '__start__')
      assert.deepEqual(stripActions(spec.rule.__start__.open), [
        { p: 'Greet', g: 'ebnf' },
      ])
      assert.deepEqual(stripActions(spec.rule.Greet.open), [
        { s: '#HI', g: 'ebnf' },
        { s: '#HELLO', g: 'ebnf' },
      ])
    })


    it('W3C literals are case-sensitive, so they emit FIXED tokens', () => {
      // The contrast with `@tabnas/abnf` is the point: RFC 5234 strings
      // are case-INsensitive and compile to case-folding regex matchers,
      // while W3C strings match exactly and compile to fixed tokens.
      const spec = ebnf('Greet ::= "hi" | "hello"')
      assert.deepEqual(spec.options.fixed.token, { '#HI': 'hi', '#HELLO': 'hello' })
      assert.equal(spec.options.match, undefined)
    })


    it('emits a single N-token alt for a terminal sequence', () => {
      const spec = ebnf('Pair ::= "a" "b" "c"')
      assert.deepEqual(stripActions(spec.rule.Pair.open), [
        { s: '#A #B #C', g: 'ebnf' },
      ])
    })


    it('honours override of start rule', () => {
      const spec = ebnf('A ::= "x"\nB ::= "y"', { start: 'B' })
      assert.deepEqual(stripActions(spec.rule.__start__.open), [
        { p: 'B', g: 'ebnf' },
      ])
    })


    it('tags every emitted alt with the ebnf group tag', () => {
      const spec = ebnf('A ::= "x" | "y"')
      for (const alt of spec.rule.A.open) {
        assert.equal(alt.g, 'ebnf')
      }
    })


    it('character classes emit a regex match token', () => {
      const spec = ebnf('D ::= [0-9]')
      const entries = Object.entries(spec.options.match.token)
      assert.equal(entries.length, 1)
      assert.match(String(entries[0][1]), /\^\[\\u0030-\\u0039\]/)
    })


    it('toSpec is the same conversion as ebnfConvert', () => {
      assert.deepEqual(
        Object.keys(toSpec('A ::= "x"').rule),
        Object.keys(ebnf('A ::= "x"').rule),
      )
    })

  })


  describe('IR shape', () => {

    it('builds terms, refs and sequences', () => {
      const g = parseEbnf('A ::= "x" B\nB ::= "y"')
      assert.deepEqual(g.productions[0], {
        name: 'A',
        alts: [[
          { kind: 'term', literal: 'x', caseSensitive: true },
          { kind: 'ref', name: 'B' },
        ]],
      })
    })


    it('builds one alt per `|` branch', () => {
      const g = parseEbnf('A ::= "x" | "y" | "z"')
      assert.equal(g.productions[0].alts.length, 3)
    })


    it('maps postfix operators onto opt / star / plus', () => {
      const g = parseEbnf('A ::= "x"? "y"* "z"+')
      assert.deepEqual(g.productions[0].alts[0].map((e) => e.kind),
        ['opt', 'star', 'plus'])
    })


    it('maps parentheses onto a group of alts', () => {
      const g = parseEbnf('A ::= ( "x" | "y" ) "z"')
      const [group, term] = g.productions[0].alts[0]
      assert.equal(group.kind, 'group')
      assert.equal(group.alts.length, 2)
      assert.equal(term.kind, 'term')
    })


    it('stacks postfix operators outermost-last', () => {
      // Not W3C EBNF (which never stacks them), but reading a stack is
      // strictly more permissive than failing on one.
      const g = parseEbnf('A ::= ( "x" )*?')
      const [el] = g.productions[0].alts[0]
      assert.equal(el.kind, 'opt')
      assert.equal(el.inner.kind, 'star')
    })


    it('maps a character class onto a regex element', () => {
      const g = parseEbnf('A ::= [a-z]')
      assert.deepEqual(g.productions[0].alts[0][0], {
        kind: 'regex', pattern: '[\\u0061-\\u007a]', flags: '',
      })
    })


    it('maps a negated character class onto a negated regex', () => {
      // `u` even with no astral member written: a negated class matches
      // the COMPLEMENT of its members, and that always contains every
      // astral code point. Without it the matcher would consume one
      // UTF-16 surrogate rather than one character.
      const g = parseEbnf('A ::= [^<&]')
      assert.deepEqual(g.productions[0].alts[0][0], {
        kind: 'regex', pattern: '[^\\u003c\\u0026]', flags: 'u',
      })
      const el = g.productions[0].alts[0][0]
      const m = '\u{1F600}'.match(new RegExp('^' + el.pattern, el.flags))
      assert.equal(m[0], '\u{1F600}', 'must consume the whole character')
    })


    it('reads #xNN inside a class, as ranges and as members', () => {
      const g = parseEbnf('A ::= [#x20-#x7E]\nB ::= [#x9#xA#xD]')
      assert.equal(g.productions[0].alts[0][0].pattern, '[\\u0020-\\u007e]')
      assert.equal(g.productions[1].alts[0][0].pattern,
        '[\\u0009\\u000a\\u000d]')
    })


    it('treats a trailing or leading hyphen in a class as a member', () => {
      const g = parseEbnf('A ::= [-+]\nB ::= [a-]')
      assert.equal(g.productions[0].alts[0][0].pattern, '[\\u002d\\u002b]')
      assert.equal(g.productions[1].alts[0][0].pattern, '[\\u0061\\u002d]')
    })


    it('switches a class above the BMP to \\u{…} and the u flag', () => {
      // `\uXXXX` only reaches U+FFFF, so an astral endpoint has to be
      // spelled `\u{…}`, which in turn requires the `u` flag.
      const g = parseEbnf('A ::= [#x10000-#x10FFFF]')
      assert.deepEqual(g.productions[0].alts[0][0], {
        kind: 'regex', pattern: '[\\u{10000}-\\u{10ffff}]', flags: 'u',
      })
    })


    it('maps a standalone #xNN onto a term', () => {
      const g = parseEbnf('A ::= #x41')
      assert.deepEqual(g.productions[0].alts[0][0], {
        kind: 'term', literal: 'A', caseSensitive: true,
      })
    })


    it('accepts both quote styles for a literal', () => {
      const g = parseEbnf(`A ::= 'a"b' | "c'd"`)
      assert.equal(g.productions[0].alts[0][0].literal, 'a"b')
      assert.equal(g.productions[0].alts[1][0].literal, "c'd")
    })


    it('has no escape sequences inside a literal', () => {
      // W3C EBNF defines none, so a backslash is the backslash
      // character and `\n` is two characters.
      const g = parseEbnf('A ::= "\\n"')
      assert.equal(g.productions[0].alts[0][0].literal, '\\n')
      assert.equal(g.productions[0].alts[0][0].literal.length, 2)
    })

  })


  describe('W3C constructs, end to end', () => {

    it('alternation: either branch', () => {
      const j = tn.make()
      j.ebnf('Greet ::= "hi" | "hello"')
      assert.equal(j.parse('hi').src, 'hi')
      assert.equal(j.parse('hello').src, 'hello')
      assert.throws(() => j.parse('nope'), /unexpected/)
    })


    it('concatenation: both, in order', () => {
      const j = tn.make()
      j.ebnf('Pair ::= "a" "b"')
      assert.equal(j.parse('ab').src, 'ab')
      assert.throws(() => j.parse('a'), /unexpected/)
      assert.throws(() => j.parse('ba'), /unexpected/)
    })


    it('optional: presence or absence', () => {
      const j = tn.make()
      j.ebnf('G ::= "hi" "there"?')
      assert.doesNotThrow(() => j.parse('hi'))
      assert.doesNotThrow(() => j.parse('hi there'))
      assert.throws(() => j.parse('hi nope'), /unexpected/)
    })


    it('star: zero or more', () => {
      const j = tn.make()
      j.ebnf('G ::= "x"* "end"')
      assert.doesNotThrow(() => j.parse('end'))
      assert.doesNotThrow(() => j.parse('x end'))
      assert.doesNotThrow(() => j.parse('x x x end'))
      assert.throws(() => j.parse('y end'), /unexpected/)
    })


    it('plus: one or more', () => {
      const j = tn.make()
      j.ebnf('G ::= "x"+ "end"')
      assert.doesNotThrow(() => j.parse('x end'))
      assert.doesNotThrow(() => j.parse('x x x end'))
      assert.throws(() => j.parse('end'), /unexpected/)
    })


    it('grouping: the group binds before what follows it', () => {
      const j = tn.make()
      j.ebnf('G ::= ( "a" | "b" ) "c"')
      assert.doesNotThrow(() => j.parse('ac'))
      assert.doesNotThrow(() => j.parse('bc'))
      assert.throws(() => j.parse('cc'), /unexpected/)
    })


    it('grouping composes with repetition', () => {
      const j = tn.make()
      j.ebnf('G ::= ( "a" "b" )+ "end"')
      assert.doesNotThrow(() => j.parse('a b end'))
      assert.doesNotThrow(() => j.parse('a b a b end'))
      assert.throws(() => j.parse('end'), /unexpected/)
    })


    it('character class: one character from the set', () => {
      const j = tn.make()
      j.ebnf('Digits ::= [0-9]+')
      assert.equal(j.parse('1').src, '1')
      assert.equal(j.parse('1234').src, '1234')
      assert.throws(() => j.parse('abc'), /unexpected/)
    })


    it('character class: enumerated members and #xNN ranges', () => {
      const j = tn.make()
      j.ebnf('Hex ::= [0-9abcdef]+')
      assert.doesNotThrow(() => j.parse('1f'))
      assert.throws(() => j.parse('g'), /unexpected/)

      const k = tn.make()
      k.ebnf('P ::= [#x30-#x39]+')
      assert.doesNotThrow(() => k.parse('42'))
      assert.throws(() => k.parse('x'), /unexpected/)
    })


    it('negated character class: anything but the members', () => {
      const j = tn.make()
      j.ebnf('S ::= "<" [^<>]+ ">"')
      assert.doesNotThrow(() => j.parse('<abc>'))
      assert.throws(() => j.parse('<a<b>'), /unexpected/)
    })


    it('standalone #xNN matches that code point', () => {
      const j = tn.make()
      j.ebnf('S ::= #x41 #x42')
      assert.doesNotThrow(() => j.parse('AB'))
      assert.throws(() => j.parse('ab'), /unexpected/)
    })


    it('literals are case-sensitive', () => {
      const j = tn.make()
      j.ebnf('S ::= "GET"')
      assert.doesNotThrow(() => j.parse('GET'))
      assert.throws(() => j.parse('get'), /unexpected/)
      assert.throws(() => j.parse('Get'), /unexpected/)
    })


    it('built-in lexer tokens are referable by bare name', () => {
      // Inherited from the shared compiler: TX / NR / ST / VL name the
      // engine's own lexer tokens. Not W3C EBNF, but the only sane way
      // to write a token-level grammar for this engine.
      const j = tn.make()
      j.ebnf('Pair ::= "{" Key ":" Val "}"\nKey ::= TX\nVal ::= NR | ST')
      assert.deepEqual(j.parse('{a:1}').kids.map((k) => k.rule), ['Key', 'Val'])
      assert.equal(j.parse('{a:"x"}').kids[1].src, '"x"')
    })


    it('comments are ignored', () => {
      const j = tn.make()
      j.ebnf('/* leading */ G ::= "a" /* inline */ "b" /* trailing */')
      assert.equal(j.parse('ab').src, 'ab')
    })

  })


  describe('ISO 14977 spellings that are accepted', () => {

    it('`=` works as the definition operator', () => {
      const j = tn.make()
      j.ebnf('greet = "hi" | "hello"')
      assert.equal(j.parse('hi').src, 'hi')
    })


    it('`;` terminates a production', () => {
      const g = parseEbnf('a = "x" ;\nb = "y" ;')
      assert.deepEqual(g.productions.map((p) => p.name), ['a', 'b'])
    })


    it('a `;` on the last production only is still fine', () => {
      const g = parseEbnf('a ::= "x"\nb ::= "y" ;')
      assert.deepEqual(g.productions.map((p) => p.name), ['a', 'b'])
    })


    it('`,` is accepted as an explicit concatenation separator', () => {
      // Juxtaposition already means sequence here, so the comma adds
      // nothing to the IR — the two spellings must agree exactly.
      assert.deepEqual(
        parseEbnf('a ::= "x" , "y"').productions,
        parseEbnf('a ::= "x" "y"').productions,
      )
    })


    it('(* … *) comments are ignored', () => {
      const j = tn.make()
      j.ebnf('(* leading *) g = "a" (* inline *) "b"')
      assert.equal(j.parse('ab').src, 'ab')
    })


    it('a multi-line (* … *) comment does not shift error line numbers', () => {
      // The ISO comment is claimed by a match-token matcher, which does
      // its own row bookkeeping; without that every line number after a
      // multi-line comment would be wrong.
      let err
      try {
        parseEbnf('(* one\ntwo\nthree *)\na ::= "x"\nb ::= { "y" }')
      } catch (e) { err = e }
      assert.ok(err instanceof EbnfParseError)
      assert.equal(err.line, 5)
    })


    it('the ISO-spelled fixture parses the same language as the W3C one', () => {
      const w3c = tn.make()
      w3c.ebnf(loadFixture('expr.ebnf'))
      const iso = tn.make()
      iso.ebnf(loadFixture('iso-style.ebnf'))
      for (const input of ['1', '1+2', '1+2*3', '(1+2)*3']) {
        assert.equal(w3c.parse(input).src, iso.parse(input).src)
      }
    })

  })


  describe('documented rejections', () => {

    // Each of these is an entry in the README's "not supported" list.
    // The test asserts both that the construct is refused and that the
    // message says which construct it was.

    it('rejects subtraction, naming the operator', () => {
      assert.throws(() => ebnf('A ::= B - C\nB ::= "b"\nC ::= "c"'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /subtraction \('-'\) at line 1/)
        assert.match(e.message, /no difference operator/)
        return true
      })
    })


    it('rejects subtraction written without a trailing space', () => {
      assert.throws(() => ebnf('A ::= B -C\nB ::= "b"'), /subtraction/)
    })


    it('rejects ISO special sequences, naming them', () => {
      assert.throws(() => ebnf('A ::= ? anything at all ?'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /special sequences/)
        return true
      })
    })


    it('rejects ISO bracket repetition, naming the alternative', () => {
      assert.throws(() => ebnf('A ::= { "x" }'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /ISO 14977 bracket repetition/)
        assert.match(e.message, /write 'A\*'/)
        return true
      })
    })


    it('rejects a stray bracket, explaining the character-class reading', () => {
      assert.throws(() => ebnf('A ::= [a-z'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /stray '\['/)
        assert.match(e.message, /character class/)
        return true
      })
    })


    it('rejects ABNF-style prefix repetition, naming the direction', () => {
      assert.throws(() => ebnf('A ::= *"x"'), /Repetition in this dialect is postfix/)
      assert.throws(() => ebnf('A ::= +"x"'), /Repetition in this dialect is postfix/)
    })


    it('rejects an unclosed group', () => {
      assert.throws(() => ebnf('A ::= ( "x"'), /unclosed group/)
    })


    it('rejects an empty literal', () => {
      assert.throws(() => ebnf('A ::= ""'), /empty string literal/)
    })


    it('rejects an empty or reversed character class', () => {
      assert.throws(() => ebnf('A ::= []'), /empty character class/)
      assert.throws(() => ebnf('A ::= [z-a]'), /reversed range/)
    })


    it('rejects a code point above U+10FFFF', () => {
      assert.throws(() => ebnf('A ::= #x110000'), /not a Unicode code point/)
      assert.throws(() => ebnf('A ::= [#x0-#x110000]'), /not a Unicode code point/)
    })


    it('rejects a malformed symbol name, naming it', () => {
      assert.throws(() => ebnf('Foo! ::= "x"'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /'Foo!' at line 1, column 1 is not a valid symbol name/)
        return true
      })
    })


    it('rejects a rule defined twice, naming the rule', () => {
      assert.throws(() => ebnf('A ::= "x"\nA ::= "y"'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /rule 'A' is defined more than once/)
        return true
      })
    })


    it('rejects two alternatives that both match nothing, naming the rule', () => {
      // Genuinely ambiguous: the empty input has two derivations, and
      // no amount of lookahead distinguishes them.
      assert.throws(() => ebnf('A ::= "x"? | "y"?'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /rule 'A' has 2 alternatives that each match nothing/)
        return true
      })
      // One optional alternative is fine.
      assert.doesNotThrow(() => ebnf('A ::= "x"? | "y"'))
    })


    it('rejects two ε-deriving alternatives inside a group', () => {
      // A group is a choice like any other, so it carries the same
      // ambiguity. The check used to look at production-level alts
      // only, so every grouped spelling slipped through and mis-parsed.
      assert.throws(() => ebnf('A ::= ("x"? | "y"?) "y"'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(
          e.message,
          /a group in rule 'A' has 2 alternatives that each match nothing/)
        return true
      })
      // Nested groups are reached too.
      assert.throws(() => ebnf('A ::= (("x"? | "y"?) "z") "w"'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        return true
      })
      // One nullable branch in a group is fine.
      assert.doesNotThrow(() => ebnf('A ::= ("x"? | "y") "z"'))
    })


    it('rejects an empty alternative rather than reading it as epsilon', () => {
      // Both dialects require an expression each side of `|`, and this
      // package refuses an empty literal, so accepting ε here made the
      // accepted language wider than the documented one.
      for (const src of ['A ::= | "x"', 'A ::= "x" |', 'A = ;', 'A ::= ()']) {
        assert.throws(() => ebnf(src), (e) => {
          assert.ok(e instanceof EbnfParseError)
          assert.match(e.message, /empty alternative/)
          return true
        }, `expected ${JSON.stringify(src)} to be rejected`)
      }
      assert.doesNotThrow(() => ebnf('A ::= "x" | "y"'))
    })


    it('rejects a leading comma, which ISO does not define', () => {
      // ISO's comma separates concatenated items; it is not a prefix.
      assert.throws(() => ebnf('A = , "x";'), (e) => {
        assert.ok(e instanceof EbnfParseError)
        assert.match(e.message, /leading comma/)
        return true
      })
      // Between two items it is the documented ISO spelling.
      assert.doesNotThrow(() => ebnf('A = "x" , "y";'))
    })


    it('accepts only the lowercase #xNN code-point spelling', () => {
      // W3C specifies `#x`; `#X41` is not among the ISO spellings this
      // package documents accepting, so it must not be case-folded.
      assert.doesNotThrow(() => ebnf('A ::= #x41'))
      assert.throws(() => ebnf('A ::= #X41'))
      assert.doesNotThrow(() => ebnf('A ::= [#x20-#x7E]'))
      assert.throws(() => ebnf('A ::= [#X20-#X7E]'))
      // Hex digits themselves may be either case.
      assert.doesNotThrow(() => ebnf('A ::= #xD7FF'))
      assert.doesNotThrow(() => ebnf('A ::= #xd7ff'))
    })


    it('rejects a reference to an undefined rule, naming both rules', () => {
      assert.throws(() => ebnf('A ::= B'), (e) => {
        assert.ok(e instanceof EbnfCompileError)
        assert.match(e.message, /ebnf: rule 'A' references unknown rule 'B'/)
        return true
      })
    })


    it('rejects a purely left-recursive rule, naming the rule', () => {
      assert.throws(() => ebnf('A ::= A "x"'), (e) => {
        assert.ok(e instanceof EbnfCompileError)
        assert.match(e.message, /ebnf: rule 'A' is purely left-recursive/)
        return true
      })
    })


    it('rejects a source with no productions', () => {
      assert.throws(() => ebnf('/* nothing but a comment */'),
        /no productions found/)
      assert.throws(() => ebnf(''), /no productions found/)
    })


    it('reports line and column on a parse error', () => {
      let err
      try { parseEbnf('A ::= "x"\nB ::= { "y" }') } catch (e) { err = e }
      assert.ok(err instanceof EbnfParseError)
      assert.equal(err.line, 2)
      assert.equal(typeof err.column, 'number')
    })

  })


  describe('left recursion', () => {

    it('rewrites P ::= P a | b into P ::= b (a)*', () => {
      const g = parseEbnf('E ::= E "+" T | T\nT ::= "1"')
      const r = eliminateLeftRecursion(g)
      const e = r.productions.find((p) => p.name === 'E')
      assert.equal(e.alts.length, 1)
      const [seed, tail] = e.alts[0]
      assert.equal(seed.kind, 'term')
      assert.equal(tail.kind, 'star')
    })


    it('a left-recursive grammar parses a whole chain', () => {
      const j = tn.make()
      j.ebnf('Expr ::= Expr "+" Term | Term\nTerm ::= NR')
      assert.equal(j.parse('1').rule, 'Expr')
      assert.equal(j.parse('1+2+3').src, '1+2+3')
      assert.deepEqual(j.parse('1+2+3').kids.map((k) => k.rule),
        ['Term', 'Term'])
    })

  })


  describe('realistic grammars', () => {

    it('expr.ebnf parses arithmetic with precedence', () => {
      const j = tn.make()
      j.ebnf(loadFixture('expr.ebnf'))
      assert.equal(j.parse('1').src, '1')
      assert.equal(j.parse('1+2').src, '1+2')
      assert.equal(j.parse('1 + 2 - 3 * 4 / 5').src, '1+2-3*4/5')
      assert.equal(j.parse('(1+2)*3').src, '(1+2)*3')
      assert.throws(() => j.parse('1 +'), /unexpected/)
      assert.throws(() => j.parse('* 1'), /unexpected/)
    })


    it('expr.ebnf nests Term under Expr and Factor under Term', () => {
      const j = tn.make()
      j.ebnf(loadFixture('expr.ebnf'))
      const out = j.parse('1+2*3')
      assert.equal(out.rule, 'Expr')
      // The leading operand folds into the repeat's parent, so the one
      // surviving child is the `2*3` Term.
      assert.deepEqual(out.kids.map((k) => k.rule), ['Term'])
      assert.equal(out.kids[0].src, '2*3')
      assert.deepEqual(out.kids[0].kids.map((k) => k.rule), ['Factor'])
    })


    it('json-subset.ebnf parses nested JSON', () => {
      const j = tn.make()
      j.ebnf(loadFixture('json-subset.ebnf'))
      for (const input of [
        '1', '"a"', 'true', 'false', 'null', '{}', '[]',
        '{"a":1}', '[1,2,3]', '{"a":[1,{"b":null}]}',
      ]) {
        assert.equal(j.parse(input).src.replace(/\s/g, ''),
          input.replace(/\s/g, ''), input)
      }
      assert.throws(() => j.parse('{"a" 1}'), /unexpected/)
      assert.throws(() => j.parse('[1,]'), /unexpected/)
    })


    it('json-subset.ebnf builds a tree over the rules that survive', () => {
      // Not every production becomes a node: the shared compiler folds
      // a rule whose body is a single token segment into its caller, so
      // `Object` and `Array` (each reached through a one-reference
      // alternative of `Value`) do not appear. The nodes that do appear
      // carry the source they matched.
      const j = tn.make()
      j.ebnf(loadFixture('json-subset.ebnf'))
      const out = j.parse('{"a":[1,2]}')
      assert.equal(out.rule, 'Json')

      const value = out.kids[0]
      assert.equal(value.rule, 'Value')
      assert.equal(value.src, '{"a":[1,2]}')

      const member = value.kids[0]
      assert.equal(member.rule, 'Member')
      assert.equal(member.src, '"a":[1,2]')

      const arr = member.kids[0]
      assert.equal(arr.rule, 'Value')
      assert.deepEqual(arr.kids.map((k) => k.src), ['1', '2'])
    })


    it('name.ebnf parses XML-style names character by character', () => {
      const j = tn.make()
      j.ebnf(loadFixture('name.ebnf'))
      assert.equal(j.parse('abc').src, 'abc')
      assert.equal(j.parse('_x-1.2').src, '_x-1.2')
      assert.throws(() => j.parse('1abc'), /unexpected/)
    })

  })


  describe('plugin', () => {

    it('installs the grammar on the instance', () => {
      const j = tn.make()
      const spec = j.ebnf('A ::= "x"')
      assert.equal(spec.options.rule.start, '__start__')
      assert.equal(j.parse('x').rule, 'A')
    })


    it('toSpec builds without installing', () => {
      const j = tn.make()
      assert.deepEqual(Object.keys(j.rule()), [])
      const spec = j.ebnf.toSpec('A ::= "x"')
      assert.ok(spec.rule.A)
      // The spec was built but not installed, so the instance still has
      // no rules at all.
      assert.deepEqual(Object.keys(j.rule()), [])
      // Whereas installing it does add them.
      j.ebnf('A ::= "x"')
      assert.ok(Object.keys(j.rule()).includes('A'))
    })


    it('each make() gets its own grammar', () => {
      const a = tn.make()
      a.ebnf('A ::= "x"')
      const b = tn.make()
      b.ebnf('B ::= "y"')
      assert.equal(a.parse('x').rule, 'A')
      assert.equal(b.parse('y').rule, 'B')
      assert.throws(() => a.parse('y'))
    })

  })


  describe('the bounded-lookahead limit', () => {

    // The engine is deterministic with bounded lookahead, so a grammar
    // that defers its decision behind an unbounded prefix used to
    // compile but then fail on the inputs that needed the extra
    // lookahead. The compiler now left-factors such alternatives
    // automatically — these pin that both spellings parse.

    it('an unbounded shared prefix is left-factored automatically', () => {
      const j = tn.make()
      j.ebnf('S ::= L "x" | L "y"\nL ::= "a"+')
      assert.doesNotThrow(() => j.parse('a x'))
      assert.doesNotThrow(() => j.parse('a y'))
      assert.doesNotThrow(() => j.parse('a a x'))
      // The `y` branch needs a decision past an unbounded run of `a`.
      assert.doesNotThrow(() => j.parse('a a y'))
    })


    it('left-factoring by hand works the same', () => {
      const j = tn.make()
      j.ebnf('S ::= L ( "x" | "y" )\nL ::= "a"+')
      assert.doesNotThrow(() => j.parse('a a a y'))
    })


    // What remains out of reach, pinned so the suite goes red the day
    // a future compiler handles it (and this section plus the docs
    // then get rewritten, as happened to the case above). Left
    // factoring is structural: distinct multi-alternative rules
    // spelling the same unbounded prefix cannot be merged, and inside
    // each the continue-vs-exit choice on the last `a` needs sight of
    // what follows the run — beyond any bounded token lookahead. The
    // dispatch prefixes decide shallow inputs; deep ones still fail.

    it('a shared prefix behind distinct recursive rules is still the limit', () => {
      const j = tn.make()
      j.ebnf('S ::= A "x" | B "y"\nA ::= "a" A | "a"\nB ::= "a" B | "a"')
      assert.doesNotThrow(() => j.parse('a x'))
      assert.doesNotThrow(() => j.parse('a y'))
      assert.throws(() => j.parse('a a x'), /unexpected/)
      assert.throws(() => j.parse('a a a a a y'), /unexpected/)
    })

  })

})
