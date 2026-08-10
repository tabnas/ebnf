# @tabnas/ebnf

EBNF grammar compiler for the [Tabnas](https://github.com/tabnas/parser)
parser: takes EBNF source in the **W3C dialect** and emits a tabnas
`GrammarSpec`.

**This is a best-effort front-end.** It implements one dialect properly
and refuses the rest by name — ISO/IEC 14977 subtraction (`A - B`),
special sequences (`? … ?`) and bracket repetition (`{ A }` / `[ A ]`)
all raise a named error rather than compiling to something plausible.
The itemised list is in
[doc/reference.md](doc/reference.md#what-is-and-is-not-supported); read
it before writing a grammar.

## Install

```bash
npm install @tabnas/parser @tabnas/bnf @tabnas/ebnf
```

`@tabnas/parser` (the engine) and `@tabnas/bnf` (the shared compiler)
are peer dependencies.

## One example

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
tn.parse('(1+2)*3').rule  // => 'Expr'
```

Build the instance once and reuse it — compiling the grammar is the
expensive part. `tn.ebnf.toSpec(src)` builds the spec without installing
it.

## Documentation

Four-quadrant [Diátaxis](https://diataxis.fr) docs:

| Quadrant | File |
|---|---|
| Tutorial (learn) | [doc/tutorial.md](doc/tutorial.md) |
| How-to guide (tasks) | [doc/guide.md](doc/guide.md) |
| Reference (API + dialect) | [doc/reference.md](doc/reference.md) |
| Concepts (why) | [doc/concepts.md](doc/concepts.md) |

Repo-level orientation, including the full supported/unsupported table,
is in the [top-level README](../README.md).

## License

MIT. Copyright (c) Richard Rodger.
