# Tutorial: your first EBNF grammar (Go)

This walks you from nothing to a working parser built from EBNF text.
Follow it in order; each step builds on the last. When you finish you
will have installed a grammar on an engine, parsed with it, read the
tree it produced, built a spec without installing it, seen a construct
this front-end refuses, and read both kinds of error.

For recipes covering one task at a time, see the
[how-to guide](guide.md). For the API and the dialect, see the
[reference](reference.md). For why the dialect is the size it is, see
[concepts](concepts.md).

## 1. Install

```bash
go get github.com/tabnas/ebnf/go@latest
```

```go
import (
    ebnf "github.com/tabnas/ebnf/go"
    tabnas "github.com/tabnas/parser/go"
)
```

The engine and the shared compiler come with it, so nothing else is
needed.

## 2. Install a grammar on an engine

`Install` reads EBNF source and puts the result on an instance:

```go
j := tabnas.Make()
spec, err := ebnf.Install(j, `
  Expr   ::= Term ( ( "+" | "-" ) Term )*
  Term   ::= Factor ( ( "*" | "/" ) Factor )*
  Factor ::= NR | "(" Expr ")"
`, &ebnf.ConvertOptions{Start: "Expr"})
```

`Start` names the production the engine begins at. `spec` is the
compiled grammar, which you can keep for inspection; installing has
already used it.

Three productions here. The spec holds 51 rules, because the compiler
desugars every repetition and grouping into helper productions of its
own.

## 3. Parse something

```go
out, err := j.Parse("1 + 2 * 3")
```

`out` is a `map[string]any` with `rule`, `src` and `kids`:

```go
m := out.(map[string]any)
m["rule"] // "Expr"
m["src"]  // "1+2*3"
```

The spaces are gone from `src` because the engine's lexer skips them
between tokens. Nothing was lost: the structure is in `kids`, and the
precedence you wrote into the grammar is what shaped it. `1 + 2 * 3`
gives an `Expr` whose single child is a `Term` covering `2*3`, so the
multiplication bound tighter, as the two rules say it should.

## 4. Read the notation you just used

Four constructs carried that grammar:

| Wrote | Means |
|---|---|
| `::=` | defines a production. `=` with a trailing `;` also works. |
| `A \| B` | alternation |
| `( … )` | grouping |
| `A*` | zero or more, postfix |

`NR` is not a rule. It is one of four builtin lexer tokens the shared
compiler provides, alongside `TX`, `ST` and `VL`, so `Factor ::= NR`
matches a number without your having to spell one out.

The rest of the dialect is in the
[reference](reference.md#what-this-dialect-accepts).

## 5. Build a spec without installing it

When you want the grammar as data, for a golden test or to install
later, use `ToSpec`:

```go
spec, err := ebnf.ToSpec(src, &ebnf.ConvertOptions{Start: "Expr"})
```

`Ebnf` is the same function under the name the rest of this fleet uses.
Both return the same spec that `Install` would have used.

## 6. Meet a refusal

This dialect is one dialect. ISO/IEC 14977 constructs that mean
something else here are refused by name rather than compiled into an
approximation:

```go
_, err := ebnf.ToSpec(`A ::= { B }`, nil)
// ebnf: ISO 14977 bracket repetition ('{') is not supported; this
// dialect spells repetition postfix — 'A*' rather than '{ A }' at
// line 1, column 7
```

That is a `*ebnf.ParseError`, and it carries the position:

```go
var pe *ebnf.ParseError
if errors.As(err, &pe) {
    pe.Line   // 1
    pe.Column // 7
}
```

## 7. Meet the other kind of error

A grammar can be perfectly good EBNF and still fail to compile. That is
a different type:

```go
_, err := ebnf.ToSpec(`A ::= B`, nil)
// ebnf: rule 'A' references unknown rule 'B'

var ce *ebnf.CompileError
errors.As(err, &ce) // true
```

The split is worth knowing because it tells you where to look.
`ParseError` means the text is not this dialect. `CompileError` means
the text parsed and the grammar it describes cannot be built.

Note the prefix on both. The shared compiler raised the second one, and
its message is kept word for word with only the package name restamped,
so somebody who wrote EBNF never sees the name of a package they did not
import.

## Where to go next

- [How-to guide](guide.md). Recipes: reusing an instance, the IR,
  left recursion, golden tests, reading errors.
- [Reference](reference.md). The four functions, the options, the error
  types, and the full table of what the dialect accepts and refuses.
- [Concepts](concepts.md). Why the dialect is deliberately small, and
  what the IR boundary buys.
- The [README](../README.md) and the root
  [README](../../README.md).
