# How-to guide

Task-shaped recipes for `@tabnas/ebnf`. For an introduction see
[tutorial.md](tutorial.md); for the exact API and dialect see
[reference.md](reference.md).

## Compile a grammar and install it

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Greet ::= "hi" | "hello"`)

tn.parse('hi').rule // => 'Greet'
```

`tn.ebnf(src)` converts and installs in one step. Compiling is the
expensive part, so build the instance once and reuse it.

## Compile without installing

`tn.ebnf.toSpec(src)` — or the bare `ebnfConvert(src)` / `toSpec(src)`
exports, which need no instance at all — return the `GrammarSpec`
without touching the engine.

```js
const { ebnfConvert } = require('@tabnas/ebnf')

const spec = ebnfConvert(`Greet ::= "hi" | "hello"`)
Object.keys(spec.rule).includes('Greet') // => true
```

Useful for inspecting what a grammar compiled to, or for handing the
spec to a different engine instance later.

## Choose the start rule

The first production is the start rule by default. Override it with the
`start` option — handy when a spec grammar lists its productions in
document order rather than starting with the one you want.

```js
const { ebnfConvert } = require('@tabnas/ebnf')

const spec = ebnfConvert('A ::= "x"\nB ::= "y"', { start: 'B' })
spec.rule.__start__.open[0].p // => 'B'
```

`__start__` is a synthetic wrapper the compiler adds so that
end-of-source is always consumed.

## Match one character out of a set

Character classes are the W3C dialect's way to say "one character from
here". The four forms:

```js
const { parseEbnf } = require('@tabnas/ebnf')

const g = parseEbnf(`
  Range ::= [a-z]
  Enum  ::= [abc]
  Not   ::= [^<&]
  Code  ::= [#x20-#x7E]
`)
g.productions.map((p) => p.alts[0][0].kind) // => ['regex', 'regex', 'regex', 'regex']
```

All four compile to one IR `regex` element, which the shared compiler
turns into a lexer matcher. Some details worth knowing before you copy a
class out of a specification:

- A class must open and close **on one line**.
- A class cannot contain `]` — there are no escape sequences. Write a
  literal `]` as the string `"]"`.
- A `-` at the start or end of a class is a literal hyphen (`[-+]`).
- `#xNN` inside a class is a code point, so `[#x9#xA#xD]` is
  tab/newline/return and `[#x20-#x7E]` is printable ASCII.
- A class reaching above U+FFFF switches the emitted pattern to `\u{…}`
  with the `u` flag.

## Translate a grammar out of a specification

Most W3C spec grammars paste in unchanged. The three things that
normally need editing:

**Subtraction.** `CharData ::= [^<&]* - ([^<&]* ']]>' [^<&]*)` uses the
difference operator, which is rejected. Where both sides are single
characters, a negated class says the same thing:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Body ::= [^&<]+`)

tn.parse('hello').src // => 'hello'
```

Where they are not, the subtraction has to be re-expressed as an
explicit grammar — there is no mechanical translation.

**Well-formedness constraints.** Annotations like `[ wfc: … ]` and
`[ vc: … ]` that spec listings carry alongside productions are prose,
not grammar. Delete them; a `[` outside a character class is an error.

**Character-level rules that assume no lexer.** Spec grammars are
written for a scannerless parser, so they spell out whitespace
handling. The tabnas lexer already skips whitespace between tokens,
which means a char-level `Name ::= [a-z]+` will happily join two
space-separated words. Use the built-in `TX` token where you want whole
words:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Attr  ::= Name "=" Value
Name  ::= TX
Value ::= ST | NR`)

tn.parse('id = "x"').src // => 'id="x"'
```

## Use the ISO 14977 spellings

`=`, `,`, `;` and `(* … *)` are accepted alongside the W3C forms. The
*operators* stay W3C — postfix `*`, not `{ … }`.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  (* comments in this form work too, but do not nest *)
  digits = digit , digit ;
  digit  = [0-9] ;
`)

tn.parse('42').src // => '42'
```

## Read a compile error

Errors come in two kinds, and the kind tells you where to look.

`EbnfParseError` — this front-end refused the source. It carries `line`
and `column` where a token was involved.

```js
const { parseEbnf } = require('@tabnas/ebnf')

let where = null
try { parseEbnf('A ::= "x"\nB ::= { "y" }') } catch (e) { where = [e.name, e.line] }
where // => ['EbnfParseError', 2]
```

`EbnfCompileError` — the EBNF parsed, but `@tabnas/bnf` could not
compile the resulting grammar. These name the offending **rule** rather
than a source position, because the compiler works on the IR:

```js
const { ebnfConvert } = require('@tabnas/ebnf')

let msg = null
try { ebnfConvert('A ::= B') } catch (e) { msg = e.message }
msg // => "ebnf: rule 'A' references unknown rule 'B'"
```

## Fix a grammar that compiles but will not parse

The engine is deterministic with bounded lookahead. A grammar that
defers its decision behind an unbounded prefix used to compile and then
fail on long inputs:

```
S ::= L "x" | L "y"
L ::= "a"+
```

The compiler now left-factors such alternatives automatically, so
`a a y` parses as written. The factored spelling remains the clearer
one — the shared prefix is parsed once and the decision happens after
it:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`S ::= L ( "x" | "y" )
L ::= "a"+`)

tn.parse('a a a y').src // => 'aaay'
```

The same move fixes most "it compiled but the parse is wrong" reports.
See [concepts.md](concepts.md#deterministic-with-bounded-lookahead) for
why the engine works this way.

## Inspect the IR without emitting a spec

`parseEbnf(src)` stops after the front-end's own work, returning
`{ productions: [...] }`. Useful for tooling, for diffing two dialects
of the same grammar, or for checking what an operator desugars to.

```js
const { parseEbnf } = require('@tabnas/ebnf')

const g = parseEbnf('A ::= "x" B?')
g.productions[0].alts[0].map((e) => e.kind) // => ['term', 'opt']
```
