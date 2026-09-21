# Divergences

Where a port produces a different result from the canonical TypeScript
in `ts/`, for the same input. Every row below was MEASURED, on
2026-09-21, by running the three implementations over the input in its
first column; nothing here is inferred from reading the source.

Each entry names who owns the repair. An entry that closes must be
deleted, and the test that pins it fails until it is, so this file
cannot go stale without the suite saying so.

The DIALECT is not a divergence. The IR the Rust front-end builds is
held to the TypeScript front-end's own output over an eighty-five source
corpus, rejection messages and source positions included, by
`rs/tests/oracle_test.rs`. What is below is what that comparison had to
set aside, and why.

## Where the divergences are pinned

There is no executable register in `test/spec` for these, because this
repository has no `test/spec` directory and no repo-root `test/` at all:
it declares no error codes, and what pins its behaviour is in-language,
message by message (`AGENTS.md`, "Error codes"). Two of the five below
are invisible to any grammar-to-output fixture, and one is about the
shape of the API rather than about a value.

So each is pinned by a Rust test in `rs/tests/divergence_test.rs`,
asserted in BOTH directions: the behaviour recorded here, and the
behaviour the canonical runtime has, so a port that starts agreeing
fails as loudly as one that starts disagreeing.

## 1. A code point naming a lone surrogate

`#xD800` names one half of a UTF-16 surrogate pair. A JavaScript string
can hold one; a Rust `String` and a Go `string` cannot.

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `A ::= #xD800` | `literal` is U+D800 | `literal` is U+FFFD | `literal` is U+FFFD |

**Reason.** `String.fromCodePoint(0xD800)` answers a lone surrogate.
`char::from_u32(0xD800)` answers `None`, because a `char` is a Unicode
scalar value by definition, and `string(rune(0xD800))` in Go yields the
replacement character for the same reason. There is no representation to
port to.

**Owner.** Nobody, unless the notation gains a way to mean this. A
surrogate code point names no character, so a grammar that matches one
matches nothing a well-formed document can contain. The W3C grammars
this dialect is for write `[#x0-#xD7FF]` and `[#xE000-#xFFFD]` around
the surrogate block precisely to exclude it.

## 2. Source span offsets count bytes

A span records where an element came from, in the units the front-end's
own engine tokens use. That is a UTF-16 code unit in TypeScript and a
byte in Go and Rust.

Measured over `A ::= "é" B` followed by `B ::= "y"`, the span of the `B`
reference:

| field | TypeScript | Go | Rust |
|---|---|---|---|
| `s`, `e` | 10, 11 | 11, 12 | 11, 12 |

**Reason.** The engine already records token positions this way, and the
front-end copies every field straight across precisely so that no
arithmetic, and so no off-by-one, happens at the boundary. Converting
would mean re-deriving a position the engine already knows.

**Owner.** Nobody: this is the engine's unit, recorded for it. Slicing
the original source with a span gives the same TEXT in every runtime,
which is what a consumer wants and what `rs/tests/spans_test.rs`
asserts. A consumer that treats a span as a UTF-16 offset is the case
this entry exists to warn.

## 3. A column counts Unicode scalar values

A column is the engine's, and the three engines count differently.

Measured over `A ::= "😀" B` followed by `B ::= "y"`, and again with `é`
in place of the emoji, the column of the `B` reference:

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `"é"` | 11 | 12 | 11 |
| `"😀"` | 12 | 14 | 11 |

**Reason.** TypeScript counts UTF-16 code units, Go counts bytes, and
this engine counts Unicode scalar values. The three agree on ASCII. Rust
agrees with TypeScript everywhere in the Basic Multilingual Plane, and
parts company past it, where one character is two UTF-16 code units; Go
parts company at any non-ASCII character.

**Owner.** Nobody, at the front-end level: the column is the engine's,
and `go/parser_ebnf.go` records the same thing about its own scanner.
A consumer that positions an editor caret from a column has to know
which runtime produced it.

## 4. Nested groups are refused sooner

A grammar arrives from outside the system, the parse tree nests once per
group, and a Rust stack that runs out ABORTS the process rather than
unwinding. Two caps apply, and the lower one is the shared compiler's.

| input | TypeScript | Go | Rust |
|---|---|---|---|
| `top ::= ( ( … "x" … ) )`, 127 deep | compiles | compiles | compiles |
| the same, 128 deep | compiles | compiles | refused: `ebnf: rule 'top' nests elements more than 128 deep, which is past what this compiler will walk. Split the rule into named rules.` |
| the same, 130 deep | compiles | compiles | refused: `ebnf: grammar nests too deeply (more than 520 rule levels, about 130 nested groups)` |
| the same, 5000 deep | refused: `Maximum call stack size exceeded` | compiles | refused, as above |

**Reason.** The 128 limit is `MAX_ELEMENT_DEPTH` in `tabnas-bnf`, which
this crate inherits: the compiler's passes over an element are
recursive, and that port bounds them rather than trusting the stack. The
520 limit is this crate's own, on the front-end, and stops a source
building a parse tree deep enough to overflow the stack on the way back
out. It was MEASURED: on the unoptimised profile, in a 2 MiB thread, 240
nested groups parses and 250 aborts the process.

The cap admits 129 groups, one more than the shared compiler's own
limit, so a grammar that nests 128 deep still meets the compiler's
diagnostic, which names the rule and the limit.

**Owner.** `tabnas-bnf` owns the 128 limit and records it in its own
`rs/README.md`. This crate owns the 520 limit. Neither is reachable by
EBNF an author writes: the deepest grammar in the XML, XPath and XQuery
specifications nests nowhere near it.

## 5. A failure is returned, never raised

`parse_ebnf`, `ebnf_convert`, `to_spec` and `ebnf` all answer a
`Result`. TypeScript throws `EbnfParseError` or `EbnfCompileError`, and
Go returns as Rust does.

TypeScript also DECORATES an engine instance with a callable `tn.ebnf`
member, carrying `tn.ebnf.toSpec`. Rust has neither exceptions nor
dynamic instance properties, so the install path is the free function
`ebnf(&mut parser, src, opts)` and the convert-only path is
`ebnf_convert(src, opts)`, which is what `tn.ebnf.toSpec` does.

One further surface difference, invisible to any value: `ebnf_rules()`
names the element rule `item` where the canonical table names it `elem`.
`@elem-bc` is one of the engine's own builtin action references and a
builtin wins the name lookup, so a rule named `elem` would run the
engine's list-element push instead of this crate's closure. The
canonical front-end hands the engine closures rather than named
references, so the collision cannot arise there, and the IR is
identical either way.

**Reason.** Rust has no exceptions and no dynamic instance properties.

**Owner.** Nobody. The diagnostic TEXT is identical in both runtimes,
which is the part that is a contract. `rs/tests/oracle_test.rs` compares
forty rejections against what `ts/dist` rendered, with the line and
column each names; the thirty-five this front-end writes itself are
compared byte for byte, and the five the ENGINE writes are compared on
the `ebnf: parse error at line L, column C:` prefix this front-end adds,
because the engine's own rendered text differs between runtimes.
