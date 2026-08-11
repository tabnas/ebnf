# Concepts

Why `@tabnas/ebnf` is shaped the way it is. For the API see
[reference.md](reference.md); for recipes see [guide.md](guide.md).

## "EBNF" is a family, not a language

Extended BNF is a category of notation. The three best-known members
disagree on nearly every surface detail:

| | ISO/IEC 14977 | W3C (XML §6) | Wirth |
|---|---|---|---|
| Definition | `a = b ;` | `a ::= b` | `a = b .` |
| Optional | `[ b ]` | `b?` | `[ b ]` |
| Repetition | `{ b }` | `b*` | `{ b }` |
| Grouping | `( b )` | `( b )` | `( b )` |
| Character set | *no notation* | `[a-z]`, `#xNN` | *no notation* |
| Comment | `(* … *)` | `/* … */` | *none* |
| Exception | `a - b` | `a - b` | *none* |

They also disagree on things that are not surface detail. ISO
meta-identifiers may contain spaces; W3C symbols may not. ISO has
special sequences (`? … ?`) whose meaning is explicitly left to the
user; W3C has none.

A package advertising "EBNF support" without saying which one is
promising something it cannot deliver, because two of those columns
cannot be read at once. `[a-z]` is either three characters or an
optional three-element sequence; `{ x }` is either a repetition or a
syntax error. No amount of cleverness resolves that — only a decision
does.

## Why W3C is the primary dialect

Three reasons, in order of weight.

**There is a corpus.** XML 1.0, XML Namespaces, XPath 1.0 through 3.1,
XQuery, XML Schema, XLink, JSONPath (RFC 9535 uses ABNF, but its
predecessors and many implementations use this notation) and a long tail
of related specifications publish load-bearing grammars in exactly this
form. ISO 14977 is widely cited and, in the wild, almost never used
verbatim — most "ISO EBNF" grammars are a per-tool dialect that borrowed
its punctuation.

