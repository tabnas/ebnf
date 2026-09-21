# Agents Guide — rs/

The Rust port of the canonical TypeScript in [`../ts`](../ts). Read
[`../AGENTS.md`](../AGENTS.md) first: it holds the cross-runtime rules
(TypeScript wins, what the dialect is and why, what this package checks
and what it does not, the bounded-lookahead limit, the version sites,
and how to treat untrusted input). This file only covers what is
specific to this crate.

## What is here, and what is not

This crate is the EBNF FRONT-END and nothing else. It parses EBNF text
into the notation-neutral grammar IR that
[`tabnas-bnf`](../../bnf/rs) compiles, exactly as `ts/src/converter.ts`
does:

```text
EBNF text ──parse_ebnf──▶ tabnas_bnf::Grammar ──emit_grammar_spec──▶ GrammarSpec
```

Everything downstream of that arrow (desugaring, left-recursion
elimination, tail repeats, probe dispatch, literal lifting, token
allocation, first-set analysis, chain emission) lives in `tabnas-bnf`. A
defect in the emitted grammar is almost always a defect there, not here,
and the fix belongs there, proven against the ABNF suite, which
exercises the shared compiler hardest.

What belongs here is the meta-grammar, the character-class and
code-point decoders, the named rejections, and the two soundness checks
(`check_duplicates`, `check_nullable_alts`).

## Layout

| Path | Mirrors |
|---|---|
| `src/lib.rs` | `ts/src/ebnf.ts` and `go/facade.go`: `VERSION`, `ebnf`, `ebnf_convert`, `to_spec`, `emit_grammar_spec`, `plugin`, `EbnfError`, `EbnfCompileError`, and the re-exports of the shared compiler under this package's own names |
| `src/parser_ebnf.rs` | the `ebnfRules` table plus `getEbnfParser` in `ts/src/converter.ts`: the meta-grammar as a tabnas grammar document, its engine options, the ISO comment matcher, the named rejections and the AST-assembly closures |
| `src/converter.rs` | the rest of `ts/src/converter.ts`: `parse_ebnf`, `check_duplicates`, `check_nullable_alts`, `EbnfParseError` |
| `src/classes.rs` | `parseCharClass`, `hexTerm` and `codePoint` |
| `tests/oracle_test.rs` | no twin: the canonical TypeScript, recorded |
| `tests/parity_test.rs` | the guard that says there are no shared fixtures to run, and fails when some land |
| `tests/ebnf_test.rs` | `ts/test/ebnf.test.js` and `go/ebnf_test.go` |
| `tests/spans_test.rs` | the `describe('source spans')` block of `ts/test/ebnf.test.js` |
| `tests/lookahead_test.rs` | `go/lookahead_test.go` and the `describe('the bounded-lookahead limit')` block |
| `tests/divergence_test.rs` | `go/divergence_test.go`, plus this port's own recorded divergences |
| `tests/untrusted_test.rs` | no twin: the boundaries a Rust port has to state, because a stack that runs out aborts rather than unwinding |
| `tests/perf_test.rs` | no twin: ratio comparisons, measured on one machine in one run |
| `tests/version_test.rs` | the five version sites must agree |
| `README.md` | the crate front page; its `rust` fences run as doctests |

## The port follows the TypeScript, not the Go

`go/parser_ebnf.go` is a hand-written recursive-descent scanner, and its
own file header calls that a deliberate divergence from the canonical
front-end. This crate does NOT follow it. The meta-grammar here is a
tabnas grammar document parsed by the engine itself, which is what
`ts/src/converter.ts` does, so a defect in one is far more likely to
appear in the other.

That choice is why the Go port's diagnostics for a lexical failure (an
unterminated string, a control character in a literal) are hand-written
here and come from the shared engine lexer, as they do in TypeScript.
Three of the four tests in `go/divergence_test.go` exist because that
scanner had to be ALIGNED to the engine's behaviour; this port gets that
behaviour for free, and `tests/divergence_test.rs` pins it anyway.

## The parse AST is engine values

