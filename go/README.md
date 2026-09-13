# ebnf (Go)

EBNF grammar compiler for the [tabnas](https://github.com/tabnas/parser)
parser: takes EBNF source in the **W3C dialect** and emits a tabnas
`GrammarSpec`.

**This is a best-effort front-end.** It implements one dialect properly
and refuses the rest by name rather than compiling it to something
plausible. The itemised list is in
[doc/reference.md](doc/reference.md#what-this-dialect-accepts); read it
before writing a grammar.

## Install

```bash
go get github.com/tabnas/ebnf/go@latest
```

```go
import (
    ebnf "github.com/tabnas/ebnf/go"
    tabnas "github.com/tabnas/parser/go"
)
```

The engine and the shared compiler, `github.com/tabnas/bnf/go`, are
module dependencies, so one `go get` is enough.

## One example

```go
j := tabnas.Make()
_, err := ebnf.Install(j, `
  Expr   ::= Term ( ( "+" | "-" ) Term )*
  Term   ::= Factor ( ( "*" | "/" ) Factor )*
  Factor ::= NR | "(" Expr ")"
`, &ebnf.ConvertOptions{Start: "Expr"})

out, err := j.Parse("1 + 2 * 3")
// rule "Expr", src "1+2*3"
```

Build the instance once and reuse it: compiling the grammar is the
expensive part, and three productions become 51 rules.
`ebnf.ToSpec(src, opts)` builds the spec without installing it.

## What this package is

A front-end, and only that. It reads one notation into the grammar IR
that [`tabnas/bnf`](https://github.com/tabnas/bnf) defines, and hands
that IR to the shared compiler:

```
EBNF text ──ParseEbnf──▶ bnf.Grammar ──bnf.EmitGrammarSpec──▶ GrammarSpec
```

So a defect in how `A*` is spelled belongs here, and a defect in how a
star compiles belongs there. The IR types this package exports
(`EbnfGrammar`, `EbnfProduction`, `EbnfSequence`, `EbnfElement`,
`ConvertOptions`) are aliases for the compiler's, not copies.

## Documentation

Documentation follows the [Diátaxis](https://diataxis.fr) framework:

- [Tutorial](doc/tutorial.md). From install to a working parser, step by
  step.
- [How-to guide](doc/guide.md). Short recipes for individual tasks.
- [Reference](doc/reference.md). The public API, and what the dialect
  accepts and refuses.
- [Concepts](doc/concepts.md). Why a best-effort front-end, where the
  IR boundary falls, and how the Go port differs from TypeScript.

For the canonical TypeScript implementation, see
[`../ts/README.md`](../ts/README.md).

## License

Copyright (c) 2025 Richard Rodger and other contributors, MIT License.
