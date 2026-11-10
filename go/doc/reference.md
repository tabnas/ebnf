# Reference (Go)

The complete public surface of the Go `ebnf` module, and what the
dialect accepts and refuses. For a guided introduction see the
[tutorial](tutorial.md); for task recipes see the
[how-to guide](guide.md); for the reasoning see
[concepts](concepts.md).

## Module

```bash
go get github.com/tabnas/ebnf/go@latest
```

```go
import (
    ebnf "github.com/tabnas/ebnf/go"
    tabnas "github.com/tabnas/parser/go"
)
```

| | |
|---|---|
| Module | `github.com/tabnas/ebnf/go` |
| Package | `ebnf` |
| Engine | `github.com/tabnas/parser/go` (imported as `tabnas`) |
| Compiler | `github.com/tabnas/bnf/go`, which defines the IR and compiles it |
| Dialect | W3C EBNF, best effort |

## Public API

Four functions, one pass, one constant, and the type aliases. Three more serve a host that translates: the tree's reader, its parser, and the translation parts.

### `func Install(j *tabnas.Tabnas, src string, opts *ConvertOptions) (*tabnas.GrammarSpec, error)`

Converts EBNF source and installs it on an instance. Returns the spec it
installed, so you can keep it for inspection.

```go
j := tabnas.Make()
spec, err := ebnf.Install(j, src, &ebnf.ConvertOptions{Start: "Expr"})
```

Use a fresh instance per grammar. Installing applies lexer settings as
well as rules, and those are instance-wide.

### `func ToSpec(src string, opts *ConvertOptions) (*tabnas.GrammarSpec, error)`
### `func Ebnf(src string, opts *ConvertOptions) (*tabnas.GrammarSpec, error)`

The same function twice. It parses the notation and hands the IR to the
shared compiler, returning the spec without installing it. `Ebnf` is the
name the rest of this fleet uses for a front-end's bare conversion
entry point; `ToSpec` is the name the TypeScript package uses.

### `func ParseEbnf(src string) (*bnf.Grammar, error)`

Stops at the IR. Reads the notation into the shared compiler's grammar
and returns it, compiling nothing.

### `func EliminateLeftRecursion(g *EbnfGrammar) *EbnfGrammar`

Runs the shared compiler's left-recursion pass alone over an IR, for a
test that pins the rewrite. Returns a new grammar.

### `const VERSION`

This module's version. It must equal `ts/package.json` `"version"`; a
drift test in each runtime enforces it.

### `func ParseEbnfTree(src string) (*tabnas.OrderedMap, error)`

Reads a document whole as the tree a host translates: the schema
grammar-rules, which tabnas/bnf defines and the other grammar notations
share, the rules as the text writes them in source order, each member in
the order the schema gives it. A source the meta-grammar refuses, or the
tree has no room for, fails with a `*ParseError` whose `Cause` is the
engine's error with its code: one of the tree's codes, which the
section on error codes of `../../AGENTS.md` lists, or the engine's own.

### `func Make() *tabnas.Tabnas`

A fresh parser whose value is that tree: the parser a host streams a
document through with the transducer part, whose rule passes the part
reads. The parser is a tabnas rule table, the canonical meta-grammar as
the Rust crate's grammar document, where `ParseEbnf` is a hand-written
scanner.

### `func Translate() *TranslationParts`

The translation parts the manifest names, each an `Entry` and its
`Source`, and the manifest's text:

- `Transduce`, entry `ebnf-transduce`, the transducer part.
- `Render`, entry `ebnf-render`, which writes a tree as W3C EBNF.
- `Spec`, entry `ebnf-render-spec`, which writes a compiled grammar spec
  as W3C EBNF.

Each call returns a copy of its own.

## Options

`opts` may be nil everywhere it is accepted, in which case the compiler
picks the start rule itself.

### `type ConvertOptions = bnf.ConvertOptions`

An alias, not a copy. The fields are the shared compiler's:

| Field | Default | Effect |
|---|---|---|
| `Start` | none | The production the engine begins at. |
| `Tag` | `"bnf"` | Stamped on emitted alternates. This package passes `"ebnf"` for you. |
| `Builtins` | `false` | Emit actions as `@name$` strings rather than closures, which `bnf.ToPureSpec` requires. |
| `Marks` | `false` | Record a mark on each user alternate, so semantic actions can bind to one. |
| `WordKeywords` | `false` | Append a word-boundary guard to a literal ending in a word character. |
| `Provenance` | on | Emit the map from each generated rule name back to the production it came from. |

Every field is documented in
[the compiler's reference](https://github.com/tabnas/bnf/blob/main/go/doc/reference.md#options).

## IR types

| Alias | Is |
|---|---|
| `EbnfGrammar` | `bnf.Grammar` |
| `EbnfProduction` | `bnf.Production` |
| `EbnfSequence` | `bnf.Sequence` |
| `EbnfElement` | `bnf.Element` |

Aliases rather than copies, so a helper written against either name
takes the other.

## Errors

| Type | Means | Carries |
|---|---|---|
| `*ParseError` | the source is not this dialect: a syntax error, or a refused construct | `Message`, `Line`, `Column`, `Cause` |
| `*CompileError` | the source parsed, and the grammar it describes cannot be built | `Message`, `Cause` |

Both implement `error` and `Unwrap`.

```go
var pe *ebnf.ParseError
var ce *ebnf.CompileError
errors.As(err, &pe)
errors.As(err, &ce)
```

`Line` and `Column` are 1-based and locate the offending token where one
is available.

A `CompileError` comes from the shared compiler. Its message is kept
word for word and only the package prefix is restamped, so every
diagnostic a user of this package sees begins `ebnf:`:

```
ebnf: rule 'A' references unknown rule 'B'
ebnf: rule 'A' is purely left-recursive (no seed alternative); cannot eliminate
ebnf: rule 'A' is defined more than once; EBNF has no incremental-alternatives operator
```

## What this dialect accepts

The notation is the same in every runtime, and the itemised tables live
with the canonical implementation:

- [Supported constructs](../../ts/doc/reference.md#supported), from
  `::=` and the ISO `= … ;` spelling through grouping, the three postfix
  repetitions, literals, character classes, code points, both comment
  syntaxes and left recursion.
- [Refused constructs](../../ts/doc/reference.md#not-supported), each
  with the reason and the error it raises.
- [Accepted, but not what you may expect](../../ts/doc/reference.md#not-an-error-but-not-what-you-may-expect),
  which is the section to read before deciding this front-end has a
  defect.

Three points are worth having here as well, because they are the ones
that catch a reader coming from ISO/IEC 14977.

**`{ A }` is refused.** Repetition is postfix in this dialect:

```
ebnf: ISO 14977 bracket repetition ('{') is not supported; this dialect
spells repetition postfix — 'A*' rather than '{ A }' at line 1, column 7
```

**`[ A ]` is not an option, and not refused either.** `[ … ]` is a
character class here, so `[ B ]` is the class matching a space, a `B`,
or a space. An ISO option written this way surfaces later as a stray
bracket or as a class that matches the wrong thing. Write `A?`.

**`A - B` is refused**, because the IR has no difference operator. Where
both sides are single characters, a negated class says the same thing.

**`TX`, `NR`, `ST` and `VL`** are builtin lexer tokens from the shared
compiler, not rules and not part of W3C EBNF. A grammar using them is
not portable out of this fleet.

The canonical reference is
[`../../ts/doc/reference.md`](../../ts/doc/reference.md).
