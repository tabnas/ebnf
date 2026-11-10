# Agents Guide — the shared fixture

## `spec/tree.tsv`, the tree a host translates

[`spec/tree.tsv`](spec/tree.tsv) is the one shared fixture: each row an
EBNF document and its tree, the schema grammar-rules a host translates
([`../AGENTS.md`](../AGENTS.md), "Translation"), as the tree's JSON text,
member order included, compared as text; or `ERROR:<code>`, the code the
tree parser refuses it with ("Error codes" there). The columns are
`grammar` and `expected`. A `grammar` cell's `\n`, `\r`, `\t` and `\\`
are the characters they name, as tabnas-support reads a cell, so a
backslash the grammar holds is written doubled (`"\\"` is the string of
one backslash, which W3C EBNF reads as itself); any other backslash is
itself. A line starting with `#` is a comment, so a document that starts
with a `#x` code point starts with a space.

Each runtime reads the file with a loader of its own beside the tree's
other tests: `ts/test/tree.test.js`, `go/tree_test.go` and
`rs/tests/tree_test.rs`, which also hold every tree to tabnas/bnf's check
of the schema. A row's expected tree is what the tree should be, checked
against the schema and the notation, never copied from what one runtime
answers: a row that pins a mistake pins it in all three.

The compiler's own behaviour is not pinned here. Its rejections are
in-language, message by message, in `ts/test/ebnf.test.js`, and the Rust
port is held to TypeScript's output by `rs/tests/oracle_test.rs`.