`Rule::node` is a `tabnas::Value`, so the AST the meta-grammar's actions
build is a tree of `Value::Object`s in exactly the shape the IR
serializes to. `production_from_value` then deserializes it into
`tabnas_bnf::Production` with serde. That is deliberate, and it is the
closest mirror of the canonical runtime there is: TypeScript builds
plain objects with those same field names, so the two representations
cannot drift apart without serde saying so.

Three consequences.

**A rule's node is a shared cell.** A pushed rule INHERITS its parent's
`Rc<RefCell<Value>>`, so `set_node` rebinds the cell (TypeScript's
`r.node = []`) while `push_node` writes through it (TypeScript's
`r.node.push(...)`). Getting that backwards silently overwrites whatever
the parent was accumulating. Every `bo` hook that starts a fresh
collection uses `set_node`; `@item-bc`, `@prod-bc`, `@alts-bc` and
`@post-bc`, which append to a collection their parent owns, use
`push_node`.

**Numbers arrive as doubles.** The engine's number is an `f64`, so a
span offset reaches serde as `8.0`, which no `usize` field will take.
`integral` rewrites every whole number in the JSON tree before
deserialization. Nothing in the IR this front-end builds is fractional,
so the conversion is total rather than a heuristic.

**A `nodeKind` is written where TypeScript writes nothing.** The typed
`Production` carries the compiler's own fields and serializes
`"nodeKind": "user"` at its default. The oracle test normalizes that
away rather than recording it in every fixture entry; a field a
front-end actually SETS is never normalized, so a real difference still
shows.

## `elem` is an engine builtin name

The canonical rule table names the element rule `elem`. That name is not
available here. `@elem-bc` is one of the engine's own builtin action
references (see `builtins.rs` in the parser crate), a builtin wins the
name lookup, and the loader wires `@<rule>-bo` and `@<rule>-bc` onto a
rule by convention. A rule named `elem` therefore runs the engine's
list-element push instead of this crate's closure, and the symptom is
subtle: the IR gains a stray empty array after every element, and every
postfix operator is dropped.

The rule is named `item` for that reason, and
`the_rule_table_calls_the_element_rule_item` in
`tests/divergence_test.rs` pins both halves. The canonical front-end
hands the engine closures rather than named references, so the collision
cannot arise there. Before adding a rule, check its name against that
builtin list.

## Every matcher is eager, and why that is safe

Each `match.token` matcher here fires wherever its pattern matches,
rather than only where the current rule's token column already expects
it, because each pattern starts with a character that has exactly one
meaning in EBNF: `[` only opens a character class, `#` only opens a code
point (the `#` line comment is off), `::=` and `=` only define a
production, and `-` only subtracts, since a hyphen inside a name is
consumed by the text matcher as part of that name.

ABNF's converter has to do the opposite, listing tokens in `s:` patterns
to widen the lexer's token column, because its matchers are ambiguous
with a bareword. If a matcher whose first character has more than one
meaning is ever added here, that reasoning stops holding: either keep it
non-eager and widen the columns, or do not add it.

`(* … *)` is a match token, not a comment definition, for the same
reason it is in TypeScript: the fixed matcher would claim `(` as `#LP`
before a comment definition were offered the position. Unlike the
canonical matcher, this one does no row bookkeeping of its own, because
this engine's lexer advances one character at a time over the consumed
source and counts embedded newlines itself. The test that a multi-line
ISO comment does not shift later line numbers is kept regardless.

## The rejection channel

Every construct this dialect refuses is refused from inside the rule
action that read the offending token, exactly as the canonical
TypeScript throws from inside its `a:` closure. A Rust action can only
answer an `ActionError`, and the engine would wrap that with a position
and a code of its own, so the finished `EbnfParseError` (message, line
and column) is recorded on a thread local and the action returns an
error carrying `REJECT_CODE`. `parse_ebnf_raw` reads it back.

The FIRST rejection wins: the engine may try further alternatives, and
the diagnostic a reader wants is the one for the construct they actually
wrote. The slot is cleared at the start of every parse.

