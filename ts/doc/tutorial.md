# Tutorial

Build a working parser from an EBNF grammar, one construct at a time.
By the end you will have compiled a small expression language and used
it to parse real input.

Prerequisites: Node 24+, and

```bash
npm install @tabnas/parser @tabnas/bnf @tabnas/ebnf
```

Everything here uses the **W3C dialect** of EBNF — `::=` for
definitions, postfix `?` `*` `+` for repetition, `[…]` for character
classes. If you have written ISO/IEC 14977 before, read
[reference.md](reference.md#what-is-and-is-not-supported) first: the
brackets mean something different here.

## 1. A grammar with one rule

A grammar is a list of productions. Install one on an engine with
`tn.ebnf(...)`, and the engine can parse that language.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`Greet ::= "hi" | "hello"`)

tn.parse('hi').rule // => 'Greet'
tn.parse('hello').src // => 'hello'
```

`|` separates alternatives. Quoted strings are terminals, matched
exactly — W3C EBNF literals are case-**sensitive**, so `"hi"` does not
match `HI`.

Each rule that matches contributes one node to the parse tree, with
three fields: `rule` (the production name), `src` (the text it matched)
and `kids` (child nodes).

## 2. Sequences and references

Put elements side by side to match them in order. Write a bare symbol
name to reference another production.

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
out.kids[0].src // => 'world'
```

`[a-z]` is a **character class**: it matches exactly one character from
the set. `+` after it means "one or more". `Name` is a sub-rule, so it
appears as a child node; `"hello"` is a terminal, so it does not.

## 3. Repetition and optionality

The three postfix operators are `?` (zero or one), `*` (zero or more)
and `+` (one or more). Parentheses group a sub-expression so an operator
applies to all of it.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`List ::= "[" ( Item ( "," Item )* )? "]"
Item ::= NR`)

tn.parse('[]').src // => '[]'
tn.parse('[1,2,3]').src // => '[1,2,3]'
```

That is the standard "comma-separated, possibly empty" shape: an
optional group containing one item followed by zero or more
`, item` pairs.

`NR` is one of the engine's **built-in lexer tokens** (`TX` bareword,
`NR` number, `ST` quoted string, `VL` `true`/`false`/`null`). They are
not part of W3C EBNF; they come from the shared compiler, and they are
what makes a token-level grammar practical on this engine.

## 4. A whole small language

Precedence in EBNF is expressed by layering rules: an expression is a
sum of terms, a term is a product of factors, a factor is a number or a
parenthesised expression.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  Expr   ::= Term ( ( "+" | "-" ) Term )*
  Term   ::= Factor ( ( "*" | "/" ) Factor )*
  Factor ::= NR | "(" Expr ")"
`)

tn.parse('1 + 2 * 3').src // => '1+2*3'
tn.parse('(1+2)*3').src // => '(1+2)*3'
tn.parse('1 + 2 - 3 * 4 / 5').rule // => 'Expr'
```

`src` has no spaces in it because the lexer skips whitespace between
tokens: `src` is the concatenation of what matched, not a slice of the
input.

## 5. When a grammar is refused

The front-end refuses constructs it cannot compile, by name, at compile
time — not at parse time, and not silently.

```js
const { ebnfConvert, EbnfParseError } = require('@tabnas/ebnf')

let name = null
try { ebnfConvert('A ::= B - C') } catch (e) { name = e.name }
name // => 'EbnfParseError'
```

`A - B` is ISO/IEC 14977's exception operator (and W3C's too). The
grammar IR has no difference operator, so rather than compiling
something almost-right, the converter stops and says so. The same is
true of `? … ?` special sequences and `{ A }` bracket repetition —
see [reference.md](reference.md#what-is-and-is-not-supported) for the
complete list.

## 6. Left recursion

The natural way to write "an expression is an expression plus a term" is
left-recursive. Write it that way; the shared compiler rewrites it.

```js
const { Tabnas } = require('@tabnas/parser')
const { ebnf } = require('@tabnas/ebnf')

const tn = new Tabnas({ plugins: [ebnf] })
tn.ebnf(`
  Expr ::= Expr "+" Term | Term
  Term ::= NR
`)

tn.parse('1+2+3').kids.map((k) => k.rule) // => ['Term', 'Term']
```

The rewrite is `Expr ::= Term ( "+" Term )*`, so the tree comes out flat
rather than left-nested, and the leading `1` folds into `Expr` itself
instead of surfacing as its own `Term`. That is a property of the
rewrite, not a bug — see [the README's left-recursion
section](../../README.md#left-recursion).

## Where next

- [guide.md](guide.md) — recipes: character classes, translating a spec
  grammar, left-factoring, reading errors.
- [reference.md](reference.md) — the API and the exact dialect.
- [concepts.md](concepts.md) — why the dialect is W3C, and what
  "deterministic with bounded lookahead" costs you.
