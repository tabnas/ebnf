# How-to guide (Go)

Recipes, one task at a time. For a guided introduction see the
[tutorial](tutorial.md); for the API and the dialect see the
[reference](reference.md); for the reasoning see
[concepts](concepts.md).

```go
import (
    ebnf "github.com/tabnas/ebnf/go"
    tabnas "github.com/tabnas/parser/go"
)
```

## Compile and install in one call

```go
j := tabnas.Make()
spec, err := ebnf.Install(j, src, &ebnf.ConvertOptions{Start: "Expr"})
if err != nil {
    return err
}
out, err := j.Parse(input)
```

`opts` may be nil, in which case the compiler picks the start rule
itself.

## Build the spec without installing it

```go
spec, err := ebnf.ToSpec(src, &ebnf.ConvertOptions{Start: "Expr"})
```

`Ebnf` is the same function under the name the rest of the fleet uses
for a bare conversion. Install the result yourself with `j.Grammar(spec)`
when you are ready.

## Reuse the instance, not the compile

Compiling is the expensive part: three productions become 51 rules, and
that work happens once per install. Parsing is cheap. So build the
instance once, keep it, and call `Parse` as often as you like.

Use a fresh instance per grammar. Installing applies lexer settings as
well as rules, and those are instance-wide, so a second grammar on the
same instance inherits the first one's lexing. The call does not fail;
the parse it produces is what changes.

## Get the IR instead of a spec

`ParseEbnf` stops after reading the notation, and hands back the shared
compiler's grammar IR:

```go
grammar, err := ebnf.ParseEbnf(src)
for _, p := range grammar.Productions {
    p.Name // "Expr", "Term", "Factor"
    p.Alts // []bnf.Sequence
}
```

This is the boundary between this package and the compiler. Use it to
test that your notation reads the way you meant, without involving
anything downstream of the IR.

The types are aliases rather than copies: `ebnf.EbnfGrammar` is
`bnf.Grammar`, and the same for `EbnfProduction`, `EbnfSequence` and
`EbnfElement`. So a helper written against one signature works with the
other.

## Write left recursion and let it be rewritten

Left recursion compiles. The shared compiler rewrites it:

```
E ::= E "+" T | T
```

becomes the equivalent of `E ::= T ( "+" T )*`. You do not have to do
that by hand, and the parse tree is the same either way.

What does not compile is a rule with no seed:

```go
_, err := ebnf.ToSpec(`A ::= A "x"`, nil)
// ebnf: rule 'A' is purely left-recursive (no seed alternative);
// cannot eliminate
```

`EliminateLeftRecursion` runs that pass alone on an IR, which is what a
test pinning the rewrite wants:

```go
rewritten := ebnf.EliminateLeftRecursion(grammar)
```

## Tell the two kinds of error apart

```go
var pe *ebnf.ParseError
var ce *ebnf.CompileError

switch {
case errors.As(err, &pe):
    // the text is not this dialect: a syntax error, or a construct
    // this front-end refuses. pe.Line and pe.Column locate it.
case errors.As(err, &ce):
    // the text parsed, and the grammar it describes cannot be built:
    // an unknown reference, a purely left-recursive rule, an
    // ambiguous FIRST set.
}
```

Both wrap their cause, so `errors.Unwrap` reaches the original.

Every message is prefixed `ebnf:`, including the ones the shared
compiler raised. Their text is kept word for word and only the package
prefix is restamped, so a user who wrote EBNF never reads the name of a
package they did not import.

## Use the builtin lexer tokens

`TX`, `NR`, `ST` and `VL` are not rules and do not need defining. They
name the engine's own lexer tokens for bare text, numbers, strings and
keyword values:

```
Factor ::= NR | "(" Expr ")"
```

They are an extension from the shared compiler rather than part of W3C
EBNF, which is worth knowing if you are porting a grammar out.

## Keep a grammar in a golden test

The spec is data. Serialise it with the shared compiler's helpers:

```go
import bnf "github.com/tabnas/bnf/go"

text := bnf.SpecToJSON(spec, 2)
```

`bnf.SpecToJSONErr` is the same with the failure surfaced, which is the
one to use in a test that should not pass on an empty string.

## Check what a refusal actually says

The refusals are by name, and the message says what to write instead:

```go
_, err := ebnf.ToSpec(`A ::= B - C`, nil)
// ebnf: subtraction ('-') is not supported. The grammar IR has no
// difference operator, so 'A - B' cannot be compiled. Where both sides
// are single characters, write the difference as a negated character
// class instead — '[^abc]' rather than 'Char - [abc]' at line 1,
// column 9
```

The full list, with what to write in place of each, is in the
[reference](reference.md#what-this-dialect-accepts).

The TypeScript recipes for the same tasks are in
[`../../ts/doc/guide.md`](../../ts/doc/guide.md).