**Every operator has a destination.** The grammar IR that
[`@tabnas/bnf`](https://github.com/tabnas/bnf) compiles has `opt`,
`star`, `plus`, `group`, `term`, `ref`, `token` and `regex` elements.
W3C's `?`, `*`, `+`, `( … )`, `"…"`, bare symbols and `[…]`/`#xNN` map
onto those one for one. ISO's `{ … }` and `[ … ]` map just as well —
but its exception operator does not map at all, and picking ISO would
mean advertising a dialect whose most distinctive operator is missing.

**Character classes are load-bearing.** A grammar for a text format
needs to say "any character except `<` and `&`". W3C has notation for
that; ISO 14977 does not, and grammars written in it enumerate the
alternatives or fall back to prose. Since the tabnas lexer matches
character classes with a regex matcher rather than a rule per character,
`[…]` is the construct that makes real grammars compile to something
efficient.

The ISO spellings that *are* accepted — `=`, `,`, `;`, `(* … *)` — were
chosen by a single test: could a reader of a W3C grammar mistake them
for anything else? None of them can. `=` never appears in W3C EBNF, nor
does `,` or `;`, and `(*` cannot begin a legal W3C group (a group must
open with an atom, and `*` is postfix). Accepting them costs nothing and
lets a grammar written in the hybrid style most tools actually emit go
through unchanged.

## Best-effort means "refuse loudly"

The interesting decision in a best-effort front-end is not what to
support; it is what to do with the rest. There are three options:

1. Accept it and compile something approximate.
2. Accept it and ignore it.
3. Refuse it by name.

The first two produce a parser that runs and is wrong — the worst
possible outcome, because the failure surfaces as a mysterious
mis-parse a long way from the grammar. This package takes the third
option everywhere. `A - B` does not become `A`; `? … ?` does not become
a comment; `{ A }` does not become `A*`. Each raises an
`EbnfParseError` naming the construct and, where a token is involved,
the line and column.

That is also why the front-end validates symbol names. The engine's
bareword token runs to the next delimiter, so without validation
`Foo! ::= "x"` would define a rule genuinely called `Foo!`, and the typo
would resurface much later as "references unknown rule `Foo`".

## Deterministic, with bounded lookahead

The tabnas engine is a push-down parser that dispatches on token
lookahead declared by the grammar. The shared compiler computes
multi-token prefixes for each alternative, and synthesises a
mark/rewind *probe* for one specific shape: an optional prefix followed
by a distinguishing token. It does not backtrack in general, and it does
not explore alternatives in parallel.

That buys predictable, linear-ish parsing and error messages that point
at a token rather than at the deepest failed branch of a search. What it
costs is grammars whose decision point is unboundedly far from the
information that decides it:

```
S ::= L "x" | L "y"
L ::= "a"+
```

`S` must commit to an alternative before running `L`, but nothing
distinguishes them until after an arbitrarily long run of `a`. The
classical remedy is left-factoring — parse the shared prefix once and
decide after it:

```
S ::= L ( "x" | "y" )
```

The shared compiler now applies that transformation itself: consecutive
alternatives sharing a leading prefix the dispatcher cannot see past
are factored into a common prefix and a transparent decision helper, so
both spellings above parse `a a y`. Alternatives a short bounded prefix
already separates are left alone — the multi-token dispatch handles
them, and their per-alternative identity (collision marks) survives.

What remains genuinely out of reach is a decision that bounded
lookahead cannot make even after factoring — alternatives that differ
only in ways no finite token prefix reveals. The honest version of the
limitation is that, not "the engine is LL(1)" (it is not — see
`Expr ::= Term "+" Expr | Term`, which works via probe dispatch) and
not a static rejection rule. This package deliberately does **not** try
to detect the class statically: the boundary depends on both the
grammar shape and the input depth. Instead it refuses the one ambiguity
it can rule out soundly — two alternatives that both match the empty
string, which makes the *grammar* ambiguous rather than merely hard —
and leaves the rest to be visible in the tests that pin it.

## Front-end, not compiler

`@tabnas/ebnf` parses one notation into the IR and stops:

```
EBNF text ──parseEbnf──▶ Grammar ──bnf.emitGrammarSpec──▶ GrammarSpec
```

Everything hard about the second arrow — desugaring repetition into
helper rules, eliminating left recursion, tail-repeat rewriting, probe
dispatch, literal lifting, token allocation, first-set analysis, chain
emission — lives in `@tabnas/bnf` and is shared with
[`@tabnas/abnf`](https://github.com/tabnas/abnf) and
[`@tabnas/gbnf`](https://github.com/tabnas/gbnf).

That split is why this package is small, and why its diagnostics come in
two flavours. An `EbnfParseError` is about *your source text* and knows
where in it the problem is. An `EbnfCompileError` is about *your
grammar* and names a rule, because by then the text is gone.

It is also why the AST is not a mirror of the productions you wrote. The
compiler folds a rule whose body is a single token segment into its
caller, rewrites a left-recursive rule into an iterative one, and routes
multi-reference alternatives through synthetic `$stepN` continuation
rules. The tree reflects the compiled grammar. Where you need a node,
give the rule something to be — a second element keeps it a rule rather
than a token.

## Case sensitivity is a dialect fact

W3C EBNF literals match exactly; RFC 5234 (ABNF) literals are
case-insensitive unless prefixed `%s`. The IR carries a `caseSensitive`
flag precisely so the two front-ends can state their own intent and
share one emitter: this package sets it on every literal, and the
emitter lowers a case-sensitive literal to a plain fixed token while a
case-insensitive one becomes a case-folding regex matcher.

A consequence worth knowing: an EBNF grammar's literals show up in
`spec.options.fixed.token`, where an ABNF grammar's show up in
`spec.options.match.token`. If you are reading a compiled spec to see
what the lexer will do, that is where to look.
