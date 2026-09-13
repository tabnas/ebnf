# Concepts (Go)

Why this front-end is the size it is, where its boundary falls, and what
the Go port does differently. This is background reading. For steps see
the [tutorial](tutorial.md) and the [how-to guide](guide.md); for the
API see the [reference](reference.md).

## One dialect, done properly

"EBNF" names a family rather than a language. W3C EBNF spells repetition
postfix and uses `[ … ]` for a character class; ISO/IEC 14977 spells
repetition `{ A }` and uses `[ A ]` for an option. The two disagree
about what the same characters mean.

A front-end can respond to that in one of two ways. It can accept both
and guess, or it can implement one and say so. This one implements the
W3C dialect and refuses the ISO constructs by name.

The refusals are the design, not a gap. `{ A }` compiled as repetition
would be right, and `[ A ]` compiled as an option would be wrong in the
same grammar, because `[ … ]` is already a character class here.
Supporting one bracket of an ISO pair while the other keeps its W3C
meaning produces grammars that are ISO everywhere except where they
silently are not, and the failure shows up as a parse that accepts the
wrong input rather than as an error.

So the error messages name the construct and say what to write instead.
A refusal that a reader can act on is worth more than an approximation
they have to discover.

## Where the boundary falls

This package parses one notation into the grammar IR that
[`tabnas/bnf`](https://github.com/tabnas/bnf) defines, and stops:

```
EBNF text ──ParseEbnf──▶ bnf.Grammar ──bnf.EmitGrammarSpec──▶ GrammarSpec
```

Everything after the IR is shared with the other BNF-family front-ends:
desugaring, left-recursion elimination, dispatch analysis, token
allocation. Three notations differ in how they spell repetition and
grouping, and agree on what those mean.

That fixes where a change belongs. How `A*` is spelled is this
package's; how a star compiles is the compiler's. `ParseEbnf` exists to
make the boundary testable from the outside: it returns the IR without
compiling it, so a test can assert that the notation read the way its
author meant with nothing downstream involved.

The IR types are exported as aliases rather than copies for the same
reason. `EbnfGrammar` **is** `bnf.Grammar`. A copy would be a second
definition to keep in step, and there is nothing here that a front-end
needs to add to it.

## Why there are two error types

A grammar can fail in two places, and they send a reader to different
work.

`ParseError` means the text is not this dialect. Something is
misspelled, or it is ISO, or it is a construct with no meaning here. The
position is in the error, and the fix is in the source text.

`CompileError` means the text is fine EBNF and the grammar it describes
cannot be built: a reference to a rule nobody defined, a rule that is
purely left-recursive, two alternatives that both match nothing. The fix
is in the grammar's design, not its spelling.

The shared compiler raises the second kind, and this package keeps its
message word for word, restamping only the package prefix. Somebody who
wrote EBNF has not imported `bnf` and should not have to learn that it
exists to read an error about their own grammar.

## What "best effort" commits to

It is a scope statement, not a disclaimer. The dialect this package
implements is implemented properly: left recursion is rewritten rather
than rejected, both comment syntaxes are read, character classes cover
ranges, enumerations, negation and code points, and the three postfix
repetitions stack.

What "best effort" rules out is the rest of the family. There is no
subtraction, because the IR has no difference operator and one cannot be
faked over the element kinds. There are no special sequences, because
ISO leaves their content undefined, so there is nothing to compile.

The line is drawn at what can be compiled correctly rather than at what
can be parsed.

## Differences from the TypeScript version

The Go port follows TypeScript, which defines the language. There are no
shared fixtures pinning the two together: this repository has no
`test/spec` directory, and what pins behaviour is each runtime's own
suite, asserting that every documented refusal is refused and that the
message names the construct. AGENTS.md records that a message assertion
is a weaker contract than a shared error-code row.

What follows is the API shape, which differs because Go does.

- **Errors, not throws.** Every entry point returns an `error`.
  TypeScript throws `EbnfParseError` and `EbnfCompileError`; the Go
  types are `*ParseError` and `*CompileError`, matched with
  `errors.As`, and both implement `Unwrap`.
- **No plugin object.** TypeScript installs through a plugin
  (`new Tabnas({ plugins: [ebnf] })`, then `tn.ebnf(src)`). Go has
  `Install(j, src, opts)`, which is the same two steps as one call.
- **Two names for the conversion.** `Ebnf` is the fleet's name for a
  front-end's bare conversion entry point, and `ToSpec` is the name the
  TypeScript package uses. Keeping both means a reader coming from
  either side finds the one they expect.
- **Aliases, not re-exports.** TypeScript re-exports the compiler's
  types; Go aliases them, which is the closest equivalent and keeps
  assignability in both directions.

The canonical implementation is in
[`../../ts/README.md`](../../ts/README.md), and its own concepts page is
[`../../ts/doc/concepts.md`](../../ts/doc/concepts.md).
