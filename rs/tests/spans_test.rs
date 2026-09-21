// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// Source spans, the Rust half of the `describe('source spans')` block in
// `ts/test/ebnf.test.js`. The front-end records where each element and
// production came from, so a compile failure can carry a range and a
// tool can underline the offending text.
//
// Every assertion below slices the ORIGINAL SOURCE with the span and
// compares the text. That is the only check worth making: an offset pair
// that is self-consistent but points at the wrong characters would
// satisfy any assertion about the numbers themselves.

mod common;

use serde_json::Value as JsonValue;
use tabnas_ebnf::{ebnf_convert, parse_ebnf, EbnfError};

const SRC: &str = concat!(
    "doc ::= item\n",
    "item ::= \"hi\" | ref | (alt | two)\n",
    "ref ::= [a-z]\n",
    "alt ::= #x41\n",
    "two ::= \"z\""
);

fn grammar() -> JsonValue {
    serde_json::to_value(parse_ebnf(SRC).expect("parses")).expect("the IR serializes")
}

/// The text a span covers.
fn text(span: &JsonValue) -> &'static str {
    let start = span["s"].as_u64().expect("a span has an offset") as usize;
    let end = span["e"].as_u64().expect("a span has an end") as usize;
    &SRC[start..end]
}

/// One production of the grammar, by name.
fn production(name: &str) -> JsonValue {
    grammar()["productions"]
        .as_array()
        .expect("productions")
        .iter()
        .find(|production| production["name"] == name)
        .unwrap_or_else(|| panic!("no production {name}"))
        .clone()
}

#[test]
fn spans_a_production_with_its_name() {
    // The name, not the body: that is what an outline entry and
    // go-to-definition want, and a body can run over many lines.
    let item = production("item");
    assert_eq!(text(&item["sp"]), "item");
    assert_eq!(
        item["sp"]["r"], 2,
        "the row is 1-based, as the engine reports"
    );
    assert_eq!(item["sp"]["c"], 1);
}

#[test]
fn spans_a_string_terminal_including_its_quotes() {
    assert_eq!(text(&production("item")["alts"][0][0]["sp"]), "\"hi\"");
}

#[test]
fn spans_a_rule_reference() {
    assert_eq!(text(&production("item")["alts"][1][0]["sp"]), "ref");
}

#[test]
fn spans_a_group_from_its_opening_paren_to_its_closing_one() {
    let group = production("item")["alts"][2][0].clone();
    assert_eq!(group["kind"], "group");
    assert_eq!(text(&group["sp"]), "(alt | two)");
    // ...and the elements inside carry their own, narrower spans.
    assert_eq!(text(&group["alts"][0][0]["sp"]), "alt");
    assert_eq!(text(&group["alts"][1][0]["sp"]), "two");
}

#[test]
fn spans_a_character_class_and_a_hex_terminal() {
    assert_eq!(text(&production("ref")["alts"][0][0]["sp"]), "[a-z]");
    assert_eq!(text(&production("alt")["alts"][0][0]["sp"]), "#x41");
}

#[test]
fn reports_a_row_and_column_that_agree_with_the_offset() {
    // A span whose offset and row and column disagree is worse than no
    // span: a consumer picking either one gets a different answer.
    for production in grammar()["productions"].as_array().expect("productions") {
        let span = &production["sp"];
        let start = span["s"].as_u64().expect("an offset") as usize;
        let before = &SRC[..start];
        let row = before.split('\n').count();
        let column = start - before.rfind('\n').map_or(0, |index| index + 1) + 1;
        assert_eq!(
            span["r"], row,
            "{}: the row disagrees with the offset",
            production["name"]
        );
        assert_eq!(
            span["c"], column,
            "{}: the column disagrees with the offset",
            production["name"]
        );
    }
}

#[test]
fn gives_a_compile_error_a_range_that_underlines_the_offender() {
    // The whole point of the feature, end to end: a real grammar, a real
    // compile failure, and a range a tool can pass to an editor without
    // parsing anything out of the message.
    let src = "doc ::= item\nitem ::= missing";
    let error = ebnf_convert(src, None).expect_err("the unknown reference is refused");
    let EbnfError::Compile(compile) = error else {
        panic!("wanted a compile failure, got {error}");
    };
    let span = compile.sp.expect("the compile error carried no range");
    assert_eq!(
        &src[span.s..span.e],
        "missing",
        "the range does not cover the unknown reference"
    );
    assert_eq!(span.r, Some(2));
    assert_eq!(span.c, Some(10));
}

#[test]
fn does_not_change_the_grammar_a_spanned_parse_compiles_to() {
    // Spans are metadata: they must not reach the emitted GrammarSpec.
    let spec = ebnf_convert(SRC, None).expect("compiles");
    let rules = serde_json::to_string(
        &spec
            .rule
            .iter()
            .map(|(name, rule)| {
                (
                    name.clone(),
                    rule.as_ref()
                        .map_or(JsonValue::Null, tabnas_ebnf::RuleSpec::to_value),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    )
    .expect("the rules serialize");
    assert!(!rules.contains("\"sp\""), "a span reached the spec");
}
