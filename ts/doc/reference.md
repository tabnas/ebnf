# Reference

Complete API surface and dialect definition for `@tabnas/ebnf`. For an
introduction see [tutorial.md](tutorial.md); for usage recipes see
[guide.md](guide.md).

All exports come from the package root:

```js ignore
const {
  ebnf, ebnfConvert, toSpec, parseEbnf,
  emitGrammarSpec, eliminateLeftRecursion, ebnfRules,
  EbnfParseError, EbnfCompileError, VERSION,
} = require('@tabnas/ebnf')
```

## Conversion

### `ebnfConvert(src, opts?) => GrammarSpec`

Take EBNF source and return a tabnas `GrammarSpec` (with a `ref` map of
action closures, an `options` block, and a `rule` table). This is the
primary entry point. Also exported as `toSpec`, and available on an
instance as `tn.ebnf.toSpec`.

- `src: string` — the EBNF source.
- `opts?: EbnfConvertOptions` — see below.

Throws `EbnfParseError` if the source cannot be read, `EbnfCompileError`
if it reads but cannot be compiled.

### `parseEbnf(src) => EbnfGrammar`

Parse EBNF source into the grammar IR (`{ productions: [...] }`)
*without* emitting a spec. Each production is `{ name, alts }`, where
`alts` is a list of sequences of `EbnfElement`s.

```js
const { parseEbnf } = require('@tabnas/ebnf')

const g = parseEbnf('A ::= "x" B*')
g.productions[0].alts[0].map((e) => e.kind) // => ['term', 'star']
```

Throws `EbnfParseError` for malformed source, for a refused construct,
or when the source defines no productions.

### `emitGrammarSpec(grammar, opts?) => GrammarSpec`

Re-exported from `@tabnas/bnf`. Convert an already-parsed grammar into a
`GrammarSpec`. `ebnfConvert(src)` is `emitGrammarSpec(parseEbnf(src), {
tag: 'ebnf', ... })`.

### `eliminateLeftRecursion(grammar) => EbnfGrammar`

Re-exported from `@tabnas/bnf`. Rewrite direct and indirect left
recursion via Paull's algorithm, returning a new grammar. Called
internally by `emitGrammarSpec`; exported for inspection.

### `ebnfRules`

The declarative table of tabnas rules that defines the EBNF grammar
itself — the meta-grammar this package uses to read EBNF source.
Exported for introspection and tooling.

### `EbnfConvertOptions`

The shared compiler's `ConvertOptions`, unchanged.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `start` | `string` | first production | Start rule name. |
| `tag` | `string` | `'ebnf'` | Group tag stamped on every emitted alt. |
| `builtins` | `boolean` | `false` | Emit probe dispatch and tree building as engine `$`-builtin refs instead of closures, keeping the spec function-free and serializable. |
| `marks` | `boolean` | `false` | Emit a stable `m` mark per user-rule alt, enabling `@<rule>:o\|c:<mark>` user-action references. |
| `wordKeywords` | `boolean` | `false` | Treat word-like literals as whole-word keywords, so `"option"` does not match the prefix of `optional`. For tokenised, keyword-rich languages; leave off for char-level grammars. |

```js
const { ebnfConvert } = require('@tabnas/ebnf')

const spec = ebnfConvert('A ::= "x"\nB ::= "y"', { start: 'B' })
spec.rule.__start__.open[0].p // => 'B'
```

## Plugin form

### `ebnf` (tabnas Plugin)

Install with `new Tabnas({ plugins: [ebnf] })` or `tn.use(ebnf)`. Adds a
callable `ebnf` member to the instance.

#### `tn.ebnf(src, opts?) => GrammarSpec`

Convert `src` and install the resulting grammar on `tn`, returning the
spec.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf('Greet ::= "hi"')

tn.parse('hi').rule // => 'Greet'
```

#### `tn.ebnf.toSpec(src, opts?) => GrammarSpec`

Convert without installing.

### `VERSION`

This package's version, as a string. Equal to `package.json`'s
`version`; a test fails the build if the two drift.

## Errors

| Class | Raised by | Carries |
|---|---|---|
| `EbnfParseError` | this front-end — syntax errors and refused constructs | `.line`, `.column` (where a token was involved), `.cause` |
| `EbnfCompileError` | `@tabnas/bnf`, wrapped — an IR the compiler cannot lower | `.cause` |

`EbnfCompileError` messages name the offending **rule** rather than a
source position, because the compiler works on the IR and no longer has
the text. They are the shared compiler's own wording, with only its
package prefix restamped to `ebnf:` so that every diagnostic this
package raises reads the same way.

```js
const { ebnfConvert } = require('@tabnas/ebnf')

