# @tabnas/ebnf

<!-- tabnas-badges -->
[![npm](https://tabnas.github.io/status/badges/ebnf-npm.svg)](https://www.npmjs.com/package/@tabnas/ebnf)
[![CI](https://github.com/tabnas/ebnf/actions/workflows/ci.yml/badge.svg)](https://github.com/tabnas/ebnf/actions/workflows/ci.yml)
[![tabnas standard](https://tabnas.github.io/status/badges/ebnf-standard.svg)](https://tabnas.github.io/status/)
<!-- /tabnas-badges -->

EBNF grammar compiler for the [tabnas](https://github.com/tabnas/parser)
parser. Takes EBNF source — the **W3C dialect**, the notation the XML,
XPath and XQuery specifications publish their grammars in — and emits a
tabnas `GrammarSpec`. Installed on an engine, the spec parses inputs in
that grammar and builds a `{rule, src, kids}` AST.

> ## This is a best-effort front-end
>
> "EBNF" is a family of notations, not a language. This package
> implements **one dialect properly** and refuses the rest by name. It
> is not an ISO/IEC 14977 implementation, it is not a universal EBNF
> reader, and it will not quietly do something plausible with syntax it
> does not understand.
>
> Concretely, and verified by the test suite rather than asserted:
>
> - **ISO 14977 exception / subtraction `A - B` is rejected.** The
>   grammar IR has no difference operator and there is no general way to
>   synthesise one.
> - **ISO 14977 special sequences `? … ?` are rejected.** Their content
>   is undefined by the standard, so there is nothing to compile.
> - **ISO 14977 bracket repetition `{ A }` and option `[ A ]` are
>   rejected.** `[ … ]` is a *character class* in the W3C dialect;
>   nothing can read `[a-z]` as both a three-character set and an
>   optional three-element sequence.
> - **Grammars that need general backtracking do not work.** The tabnas
>   engine is deterministic, with bounded grammar-declared lookahead
>   plus a mark/rewind probe for one specific optional-prefix shape. A
>   grammar that hides its decision behind an unbounded prefix compiles
>   and then fails on the inputs that need the extra lookahead — see
>   [Bounded lookahead](#bounded-lookahead) for what that looks like and
>   how to left-factor around it.
>
> The full itemised list is in
> [ts/doc/reference.md](ts/doc/reference.md#what-is-and-is-not-supported)
> and repeated below. Read it before writing a grammar, not after.

```bash
npm install @tabnas/parser @tabnas/bnf @tabnas/ebnf
```

## Which EBNF, and why

The dialect is **W3C EBNF**, as defined in
[XML 1.0 §6](https://www.w3.org/TR/xml/#sec-notation): `Symbol ::=
expression`, postfix `?` `*` `+`, `|` for alternation, `( … )` for
grouping, `"…"` / `'…'` for case-sensitive literals, `[…]` for character
classes, `#xNN` for code points, `/* … */` for comments.

Three reasons it is the primary dialect rather than ISO 14977:

1. **There is a corpus.** XML, XML Namespaces, XPath 1.0–3.1, XQuery,
   XML Schema, JSONPath and a long list of other specifications publish
   real, load-bearing grammars in exactly this notation. ISO 14977 is
   widely cited and almost never used verbatim.
2. **It maps onto the IR.** Postfix `?`/`*`/`+` are the IR's `opt`,
   `star` and `plus`; `( … )` is `group`; `[…]` and `#xNN` are `regex`
   and `term`. Every operator has a destination.
3. **The brackets settle it.** ISO writes optionality as `[ A ]` and
   repetition as `{ A }`, while W3C writes character classes as `[…]`.
   Those two readings cannot coexist, so the dialect has to be a
   documented choice. It is documented here.

A handful of ISO spellings **are** accepted, chosen because none of them
can collide with the W3C reading: `=` as a definition operator, `,` as
an explicit concatenation separator, `;` as a production terminator, and
`(* … *)` comments. Accepting those spellings does not make this an ISO
implementation — everything else about ISO 14977, including its
bracket operators, its exception operator, its special sequences and its
space-bearing meta-identifiers, is out.

## A first grammar

A grammar is a set of **productions**, `Symbol ::= definition`.
Alternatives are separated by `|`, and terminals are quoted strings.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Greet ::= "hi" | "hello"`)

tn.parse('hi') // => ({ rule: 'Greet', src: 'hi', kids: [] })
```

Every rule that matches produces one AST node with three fields:

- **`rule`** — the production's name, so you can navigate the tree by
  the names you wrote.
- **`src`** — the source text the rule matched.
- **`kids`** — child nodes, one per sub-rule the production referenced.

## Sequences, sub-rules and character classes

Write elements one after another to match them in order, reference
another production by its bare name to nest it, and use a character
class for "one character out of this set":

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  Greeting ::= "hello" Name
  Name     ::= [a-z]+
`)

const out = tn.parse('hello world')
out.kids.map((k) => k.rule) // => ['Name']
out.kids[0].src             // => 'world'
```

Note that `out.src` is `'helloworld'`, not `'hello world'`: the lexer
skips whitespace between tokens, so `src` is the concatenation of what
was matched, not a slice of the original input.

## Repetition, optionals and groups

Repetition is **postfix** — `A?`, `A*`, `A+` — and `( … )` groups a
sub-expression so an operator applies to the whole of it:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Csv ::= NR ( "," NR )*`)

tn.parse('1,2,3').src // => '1,2,3'
tn.parse('7').src     // => '7'
```

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Flag ::= "log" ( "=" TX )?`)

tn.parse('log=debug').src // => 'log=debug'
tn.parse('log').src       // => 'log'
```

## Terminals: literals, character classes, code points, tokens

**A quoted literal** is matched verbatim and **case-sensitively** — the
W3C rule, and the opposite of ABNF's. Single and double quotes are
interchangeable, so a literal containing one quote is written with the
other. There are **no escape sequences**: W3C EBNF defines none, so a
backslash is the backslash character and `"\n"` is the two characters
`\` and `n`. Write control characters as `#xA`, and a quote that the
literal cannot contain as its own `#x22` / `#x27`.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`S ::= "GET"`)

tn.parse('GET').src // => 'GET'

let rejected = false
try { tn.parse('get') } catch (e) { rejected = true }
rejected // => true
```

**A character class** matches one character: `[a-z]` a range, `[abc]` an
enumeration, `[^<&]` a negation, `[#x20-#x7E]` a range written as code
points. A class must open and close on one line and cannot contain `]`
(there are no escapes — write a literal `]` as the string `"]"`).

**A standalone `#xNN`** is a single code point, exactly as in the XML
specification's own `Char` production:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Char ::= #x9 | #xA | #xD | [#x20-#xD7FF]`)

tn.parse('a').src // => 'a'
```

**A built-in lexer token** — `TX` (bareword), `NR` (number), `ST`
(quoted string), `VL` (`true`/`false`/`null`) — matches whole tokens the
engine's lexer already produces. This is not W3C EBNF; it comes from the
shared compiler, and it is the only sane way to write a token-level
grammar for this engine. Prefer it over deriving text character by
character: because whitespace between tokens is skipped, a char-level
`[a-z]+` will happily run two space-separated words together, while `TX`
will not.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  Pair ::= "{" Key ":" Val "}"
  Key  ::= TX
  Val  ::= NR | ST
`)

tn.parse('{a:1}').kids.map((k) => k.rule) // => ['Key', 'Val']
tn.parse('{a:"x"}').kids[1].src           // => '"x"'
```

## The ISO spellings that are accepted

`=`, `,`, `;` and `(* … *)`. The operators stay W3C:

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  (* concatenation commas and terminating semicolons are optional *)
  digits = digit , digit ;
  digit  = [0-9] ;
`)

tn.parse('42').src // => '42'
```

`(* … *)` comments do not nest: the first `*)` ends the comment.

## Left recursion

Written left-recursively, an additive expression is "an expression, a
`+`, then a term". The shared compiler accepts that directly: a
left-recursion pass (Paull's algorithm) rewrites both direct (`P ::= P a
| b`) and indirect (`P ::= Q a`, `Q ::= P b`) recursion into the
iterative form a push-down engine can run without re-entering a rule at
the same source position.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  Expr ::= Expr "+" Term | Term
  Term ::= NR
`)

tn.parse('1+2+3').rule                    // => 'Expr'
tn.parse('1+2+3').kids.map((k) => k.rule) // => ['Term', 'Term']
```

It is a rewrite, not native left-recursive parsing: `Expr ::= Expr "+"
Term | Term` is compiled as `Expr ::= Term ( "+" Term )*`, so the tree
is flat rather than left-nested and the leading operand folds into
`Expr` itself. Left-associativity is something you apply in an action,
not a shape you read off the AST. A rule with no non-recursive
alternative (`Loop ::= Loop "x"`) has nothing to anchor the iteration on
and is rejected by name.

## What is and is not supported

### Supported

| Construct | Syntax | Compiles to |
|---|---|---|
| Production | `Symbol ::= expr` | a rule |
| Production (ISO spelling) | `symbol = expr ;` | a rule |
| Alternation | `A \| B` | one alt per branch |
| Concatenation | `A B` — or ISO `A , B` | one sequence |
| Grouping | `( A \| B )` | IR `group` |
| Optional | `A?` | IR `opt` |
| Zero or more | `A*` | IR `star` |
| One or more | `A+` | IR `plus` |
| Stacked postfix | `(A)*?` | nested `opt`/`star`/`plus` |
| Literal | `"abc"`, `'abc'` | IR `term`, **case-sensitive** |
| Character class | `[a-z]`, `[abc]`, `[-+]` | IR `regex` |
| Negated class | `[^<&]` | IR `regex` |
| Code point in a class | `[#x20-#x7E]`, `[#x9#xA]` | IR `regex` |
| Standalone code point | `#x41` | IR `term` |
| Rule reference | a bare `Symbol` | IR `ref` |
| Built-in lexer token | `TX`, `NR`, `ST`, `VL` | IR `token` |
| Comment (W3C) | `/* … */` | ignored |
| Comment (ISO) | `(* … *)` | ignored, does not nest |
| Left recursion | `E ::= E "+" T \| T` | rewritten iteratively |

Names are `[A-Za-z_][A-Za-z0-9_.-]*`. A hyphen inside a name is part of
the name (`foo-bar` is one symbol); a hyphen that *starts* a token is
the subtraction operator, and rejected.

### Not supported — each raises a named error

| Construct | Why | Error |
|---|---|---|
| Subtraction `A - B` | the IR has no difference operator | `EbnfParseError: … subtraction ('-') … is not supported` |
| Special sequence `? … ?` | content is undefined by ISO 14977 | `EbnfParseError: … special sequences … are not supported` |
| ISO repetition `{ A }` | `A*` is the dialect's spelling | `EbnfParseError: … ISO 14977 bracket repetition …` |
| ISO option `[ A ]` | `[ … ]` is a character class here | `EbnfParseError: … stray '[' …` |
| ABNF-style prefix repetition `*A`, `1*A` | repetition is postfix here | `EbnfParseError: … Repetition in this dialect is postfix` |
| Empty literal `""` | matches nothing; there is no epsilon terminal | `EbnfParseError: … empty string literal …` |
| Escape sequences in a literal | W3C EBNF defines none | *not an error* — `\` is a literal backslash |
| Space-bearing ISO meta-identifiers | names are one token | `EbnfParseError: … is not a valid symbol name` |
| The same rule defined twice | EBNF has no incremental-alternatives operator | `EbnfParseError: rule 'X' is defined more than once` |
| Two alternatives that both match nothing | genuinely ambiguous | `EbnfParseError: rule 'X' has 2 alternatives that each match nothing` |
| Reference to an undefined rule | nothing to compile | `EbnfCompileError: rule 'X' references unknown rule 'Y'` |
| Purely left-recursive rule | no seed to anchor the iteration | `EbnfCompileError: rule 'X' is purely left-recursive` |

### Bounded lookahead

The remaining limit is not a syntax the parser refuses; it is a class of
grammar the *engine* cannot run. The tabnas engine is deterministic. It
dispatches on bounded, grammar-declared lookahead, plus a mark/rewind
probe that the shared compiler synthesises for one specific shape — an
optional prefix followed by a distinguishing token. It does not
backtrack in general.

So a grammar that defers its decision behind an unbounded prefix
compiles without complaint and then fails on the inputs that need the
extra lookahead:

```
S ::= L "x" | L "y"
L ::= "a"+
```

`a x`, `a y` and `a a x` parse; `a a y` does not. **There is no static
check for this in the front-end, and adding one would be dishonest:**
the boundary depends on the shape *and* on how deep the input nests, so
any rule sharp enough to catch this grammar also rejects `Expr ::= Term
"+" Expr | Term`, which works. What the front-end does instead is refuse
the one case it can rule out soundly — two alternatives that both match
the empty string — and leave the rest to the compiler's own named
errors.

The fix is left-factoring, by hand, at the point the notation makes it
obvious:

```
S ::= L ( "x" | "y" )
L ::= "a"+
```

Both shapes are pinned by tests
([`ts/test/ebnf.test.js`](ts/test/ebnf.test.js), "the bounded-lookahead
limit"), so if a future compiler handles the first one, the suite goes
red and this section gets rewritten rather than quietly aging.

## How it fits together

`@tabnas/ebnf` is a **front-end**. It parses one notation into the
grammar IR defined by [`@tabnas/bnf`](https://github.com/tabnas/bnf),
and does nothing else:

```
EBNF text ──parseEbnf──▶ Grammar ──bnf.emitGrammarSpec──▶ GrammarSpec
```

Everything downstream of that IR — desugaring repetition into helper
rules, eliminating left recursion, tail-repeat rewriting, probe
dispatch, literal lifting, token allocation, first-set analysis, chain
emission — lives in `@tabnas/bnf` and is shared with
[`@tabnas/abnf`](https://github.com/tabnas/abnf) (RFC 5234) and
[`@tabnas/gbnf`](https://github.com/tabnas/gbnf) (llama.cpp GBNF). A
diagnostic that names a rule rather than a source position comes from
there.

| Path | Description |
|---|---|
| [`ts/`](ts/) | TypeScript / JavaScript (`@tabnas/ebnf`). |
| [`go/`](go/) | Go port — not yet written. |

## Documentation

Four-quadrant [Diátaxis](https://diataxis.fr) docs:

| | TypeScript |
|---|---|
| Tutorial (learn) | [ts/doc/tutorial.md](ts/doc/tutorial.md) |
| Guide (tasks) | [ts/doc/guide.md](ts/doc/guide.md) |
| Reference (API + dialect) | [ts/doc/reference.md](ts/doc/reference.md) |
| Concepts (why) | [ts/doc/concepts.md](ts/doc/concepts.md) |

See [`ts/README.md`](ts/README.md) for per-language orientation and
[`AGENTS.md`](AGENTS.md) for the internals.

## License

MIT. Copyright (c) Richard Rodger.