## The depth caps are measured, not guessed

Two caps, on two different resources, because bounding one and not the
other bounds nothing.

`MAX_GROUP_DEPTH` is 520 RULE levels, about 129 nested groups. It bounds
the ENGINE's own stack while the source is read, and is checked where a
group opens. The number was measured: on the unoptimised profile, in one
of the 2 MiB threads `cargo test` runs a test on, 240 nested groups
parses and 250 overflows the stack and ABORTS the process. The first
draft of this port used 2048, copied from the ABNF crate, and `cargo
test` aborted.

`MAX_NEST_DEPTH` is 130 NESTING levels. It bounds the TREE that reading
the source builds, which is what everything downstream of the parse
walks recursively: `Value::to_json`, `integral`, serde's `from_value`
and the default drop of a `Value`. Measured the same way: 390 stacked
postfix operators parses and 400 aborts, inside `from_value`.

The second cap exists because the first one does not reach. A group
costs four rule levels and a postfix operator costs one, so `A ::= "x"`
followed by four hundred question marks nests the IR 401 deep at a rule
depth of 406 -- nowhere near 520, and an uncatchable abort on untrusted
input. Counting only groups also missed the mixture: 129 nested groups,
which the group cap admits, carrying two operators each nest 388 deep
and aborted.

So the nesting cap counts a group and a postfix operator ALIKE, and is
checked as each group closes (`@atom-group-close`) and as each operator
is read (`@post-opt`, `@post-star`, `@post-plus`). Reading it there
rather than once the postfix chain has closed is deliberate: a chain is
one rule level per operator, so a source of thousands would otherwise
run the engine's stack out before any count could be taken.

The count is exact rather than an estimate. Two registers on the parse
context carry it: `ebnfNest` is the depth of the element that just
completed, and `ebnfLevelMax` is the deepest element at the current
group level, which a group takes one more than when it closes. A group
rule saves the enclosing level's value in its OWN `u` bag and puts it
back at close, so the register is stack disciplined without a stack.
`n` would not do: the engine inherits it DOWNWARD, and a postfix
operator at an outer level is read after its subtree has finished.

130 is deliberately one past 128, the shared compiler's own limit on
element nesting, so a grammar of 128 nested levels is refused BY THE
COMPILER with the better diagnostic, and these caps only catch what is
deeper still. `the_group_cap_admits_129_and_refuses_130`,
`the_nest_cap_admits_129_postfix_operators_and_refuses_130` and
`groups_and_postfix_operators_share_one_nesting_cap` in
`tests/untrusted_test.rs` are the tests that would have caught each
draft: every one READS a grammar at the cap rather than only asserting
that a deeper one is refused.

## Parity is held against a recorded oracle

This repository ships no shared `test/spec` fixtures, and declares no
error codes, so there is nothing for `tabnas_support::Runner` to read.
`tests/oracle/ebnf-ir.json` stands in: for eighty-five EBNF sources it
records what `ts/dist` answered, the IR, the emitted rule names, the
empty-input verdict, and the rejection with its line and column.

Regenerate it with, from the repository root:

```bash
(cd ts && npm run build)
node rs/tests/oracle/gen-oracle.js
```

Add a source to the array in that script rather than editing the JSON by
hand. A generated fixture nobody can regenerate is a fixture nobody can
trust.

`tests/parity_test.rs` is the guard: the day shared fixtures DO land it
fails and names them, so they cannot sit unread while a `parity_test.rs`
that runs nothing reports green.

## Verify your work

From `rs/`:

```bash
cargo fmt --check
cargo test --all-targets
cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
```

`--all-targets` does NOT include doctests, and `README.md` is doctested,
so both test commands are needed. From the repository root,
`ci/rust/run.sh` runs the whole gate including the `Cargo.lock` check,
and `make test-rs` is the fast inner loop.

`README.md` is in the gated prose set (`ts/scripts/gated-docs.cjs`), so
a change to it has to keep `make prose` green and the counts in
`.vale.ini` and `docs/STYLE-GUIDE.md` re-measured with
`make prose-counts`.
