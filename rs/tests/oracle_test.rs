// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The canonical TypeScript front-end is the oracle for this port.
//
// This repository ships NO shared `test/spec` fixtures -- there is no
// repo-root `test/` at all, and `AGENTS.md` says so under "Error codes"
// -- so there is nothing for `tabnas_support::Runner` to read and no
// `ERROR:<code>` contract to hold this port to. What exists instead is
// the canonical implementation itself, so parity is measured against it
// directly: `tests/oracle/ebnf-ir.json` records, for eighty-five EBNF
// sources, exactly what `ts/dist` answered -- the grammar IR, the rule
// names and the empty-input verdict of the compiled spec, or the
// rejection and the line and column it named.
//
// Regenerate it with:
//
//     node rs/tests/oracle/gen-oracle.js
//
// from the repository root, after `cd ts && npm run build`. The script
// is committed beside the fixture so the measurement can be repeated
// rather than believed.
//
// The corpus is every source the TypeScript suite exercises, the four
// `.ebnf` fixtures, and the documented rejections, plus malformed input
// the suite does not reach. It is deliberately ASCII: a span's offsets
// count UTF-16 code units in TypeScript and bytes here, which is the
// divergence `DIVERGENCE.md` records, and an ASCII corpus measures
// everything else without tripping over it. `tests/divergence_test.rs`
// pins the non-ASCII half on its own.

mod common;

use serde_json::Value as JsonValue;

use common::{convert, normalize_ir, rule_names, strip_offsets};
use tabnas_ebnf::parse_ebnf;

/// One oracle entry: the source, and what TypeScript answered.
fn entries() -> Vec<JsonValue> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("oracle")
        .join("ebnf-ir.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).expect("the oracle fixture is JSON")
}

fn text<'a>(entry: &'a JsonValue, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(JsonValue::as_str)
}

/// The engine's own diagnostic is not the front-end's, and the two
/// runtimes render it differently: TypeScript colours it with ANSI
/// escapes and names its own token text. What both DO promise is the
/// `ebnf: parse error at line L, column C:` prefix, which is this
/// front-end's, so that is what a generic parse failure is compared on.
fn generic_prefix(message: &str) -> Option<&str> {
    message
        .starts_with("ebnf: parse error")
        .then(|| message.split(':').next().unwrap_or(message))?;
    message.find(':').map(|end| &message[..end])
}

#[test]
fn the_ir_matches_the_typescript_oracle() {
    let mut checked = 0;
    for entry in entries() {
        let src = text(&entry, "src").expect("every entry names a source");
        let Some(want) = entry.get("ir") else {
            continue;
        };
        let grammar = parse_ebnf(src)
            .unwrap_or_else(|error| panic!("{src:?}: TypeScript parses this, and got: {error}"));
        let got = serde_json::to_value(&grammar).expect("the IR serializes");
        // A source carrying a non-ASCII character is compared without
        // the offset half of its spans: those count UTF-16 code units
        // in TypeScript and bytes here. Row and column are compared
        // either way, and `tests/divergence_test.rs` pins the boundary
        // itself.
        let (got, want) = if src.is_ascii() {
            (normalize_ir(&got), normalize_ir(want))
        } else {
            (
                strip_offsets(&normalize_ir(&got)),
                strip_offsets(&normalize_ir(want)),
            )
        };
        assert_eq!(
            got, want,
            "{src:?}: the IR differs from the TypeScript oracle"
        );
        checked += 1;
    }
    assert!(20 < checked, "the oracle carries {checked} parsed grammars");
}

#[test]
fn every_rejection_matches_the_typescript_oracle() {
    let mut checked = 0;
    for entry in entries() {
        let Some(want) = text(&entry, "error") else {
            continue;
        };
        let error = match parse_ebnf(text(&entry, "src").expect("a source")) {
            Ok(_) => panic!(
                "{:?}: TypeScript refuses this, and this port accepted it. Wanted: {want}",
                text(&entry, "src")
            ),
            Err(error) => error,
        };

        if want.starts_with("ebnf: parse error") {
            // A generic engine rejection: the runtimes agree on the
            // front-end's own prefix and on WHERE, not on the engine's
            // rendered text.
            assert!(
                error.message.starts_with("ebnf: parse error"),
                "{:?}: wanted a generic parse error, got {}",
                text(&entry, "src"),
                error.message
            );
            assert_eq!(
                generic_prefix(&error.message),
                generic_prefix(want),
                "{:?}: the rejection reads differently",
                text(&entry, "src")
            );
        } else {
            assert_eq!(
                error.message,
                want,
                "{:?}: the rejection reads differently",
                text(&entry, "src")
            );
        }

        if let Some(line) = entry.get("line").and_then(JsonValue::as_u64) {
            assert_eq!(
                error.line,
                Some(line as usize),
                "{:?}: the rejection names a different line",
                text(&entry, "src")
            );
        }
        if let Some(column) = entry.get("column").and_then(JsonValue::as_u64) {
            assert_eq!(
                error.column,
                Some(column as usize),
                "{:?}: the rejection names a different column",
                text(&entry, "src")
            );
        }
        checked += 1;
    }
    assert!(30 < checked, "the oracle carries {checked} rejections");
}

#[test]
fn the_compiled_spec_matches_the_typescript_oracle() {
    let mut checked = 0;
    for entry in entries() {
        let src = text(&entry, "src").expect("a source");
        if let Some(want) = text(&entry, "compileError") {
            let error = convert(src)
                .err()
                .unwrap_or_else(|| panic!("{src:?}: TypeScript refuses this at compile time"));
            assert_eq!(
                error.to_string(),
                want,
                "{src:?}: the compile rejection reads differently"
            );
            checked += 1;
            continue;
        }
        let Some(want) = entry.get("rules").and_then(JsonValue::as_array) else {
            continue;
        };
        let spec = convert(src).unwrap_or_else(|error| panic!("{src:?}: {error}"));
        let names: Vec<JsonValue> = rule_names(&spec).into_iter().map(JsonValue::from).collect();
        assert_eq!(&names, want, "{src:?}: the emitted rule names differ");

        // Whether the empty string is in the language is settled at
        // compile time, not by the rules: the engine short-circuits `''`
        // before the parse loop starts, so no rule ever sees it.
        if let Some(empty) = entry.get("emptyOk").and_then(JsonValue::as_bool) {
            let got = spec
                .options
                .get("lex")
                .and_then(|lex| lex.get("empty"))
                .and_then(JsonValue::as_bool);
            assert_eq!(got, Some(empty), "{src:?}: the empty-input verdict differs");
        }
        checked += 1;
    }
    assert!(
        20 < checked,
        "the oracle carries {checked} compiled grammars"
    );
}