let msg = null
try { ebnfConvert('A ::= B') } catch (e) { msg = e.message }
msg // => "ebnf: rule 'A' references unknown rule 'B'"
```

## The EBNF dialect

**Primary dialect: W3C EBNF**, as defined in
[XML 1.0 §6 "Notation"](https://www.w3.org/TR/xml/#sec-notation) and
used by XPath, XQuery, XML Schema and JSONPath. Chosen because it has a
real published corpus, because every one of its operators has a
destination in the grammar IR, and because its `[…]` character class
cannot coexist with ISO/IEC 14977's `[ … ]` optional — the dialect has
to be a choice, so it is a documented one.

A small set of ISO/IEC 14977 **spellings** is accepted on top, chosen
because none can collide with the W3C reading. Accepting them does not
make this an ISO implementation.

### What is and is not supported

#### Supported

| Construct | Syntax | IR |
|---|---|---|
| Production | `Symbol ::= expr` | `Production` |
| Production, ISO spelling | `symbol = expr ;` | `Production` |
| Alternation | `A \| B` | one alt per branch |
| Concatenation | `A B` | one `Sequence` |
| Concatenation, ISO spelling | `A , B` | one `Sequence` (the comma adds nothing) |
| Grouping | `( A \| B )` | `group` |
| Optional | `A?` | `opt` |
| Zero or more | `A*` | `star` |
| One or more | `A+` | `plus` |
| Stacked postfix | `(A)*?` | nested — an extension; W3C never stacks them |
| Literal, double-quoted | `"abc"` | `term`, `caseSensitive: true` |
| Literal, single-quoted | `'abc'` | `term`, `caseSensitive: true` |
| Character class, range | `[a-z]` | `regex` |
| Character class, enumeration | `[abc]`, `[-+]` | `regex` |
| Character class, negated | `[^<&]` | `regex` |
| Character class, code points | `[#x20-#x7E]`, `[#x9#xA]` | `regex` |
| Standalone code point | `#x41` | `term` |
| Rule reference | a bare `Symbol` | `ref` |
| Built-in lexer token | `TX`, `NR`, `ST`, `VL` | `token` — an extension from the shared compiler |
| Comment, W3C | `/* … */` | ignored |
| Comment, ISO | `(* … *)` | ignored; does **not** nest |
| Left recursion | `E ::= E "+" T \| T` | rewritten to `E ::= T ( "+" T )*` |

Symbol names match `[A-Za-z_][A-Za-z0-9_.-]*`.

#### Not supported

Each of these raises a named error at conversion time. Nothing in this
list compiles to an approximation.

| Construct | Example | Why | Error class and wording |
|---|---|---|---|
| Subtraction / exception | `A - B` | The IR has no difference operator, and subtracting one language from another is not expressible over the `Element` kinds. Where both sides are single characters, use a negated class. | `EbnfParseError`: `subtraction ('-') … is not supported` |
| Special sequence | `? … ?` | ISO 14977 leaves the content undefined, so there is nothing to compile. Also catches a `?` with no element in front of it. | `EbnfParseError`: `special sequences ('? … ?') are not supported` |
| ISO repetition | `{ A }` | `A*` is this dialect's spelling. Supporting one bracket of an ISO pair while the other means something else invites grammars that are ISO everywhere except where they silently are not. | `EbnfParseError`: `ISO 14977 bracket repetition '{ … }' … is not supported` |
| ISO option | `[ A ]` | `[ … ]` is a character class here. Reported as a stray bracket, because that is what an unmatched `[` is once classes are claimed whole. | `EbnfParseError`: `stray '[' …` |
| ABNF prefix repetition | `*A`, `1*A` | Repetition is postfix in this dialect. | `EbnfParseError`: `Repetition in this dialect is postfix` |
| Empty literal | `""` | Matches nothing; W3C EBNF has no epsilon terminal. | `EbnfParseError`: `empty string literal … matches nothing` |
| Empty or reversed class | `[]`, `[z-a]` | Matches nothing / is malformed. | `EbnfParseError`: `empty character class`, `reversed range` |
| Code point above U+10FFFF | `#x110000` | Not a Unicode code point. | `EbnfParseError`: `is not a Unicode code point` |
| Malformed symbol name | `Foo! ::= …` | The lexer's bareword runs to the next delimiter, so an unvalidated name would become a real rule and the typo would surface much later as an unknown reference. | `EbnfParseError`: `is not a valid symbol name` |
| ISO meta-identifier with spaces | `two words = …` | Names are one token. Falls out as a name-validation or syntax error. | `EbnfParseError` |
| The same rule defined twice | `A ::= "x"` then `A ::= "y"` | EBNF has no incremental-alternatives operator (ABNF's `=/`); the compiler would silently take one of them. | `EbnfParseError`: `rule 'A' is defined more than once` |
| Two alternatives that both match nothing | `A ::= "x"? \| "y"?` | The empty input has two derivations; no lookahead distinguishes them. | `EbnfParseError`: `rule 'A' has 2 alternatives that each match nothing` |
| Reference to an undefined rule | `A ::= B` with no `B` | Nothing to compile. | `EbnfCompileError`: `rule 'A' references unknown rule 'B'` |
| Purely left-recursive rule | `A ::= A "x"` | No seed alternative to anchor the iteration on. | `EbnfCompileError`: `rule 'A' is purely left-recursive` |

#### Not an error, but not what you may expect

- **Escape sequences in a literal.** W3C EBNF defines none, so `"\n"` is
  the two characters `\` and `n`, and `"\"` is a one-character literal.
  Write control characters as `#xA` or `[#xA]`.
- **A `]` inside a character class.** Classes are claimed whole by a
  single-line matcher, so the first `]` ends the class. Write a literal
  `]` as the string `"]"`.
- **A hyphen inside a name.** `foo-bar` is one symbol; the text matcher
  consumes the hyphen as part of the name. Only a hyphen that *starts* a
  token is the subtraction operator, so `A - B` and `A -B` are rejected
  while `A-B` is a reference to a rule called `A-B`.
- **Nested `(* … *)` comments.** The first `*)` ends the comment.
- **Whitespace.** The engine's lexer skips whitespace between tokens, so
  a char-level rule such as `Name ::= [a-z]+` will join two
  space-separated words. Spec grammars written for scannerless parsers
  usually assume the opposite. Use `TX` for whole words.
- **Not every production becomes a tree node.** The shared compiler
  folds a rule whose body is a single token segment into its caller, and
  a left-recursive rule is rewritten before emission. The AST reflects
  the compiled grammar, not a one-to-one image of the source.

### Bounded lookahead

The tabnas engine is deterministic: it dispatches on bounded,
grammar-declared lookahead plus a mark/rewind probe that the shared
compiler synthesises for one specific shape — an optional prefix
followed by a distinguishing token. It does not backtrack in general.

A grammar that hides its decision behind an unbounded prefix therefore
**compiles** and then fails on the inputs that need the extra lookahead:

```
S ::= L "x" | L "y"
L ::= "a"+
```

`a x`, `a y` and `a a x` parse; `a a y` does not. The front-end does not
try to detect this statically, and that is deliberate: the boundary
depends on the shape *and* on how deeply the input nests, so any check
sharp enough to catch this grammar also rejects `Expr ::= Term "+" Expr
| Term`, which works. The one ambiguity that *can* be ruled out soundly
— two alternatives that both match the empty string — is refused, and
the rest is left to the compiler's own named errors.

Left-factor by hand:

```
S ::= L ( "x" | "y" )
L ::= "a"+
```

Both shapes are pinned by tests, so this section fails the build rather
than aging quietly if the compiler's reach changes.

## The meta-grammar

The EBNF notation itself is read by a tabnas grammar (`ebnfRules`) over
this token vocabulary:

| Token | Matches | Matcher |
|---|---|---|
| `#DEF` | `::=` | eager match token |
| `#DEFE` | `=` | eager match token |
| `#ALT` `#LP` `#RP` `#STAR` `#PLUS` `#QM` | `\|` `(` `)` `*` `+` `?` | fixed |
| `#CA` `#SC` | `,` `;` | fixed |
| `#OS` `#CS` `#OB` `#CB` | `[` `]` `{` `}` | fixed — only ever reached on input that is about to be refused |
| `#CC` | a complete `[…]` class | eager match token |
| `#HX` | `#xNN` | eager match token |
| `#SUB` | `-` at a token start | eager match token |
| `#CM` | `/* … */`, `(* … *)` | comment matcher / eager match token |
| `#TX` `#ST` `#ZZ` | bareword, quoted string, end-of-source | engine defaults |

Every match token is *eager* — it fires wherever its pattern matches,
rather than only where the current rule's token column already expects
it. That is safe because each pattern starts with a character that has
exactly one meaning in EBNF: `[` only opens a class, `#` only opens a
code point, `::=`/`=` only define a production, and a `-` inside a name
is consumed by the text matcher as part of that name and never reaches a
position of its own.
