// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The bounded-lookahead limit, pinned behaviourally.
//
// `ts/test/ebnf.test.js`, `describe('the bounded-lookahead limit')`, and
// `go/lookahead_test.go` are the other halves; each test below names its
// counterpart by exact string so a grep from either side lands on a real
// test.
//
// Every other test in this crate stops at the IR or the GrammarSpec.
// That is enough for what the front-end accepts, but the limit is not a
// property of the front-end: it is where the shared compiler's reach
// runs out, and that only shows when input is actually parsed. So these
// install the grammar on an engine and parse, which is what makes them
// worth having separately.
//
// `AGENTS.md`, "The dialect decision", requires that a change in the
// compiler's reach moves these tests and all four documents in the same
// change. Measured 2026-09-21: every case below agrees with TypeScript
// and with the Go port, rejections included.

mod common;

use common::engine_for;
use tabnas::Tabnas;

fn engine(src: &str) -> Tabnas {
    engine_for(src).unwrap_or_else(|error| panic!("{src:?} failed to compile: {error}"))
}

fn accepts(parser: &Tabnas, input: &str) {
    if let Err(error) = parser.parse(input) {
        panic!("parse({input:?}): want accept, got {error}");
    }
}

/// Pin the REASON, not just the refusal. A grammar that failed to
/// compile, or one that refused every input, would satisfy a bare "it
/// errored" assertion while proving nothing about lookahead.
fn rejects_unexpected(parser: &Tabnas, input: &str) {
    let error = parser
        .parse(input)
        .err()
        .unwrap_or_else(|| panic!("parse({input:?}): want reject, got accept"));
    assert!(
        error.to_string().contains("unexpected"),
        "parse({input:?}): want an `unexpected` rejection, got {error}"
    );
}

/// TS: 'an unbounded shared prefix is left-factored automatically'.
/// Go: `TestUnboundedSharedPrefixIsLeftFactored`.
#[test]
fn an_unbounded_shared_prefix_is_left_factored_automatically() {
    let parser = engine("S ::= L \"x\" | L \"y\"\nL ::= \"a\"+");
    accepts(&parser, "a x");
    accepts(&parser, "a y");
    accepts(&parser, "a a x");
    // The `y` branch needs a decision past an unbounded run of `a`.
    accepts(&parser, "a a y");
}

/// TS: 'left-factoring by hand works the same'.
/// Go: `TestLeftFactoringByHandWorksTheSame`.
#[test]
fn left_factoring_by_hand_works_the_same() {
    let parser = engine("S ::= L ( \"x\" | \"y\" )\nL ::= \"a\"+");
    accepts(&parser, "a a a y");
}

/// TS: 'a shared prefix behind distinct recursive rules is still the
/// limit'.
/// Go: `TestSharedPrefixBehindDistinctRecursiveRulesIsStillTheLimit`.
///
/// Left factoring is structural, so two distinct rules spelling the same
/// unbounded prefix cannot merge. The limit that leaves is asymmetric,
/// and that asymmetry is the part worth pinning: A and B are the same
/// shape, spelled the same way, so what separates them is only which
/// alternative each sits behind.
#[test]
fn a_shared_prefix_behind_distinct_recursive_rules_is_still_the_limit() {
    let parser = engine("S ::= A \"x\" | B \"y\"\nA ::= \"a\" A | \"a\"\nB ::= \"a\" B | \"a\"");
    accepts(&parser, "a x");
    accepts(&parser, "a y");
    // The first alternative carries any depth.
    accepts(&parser, "a a x");
    accepts(&parser, "a a a a a x");
    // The second does not, at any depth past one.
    rejects_unexpected(&parser, "a a y");
    rejects_unexpected(&parser, "a a a a a y");
}
