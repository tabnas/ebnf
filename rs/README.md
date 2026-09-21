# tabnas-ebnf (Rust)

An EBNF grammar front-end for the
[`tabnas`](https://github.com/tabnas/parser) parsing engine, crate
`tabnas_ebnf`.

The grammar it installs is whatever EBNF text it is handed at run time.
Feed it the grammar a specification publishes and the engine parses that
language.

```text
EBNF text ──parse_ebnf──▶ Grammar ──emit_grammar_spec──▶ GrammarSpec
```

The first arrow is what this crate adds: the meta-grammar that reads
EBNF syntax, the character-class and code-point decoders, and the named
rejections for constructs the IR cannot express. Everything downstream
of that IR lives in [`tabnas-bnf`](https://github.com/tabnas/bnf) and is
shared with the ABNF and GBNF front-ends: desugaring, left-recursion
elimination, tail repeats, probe dispatch, literal lifting, token
allocation and chain emission.

This is the Rust port of the canonical TypeScript implementation in
[`../ts`](../ts); the TypeScript version is authoritative and this crate
tracks it. The Go port in [`../go`](../go) reads the same dialect.

## Which EBNF

"EBNF" names a family, not a language. ISO/IEC 14977, W3C, Wirth and a
long tail of per-tool dialects disagree on the definition operator, the
terminator, the comment syntax and, worst, on what brackets mean.

The primary dialect here is **W3C EBNF**, the notation the XML, XPath
and XQuery specifications publish their grammars in, because it has a
real published corpus and because its operators map one for one onto the
IR. Four ISO/IEC 14977 spellings are accepted on top, and only the four
that cannot collide with the W3C reading.

| Construct | Spelling |
|---|---|
| definition | `Name ::= body`, and the ISO `Name = body` |
| choice | `\|` |
| concatenation | juxtaposition, and the ISO `,` between two items |
| literal | `"text"` or `'text'`, case-SENSITIVE |
| character class | `[a-z]`, `[^<&]`, `[#x20-#x7E]`, `[#x9#xA#xD]` |
| code point | `#x41`, lowercase `x`, either case of hex digit |
| repetition | postfix `A?`, `A*`, `A+` |
| grouping | `( A \| B )` |
| comment | `/* … */`, and the ISO `(* … *)` |
| terminator | the ISO `;`, optional and ignored |

This is a **best-effort** front-end. A grammar using only the constructs
above compiles; anything else raises a named error rather than silently
compiling to the wrong language. The itemised list of what is not
supported is in [`../ts/doc/reference.md`](../ts/doc/reference.md), and
the reasoning behind the dialect is in
[`../ts/doc/concepts.md`](../ts/doc/concepts.md).

## Use

Compile a grammar and install it on an engine:

```rust
use tabnas::Tabnas;
use tabnas_ebnf::ebnf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Tabnas::new();
    ebnf(&mut parser, "Greet ::= \"hi\" | \"hello\"", None)?;

    let tree = parser.parse("hello")?;
    assert_eq!(tree.to_json()["rule"], "Greet");
    Ok(())
}
```

Use a fresh instance per grammar: installing applies lexer settings as
well as rules, and those are instance-wide.

A grammar builds a parse tree by default, one `{rule, src, kids}` node
per rule the author wrote:

```rust
use tabnas::Tabnas;
use tabnas_ebnf::ebnf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "List ::= \"[\" Item ( \",\" Item )* \"]\"\nItem ::= [a-z]+";
    let mut parser = Tabnas::new();
    ebnf(&mut parser, source, None)?;

    let tree = parser.parse("[a,b,c]")?.to_json();
    assert_eq!(tree["src"], "[a,b,c]");
    assert_eq!(tree["kids"].as_array().map(Vec::len), Some(3));
    assert_eq!(tree["kids"][1]["src"], "b");
    Ok(())
}
```

To build the grammar without installing it, use `ebnf_convert`, which is
what `tn.ebnf.toSpec` does in the canonical package:

```rust
use tabnas_ebnf::{ebnf_convert, EbnfConvertOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = EbnfConvertOptions {
        start: Some("Second".to_string()),
        ..EbnfConvertOptions::default()
    };
    let spec = ebnf_convert("First ::= \"a\"\nSecond ::= \"b\"", Some(&options))?;

    assert!(spec.rule.contains_key("Second"));
    let mut parser = tabnas::Tabnas::new();
    spec.install(&mut parser)?;
    assert_eq!(parser.parse("b")?.to_json()["rule"], "Second");
    Ok(())
}
```

## The IR, and where a node came from

`parse_ebnf` answers the grammar IR on its own, for a caller that wants
to inspect or rewrite it before compiling. Every element and every
production records where in the source it came from, so a compile
failure carries a range and a tool can underline the offending text:

```rust
use tabnas_ebnf::parse_ebnf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "Item ::= \"hi\" | (alt | two)\nalt ::= #x41\ntwo ::= \"z\"";
    let grammar = parse_ebnf(source)?;

    let item = &grammar.productions[0];
    let span = item.sp.expect("a production records its name");
    // A production is spanned by its NAME, which is what an outline
    // entry and go-to-definition want; a body can run over many lines.
    assert_eq!(&source[span.s..span.e], "Item");

    let group = &item.alts[1][0];
    let span = group.sp.expect("a group records its brackets");
    assert_eq!(&source[span.s..span.e], "(alt | two)");
    Ok(())
}
```

A span's offsets are in the units the engine's own tokens use, which is
a byte here and a UTF-16 code unit in the canonical TypeScript. Slicing
the original source with a span gives the same text in either runtime,
which is what a consumer wants a span for. See
[`../DIVERGENCE.md`](../DIVERGENCE.md).

## Left recursion, and what the compiler reaches

Left recursion is rewritten automatically, `P ::= P a | b` becoming
`P ::= b (a)*`:

```rust
use tabnas::Tabnas;
use tabnas_ebnf::ebnf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Tabnas::new();
    ebnf(&mut parser, "Expr ::= Expr \"+\" Term | Term\nTerm ::= NR", None)?;

    let tree = parser.parse("1+2+3")?.to_json();
    assert_eq!(tree["rule"], "Expr");
    assert_eq!(tree["src"], "1+2+3");
    Ok(())
}
```

The engine is deterministic, with bounded lookahead plus a probe for one
optional-prefix shape, and the shared compiler left-factors a shared
prefix the dispatcher cannot see past. What remains out of reach depends
on the grammar shape AND the input depth, so the limit is pinned by
`rs/tests/lookahead_test.rs` rather than described here; the four
documents under [`../ts/doc`](../ts/doc) carry the explanation.

## What this front-end checks

Two checks live here because they are sound and cheap. A repeated symbol
is refused, since EBNF has no incremental-alternatives operator and the
compiler would otherwise take whichever definition it saw last. Two
alternatives of one production that both derive the empty string are
refused as well, because the grammar itself is then ambiguous and no
lookahead can choose between them.

There is deliberately no general ambiguity or backtracking check. Any
static rule sharp enough to reject the shapes that do fail also rejects
grammars that work at any depth.

## Options

| Option | Effect |
|---|---|
| `start` | Start rule name (default: the first production). |
| `tag` | Group tag stamped on every emitted alt, and the prefix of every diagnostic (default `ebnf`). |
| `builtins` | Emit probe dispatch and tree building as engine `$`-builtin refs instead of closures, keeping the grammar function-free and serializable. |
| `marks` | Emit a stable mark per user-rule alt, enabling `@<rule>:o\|c:<mark>` action references. |
| `word_keywords` | Treat word-like literals as whole-word keywords, so `"option"` does not match the prefix of `optional`. |
| `provenance` | Emit the map from each generated rule name back to the production it came from. On by default. |

## Install

The `tabnas` and `tabnas-bnf` crates are not published to a registry, so
both are consumed as **sibling checkouts**, the standard tabnas
development model. Clone `https://github.com/tabnas/parser` and
`https://github.com/tabnas/bnf` next to this repository and point at
them:

```toml
[dependencies]
tabnas-ebnf = { path = "../ebnf/rs" }
tabnas = { path = "../parser/rs" }
```

Both entries are needed. A crate's dependencies are not passed on to its
dependents, so `tabnas-ebnf` alone does not put `tabnas` in the extern
prelude, and the examples above that name `tabnas::Tabnas` would not
resolve.

## Differences from the canonical TypeScript

The IR this front-end builds is held to the TypeScript front-end's own
output over an eighty-five source corpus by `rs/tests/oracle_test.rs`:
the IR itself, the emitted rule names, the empty-input verdict, and
forty rejections with the line and column each names. Thirty-five of
those rejections are this front-end's own and are compared byte for
byte; the other five are the engine's, whose rendered text differs
between runtimes, and are compared on the prefix and the position. The
differences below are in the surface and at the edges of Unicode, not in
the dialect:

- **A failure is returned, never raised.** `parse_ebnf`, `ebnf_convert`,
  `to_spec` and `ebnf` all answer a `Result`. The diagnostics themselves
  are the same text, which the oracle compares byte for byte.
- **There is no instance decoration.** TypeScript adds a callable
  `tn.ebnf` member to the engine; Rust has no such thing, so the install
  path is the free function `ebnf(&mut parser, src, opts)` and the
  convert-only path is `ebnf_convert(src, opts)`.
- **Source spans count bytes**, where TypeScript counts UTF-16 code
  units. A column counts Unicode scalar values, which agrees with
  TypeScript everywhere in the Basic Multilingual Plane.
- **A surrogate code point becomes the replacement character.** `#xD800`
  names half of a UTF-16 pair, which no Rust `String` can hold;
  TypeScript answers a lone surrogate. The Go port does the same as this
  one.
- **Nested groups are refused past a documented cap.** A grammar arrives
  from outside the system, the parse tree nests once per group, and a
  Rust stack that runs out aborts the process rather than unwinding. The
  cap admits 129 nested groups, one more than the shared compiler's own
  limit on element nesting, so the compiler's better diagnostic is still
  the one a grammar meets first.
- **One rule of the meta-grammar is named differently.** `ebnf_rules()`
  calls the element rule `item` where the canonical table calls it
  `elem`, because `@elem-bc` is one of the engine's own builtin action
  references. Nothing about the IR changes.

Each difference is measured and recorded in
[`../DIVERGENCE.md`](../DIVERGENCE.md), and pinned by
`rs/tests/divergence_test.rs`.

## Build and test

Both dependencies are path dependencies on sibling checkouts, so there
is nothing to fetch:

```bash
cargo test --all-targets
cargo test --doc
```

Or, from the repository root, `make test-rs`. For what CI would say,
including formatting and the `Cargo.lock` check, run `ci/rust/run.sh`.

The suite ports the TypeScript and Go unit suites, holds the front-end
to the TypeScript oracle in `rs/tests/oracle/ebnf-ir.json`, and pins the
bounded-lookahead limit, the source spans, the recorded divergences and
the behaviour on untrusted input.

## License

MIT.
