# Agents Guide — ebnf

## What this project is

`@tabnas/ebnf` is a **grammar front-end**: it parses EBNF text into the
notation-neutral grammar IR defined by
[`@tabnas/bnf`](https://github.com/tabnas/bnf), and does nothing else.

```
EBNF text ──parseEbnf──▶ Grammar ──bnf.emitGrammarSpec──▶ GrammarSpec
```

Everything downstream of that IR — desugaring repetition into helper
rules, left-recursion elimination, tail-repeat rewriting, probe
dispatch, literal lifting, token allocation, first-set analysis, chain
emission — lives in `@tabnas/bnf` and is shared with
[`@tabnas/abnf`](https://github.com/tabnas/abnf) (RFC 5234) and
[`@tabnas/gbnf`](https://github.com/tabnas/gbnf) (llama.cpp GBNF).
**Do not reimplement any of it here.** If a grammar compiles wrongly and
the cause is in the second arrow, the fix belongs in `bnf`.

`@tabnas/abnf`'s `ts/src/converter.ts` is the reference front-end; this
package deliberately mirrors its structure (a tabnas grammar that reads
the notation, a `parseX(src)` returning the IR, a facade that calls
`emitGrammarSpec`, a plugin export).

## The dialect decision

"EBNF" is a family of notations that disagree on brackets. This package
implements **W3C EBNF** (XML 1.0 §6) as its primary dialect, plus four
ISO/IEC 14977 spellings that cannot collide with the W3C reading (`=`,
`,`, `;`, `(* … *)`). ISO's `{ A }` repetition and `[ A ]` option are
refused by name, because `[ … ]` is a character class here.

The rationale — corpus, IR fit, and the bracket collision — is in
[`ts/doc/concepts.md`](ts/doc/concepts.md). Do not widen the accepted
syntax without updating that reasoning and the itemised table in
[`ts/doc/reference.md`](ts/doc/reference.md#what-is-and-is-not-supported).

**This is a best-effort package and the docs say so in three places**
(top-level README, `ts/README.md`, `ts/doc/reference.md`). Any change
that alters what compiles must move all three, and must move the
tests in `ts/test/ebnf.test.js` that pin each rejection. A construct
that does not work goes in the not-supported list; it does not get
quietly dropped from the tests.

## Repository map

| Path | What it is |
|---|---|
| [`ts/src/converter.ts`](ts/src/converter.ts) | The front-end. Holds `ebnfRules` (the tabnas grammar that reads EBNF source), the character-class and code-point decoders, the named rejections, and the two soundness checks (`checkDuplicates`, `checkNullableAlts`). |
| [`ts/src/ebnf.ts`](ts/src/ebnf.ts) | Facade + plugin. Exports `ebnf` (Plugin), `ebnfConvert` / `toSpec`, `parseEbnf`, `EbnfParseError`, `EbnfCompileError`, `VERSION`, and re-exports `emitGrammarSpec` / `eliminateLeftRecursion` from `bnf`. |
| [`ts/test/ebnf.test.js`](ts/test/ebnf.test.js) | The suite: IR shape, end-to-end parses, every documented rejection, the realistic fixtures, and the bounded-lookahead limit. |
| [`ts/test/grammar/`](ts/test/grammar/) | `.ebnf` fixtures — `expr.ebnf`, `json-subset.ebnf`, `name.ebnf`, and `iso-style.ebnf` (the same language as `expr.ebnf` in the accepted ISO spellings). |
| [`ts/test/doc-examples.test.js`](ts/test/doc-examples.test.js) | Runs every ```js fence carrying a `// =>` assertion in the README and `ts/doc`. Shared harness, identical across tabnas repos. |
| [`ts/test/version.test.js`](ts/test/version.test.js) | `VERSION` vs `package.json` "version". |
| [`ts/doc/`](ts/doc/) | Four-quadrant Diátaxis docs. |
| [`go/`](go/) | Go port — **not yet written**. The scaffold's zon sources were removed; `go/doc/` and the remaining `*_test.go` files are stale and will be rewritten with the port. |

## How the meta-grammar reads EBNF

The notation is parsed by a tabnas grammar (`ebnfRules`) on a bare
engine — no grammar plugin, since EBNF defines its own syntax from
scratch. Five rules: `ebnf` → `prod` → `alts` → `seq` → `elem` →
`post` / `atom`.

Two things are worth understanding before editing it.

**Every match token is eager.** ABNF's converter has to list tokens in
`s:` patterns to widen the lexer's token column, because its matchers
(`%xNN`, `%s"…"`, prose) are ambiguous with a bareword. None of EBNF's
are: `[` only opens a character class, `#` only opens a code point,
`::=`/`=` only define a production, and a `-` inside a name is consumed
by the text matcher as part of that name. So each `match.token` matcher
carries `eager$`, opting out of token-column gating, and the rule table
stays about parsing rather than about lexing. If you add a matcher whose
first character has more than one meaning, that reasoning stops holding
— either keep it non-eager and widen the columns, or do not add it.

**`(* … *)` is a match token, not a comment definition.** The fixed
matcher runs before the comment matcher, so `(` is already `#LP` by the
time a `comment.def` entry would be offered the position. The ISO
comment is therefore claimed by an eager matcher function that emits an
ordinary `#CM` (which the parser skips like any other comment) and does
its own row/column bookkeeping — without that, a multi-line comment
would shift every subsequent error's reported line. There is a test for
exactly that.

**`[` `]` `{` `}` stay declared as fixed tokens** even though a
well-formed character class is claimed whole by `#CC` before the fixed
matcher is reached. They are declared so the text matcher treats them as
delimiters (otherwise `A[a-z]` lexes as one long bareword), and the tins
therefore only surface on input that is about to be refused — which is
where the named errors hang.

## What this package checks, and what it does not

Two checks live here because they are sound and cheap:

- `checkDuplicates` — EBNF has no incremental-alternatives operator, so
  a repeated symbol is a mistake the compiler would silently resolve.
- `checkNullableAlts` — two alternatives of one production that both
  derive ε make the *grammar* ambiguous. The compiler emits a dispatch
  for it anyway and the result mis-parses.

**There is deliberately no general ambiguity or backtracking check.**
The engine is deterministic with bounded lookahead plus a probe for one
optional-prefix shape, and the shared compiler left-factors a shared
prefix the dispatcher cannot see past. What remains depends on the
grammar shape *and* the input depth: `S ::= A "x" | B "y"` with
`A ::= "a" A | "a"` and `B` likewise fails `a a x` (factoring is
structural, so two distinct rules spelling the same unbounded prefix
cannot merge), while both `S ::= L "x" | L "y"` with `L ::= "a"+` and
`Expr ::= Term "+" Expr | Term` work at any depth — the first by
factoring, the second by probe dispatch. Any static rule sharp enough to
reject the first also rejects the others. The limit is documented in
README.md, `ts/doc/concepts.md`, `ts/doc/guide.md` and
`ts/doc/reference.md`, and pinned by the "bounded-lookahead limit"
tests instead, so a change in the compiler's reach turns the suite red
rather than aging the prose.

When that happens — as it did when left factoring landed — update the
tests AND all four documents in the same change. The suite pins
behaviour, not prose: a bare ``` fence carries no assertion, so
`doc-examples.test.js` cannot catch a stale claim for you.

If you are tempted to add a heuristic here: run it against
`ts/test/grammar/expr.ebnf` and `json-subset.ebnf` first.

## Authority and alignment rules

1. **`ts/` is canonical.** `go/` will track it. Neither exists yet in
   Go; do not add a Go port piecemeal.
2. **Nothing notation-neutral belongs here.** If a change would help
   ABNF or GBNF too, it belongs in `@tabnas/bnf`.
3. **`VERSION` in `ts/src/ebnf.ts` MUST equal `ts/package.json`
   "version".** `ts/test/version.test.js` fails the build on drift.
4. **Compiler diagnostics keep the compiler's wording.** The facade
   restamps only the leading package prefix (`bnf:` / `abnf:` → `ebnf:`)
   so every error this package raises reads consistently; the rest of
   the message, which names the offending rule, is passed through
   untouched. The prefix has moved once already, which is why the
   replace matches both spellings.
5. **Every documented rejection has a test asserting both that it is
   refused and that the message names the construct or rule.** A
   rejection without such a test is not documented behaviour.

## Repo-specific gotchas

- **Whitespace is skipped between tokens.** Spec grammars are written
  for scannerless parsers and spell out whitespace themselves. A
  char-level `Name ::= [a-z]+` will therefore join two space-separated
  words. This is inherited from the engine, not fixable here; the docs
  point at the built-in `TX`/`NR`/`ST`/`VL` tokens instead.
- **String escapes are off.** `string.escapeChar` is pointed at DEL
  (`#x7F`) because W3C EBNF defines no escape sequences and the engine
  has no shared "escaping off" switch. Without it, `"\"` — a
  one-character literal, and a common one — would swallow its closing
  quote.
- **`value.lex` and `number.lex` are off** in the meta-grammar. `true`,
  `false` and `null` are ordinary symbol names in a published grammar,
  and EBNF has no numeric literals.
- **Not every production becomes a tree node.** The shared compiler
  folds a rule whose body is a single token segment into its caller.
  Tests that assert tree shape must be written against what the
  compiler emits, not against the source productions.
- **`.github/workflows/` still carries the scaffold's dependency list**
  (`deps: "parser debug json abnf railroad jsonic"`). It needs `bnf` and
  does not need most of the rest. Session credentials cannot write those
  files; a maintainer promotes the change.

## Build and test

```bash
cd ts
npm install     # links @tabnas/bnf from ../../bnf/ts
npm run build   # tsc --build src
npm test        # node --test test/**/*.test.js
```

The `@tabnas/bnf` devDependency is a `file:` path into the sibling
checkout. In an isolated clone that path dangles; install the published
version instead.
