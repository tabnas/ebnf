// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The Rust half of `ts/test/ebnf.test.js`, section for section: the
// converter's output shape, the IR the front-end builds, every W3C and
// accepted-ISO construct end to end, every documented rejection, left
// recursion, the realistic fixtures, the plugin, and the empty-input
// decision.
//
// `tests/oracle_test.rs` holds the same front-end to the canonical
// TypeScript output over a larger corpus. This file is the READABLE
// half: it says what each behaviour is for, in the canonical suite's
// own order, so a reader can see what the port promises without
// decoding a fixture.

mod common;

use common::{
    accepts_empty, convert, convert_with, engine_for, fixture, install, parse_with, rule_names,
    tree,
};
use serde_json::{json, Value as JsonValue};
use tabnas_ebnf::{
    ebnf, ebnf_convert, emit_grammar_spec, parse_ebnf, to_spec, EbnfConvertOptions, EbnfError,
};

/// The IR of one source, with every source span removed.
///
/// Most assertions here are about the SHAPE the front-end builds, or
/// about two spellings producing the same structure. Neither has
/// anything to say about where in the source a node came from, and one
/// of them compares parses of DIFFERENT strings, whose offsets
/// legitimately differ. Spans are asserted on their own, in
/// `tests/spans_test.rs`.
fn ir(src: &str) -> JsonValue {
    let grammar = parse_ebnf(src).unwrap_or_else(|error| panic!("{src:?}: {error}"));
    no_spans(&serde_json::to_value(&grammar).expect("the IR serializes"))
}

fn no_spans(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(no_spans).collect()),
        JsonValue::Object(entries) => JsonValue::Object(
            entries
                .iter()
                .filter(|(key, item)| "sp" != key.as_str() && !item.is_null())
                .filter(|(key, item)| {
                    !("nodeKind" == key.as_str() && Some("user") == item.as_str())
                })
                .map(|(key, item)| (key.clone(), no_spans(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The productions of a parsed grammar, spans stripped.
fn prods(src: &str) -> JsonValue {
    ir(src)["productions"].clone()
}

/// Strip the emitter-injected action references from an alt list, so a
/// test can assert the structural shape without pinning the action
/// identities.
fn strip_actions(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(strip_actions).collect()),
        JsonValue::Object(entries) => JsonValue::Object(
            entries
                .iter()
                .filter(|(key, _)| "a" != key.as_str())
                .map(|(key, item)| (key.clone(), strip_actions(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The open alts of one rule of a compiled spec, as JSON.
fn open_alts(src: &str, rule: &str) -> JsonValue {
    let spec = convert(src).unwrap_or_else(|error| panic!("{src:?}: {error}"));
    let alts: Vec<JsonValue> = spec
        .rule
        .get(rule)
        .and_then(Option::as_ref)
        .unwrap_or_else(|| panic!("{src:?}: no rule {rule}"))
        .open
        .iter()
        .map(tabnas_ebnf::AltSpec::to_value)
        .collect();
    strip_actions(&JsonValue::Array(alts))
}

/// The message of whatever refused `src`.
fn refusal(src: &str) -> String {
    match ebnf_convert(src, None) {
        Ok(_) => panic!("{src:?} was accepted, and should not be"),
        Err(error) => error.to_string(),
    }
}

// ---- converter -------------------------------------------------------

#[test]
fn emits_a_spec_for_an_alternation_of_terminals() {
    let spec = convert("Greet ::= \"hi\" | \"hello\"").expect("compiles");
    // A synthetic `__start__` wrapper ensures end of source is always
    // consumed; it pushes the user's start rule.
    assert_eq!(
        spec.options.get("rule").and_then(|rule| rule.get("start")),
        Some(&json!("__start__"))
    );
    assert_eq!(
        open_alts("Greet ::= \"hi\" | \"hello\"", "__start__"),
        json!([{ "p": "Greet", "g": "ebnf" }])
    );
    assert_eq!(
        open_alts("Greet ::= \"hi\" | \"hello\"", "Greet"),
        json!([{ "s": "#HI", "g": "ebnf" }, { "s": "#HELLO", "g": "ebnf" }])
    );
}

#[test]
fn w3c_literals_are_case_sensitive_so_they_emit_fixed_tokens() {
    // The contrast with `tabnas-abnf` is the point: RFC 5234 strings are
    // case-INsensitive and compile to case-folding regex matchers, while
    // W3C strings match exactly and compile to fixed tokens.
    let spec = convert("Greet ::= \"hi\" | \"hello\"").expect("compiles");
    assert_eq!(
        spec.options
            .get("fixed")
            .and_then(|fixed| fixed.get("token")),
        Some(&json!({ "#HI": "hi", "#HELLO": "hello" }))
    );
    assert_eq!(spec.options.get("match"), None);
}

#[test]
fn emits_a_single_n_token_alt_for_a_terminal_sequence() {
    assert_eq!(
        open_alts("Pair ::= \"a\" \"b\" \"c\"", "Pair"),
        json!([{ "s": "#A #B #C", "g": "ebnf" }])
    );
}

#[test]
fn honours_an_override_of_the_start_rule() {
    let options = EbnfConvertOptions {
        start: Some("B".to_string()),
        ..EbnfConvertOptions::default()
    };
    let spec = convert_with("A ::= \"x\"\nB ::= \"y\"", &options).expect("compiles");
    let start = spec.rule["__start__"].as_ref().expect("a start rule");
    let alts = strip_actions(&JsonValue::Array(
        start
            .open
            .iter()
            .map(tabnas_ebnf::AltSpec::to_value)
            .collect(),
    ));
    assert_eq!(alts, json!([{ "p": "B", "g": "ebnf" }]));
}

#[test]
fn tags_every_emitted_alt_with_the_ebnf_group_tag() {
    let spec = convert("A ::= \"x\" | \"y\"").expect("compiles");
    for alt in &spec.rule["A"].as_ref().expect("rule A").open {
        assert_eq!(alt.g(), Some("ebnf"), "every alt carries the tag");
    }
}

#[test]
fn character_classes_emit_a_regex_match_token() {
    let spec = convert("D ::= [0-9]").expect("compiles");
    let tokens = spec
        .options
        .get("match")
        .and_then(|matched| matched.get("token"))
        .and_then(JsonValue::as_object)
        .expect("a match token table");
    assert_eq!(tokens.len(), 1);
    let source = tokens.values().next().expect("one entry").to_string();
    assert!(
        source.contains("^[\\\\u0030-\\\\u0039]"),
        "the class reached the matcher: {source}"
    );
}

#[test]
fn to_spec_is_the_same_conversion_as_ebnf_convert() {
    let one = to_spec("A ::= \"x\"", None).expect("compiles");
    let two = ebnf_convert("A ::= \"x\"", None).expect("compiles");
    assert_eq!(rule_names(&one), rule_names(&two));
}

// ---- IR shape --------------------------------------------------------

#[test]
fn builds_terms_refs_and_sequences() {
    assert_eq!(
        prods("A ::= \"x\" B\nB ::= \"y\"")[0],
        json!({
            "name": "A",
            "alts": [[
                { "kind": "term", "literal": "x", "caseSensitive": true },
                { "kind": "ref", "name": "B" }
            ]]
        })
    );
}

#[test]
fn builds_one_alt_per_branch() {
    assert_eq!(
        prods("A ::= \"x\" | \"y\" | \"z\"")[0]["alts"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
}

#[test]
fn maps_postfix_operators_onto_opt_star_and_plus() {
    let grammar = prods("A ::= \"x\"? \"y\"* \"z\"+");
    let kinds: Vec<&str> = grammar[0]["alts"][0]
        .as_array()
        .expect("one alt")
        .iter()
        .map(|element| element["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(kinds, ["opt", "star", "plus"]);
}

#[test]
fn maps_parentheses_onto_a_group_of_alts() {
    let alt = prods("A ::= ( \"x\" | \"y\" ) \"z\"")[0]["alts"][0].clone();
    assert_eq!(alt[0]["kind"], "group");
    assert_eq!(alt[0]["alts"].as_array().map(Vec::len), Some(2));
    assert_eq!(alt[1]["kind"], "term");
}

#[test]
fn stacks_postfix_operators_outermost_last() {
    // Not W3C EBNF, which never stacks them, but reading a stack is
    // strictly more permissive than failing on one.
    let element = prods("A ::= ( \"x\" )*?")[0]["alts"][0][0].clone();
    assert_eq!(element["kind"], "opt");
    assert_eq!(element["inner"]["kind"], "star");
}

#[test]
fn maps_a_character_class_onto_a_regex_element() {
    assert_eq!(
        prods("A ::= [a-z]")[0]["alts"][0][0],
        json!({ "kind": "regex", "pattern": "[\\u0061-\\u007a]", "flags": "" })
    );
}

#[test]
fn maps_a_negated_character_class_onto_a_negated_regex() {
    // `u` even with no astral member written: a negated class matches
    // the COMPLEMENT of its members, and that always contains every
    // astral code point.
    assert_eq!(
        prods("A ::= [^<&]")[0]["alts"][0][0],
        json!({ "kind": "regex", "pattern": "[^\\u003c\\u0026]", "flags": "u" })
    );
    // ...and the matcher the class compiles to consumes the WHOLE
    // character, not one surrogate half. Rust's `regex` is Unicode-aware
    // throughout, so this is what the `u` flag buys in JavaScript.
    let pattern = prods("A ::= [^<&]")[0]["alts"][0][0]["pattern"]
        .as_str()
        .expect("a pattern")
        .to_string();
    let anchored = format!("^{pattern}");
    let regex = regex_lite(&anchored);
    assert_eq!(regex("\u{1F600}"), Some("\u{1F600}".to_string()));
}

/// Compile a pattern and answer what it matches at the start of a
/// string. A closure rather than a helper module: one assertion needs
/// it.
fn regex_lite(pattern: &str) -> impl Fn(&str) -> Option<String> + '_ {
    let regex = regex::Regex::new(pattern).expect("the emitted pattern compiles");
    move |input: &str| regex.find(input).map(|found| found.as_str().to_string())
}

#[test]
fn reads_hex_code_points_inside_a_class_as_ranges_and_as_members() {
    let grammar = prods("A ::= [#x20-#x7E]\nB ::= [#x9#xA#xD]");
    assert_eq!(grammar[0]["alts"][0][0]["pattern"], "[\\u0020-\\u007e]");
    assert_eq!(
        grammar[1]["alts"][0][0]["pattern"],
        "[\\u0009\\u000a\\u000d]"
    );
}

#[test]
fn treats_a_trailing_or_leading_hyphen_in_a_class_as_a_member() {
    let grammar = prods("A ::= [-+]\nB ::= [a-]");
    assert_eq!(grammar[0]["alts"][0][0]["pattern"], "[\\u002d\\u002b]");
    assert_eq!(grammar[1]["alts"][0][0]["pattern"], "[\\u0061\\u002d]");
}

#[test]
fn switches_a_class_above_the_bmp_to_braced_escapes_and_the_u_flag() {
    // `\uXXXX` only reaches U+FFFF, so an astral endpoint has to be
    // spelled `\u{…}`, which in JavaScript requires the `u` flag.
    assert_eq!(
        prods("A ::= [#x10000-#x10FFFF]")[0]["alts"][0][0],
        json!({ "kind": "regex", "pattern": "[\\u{10000}-\\u{10ffff}]", "flags": "u" })
    );
}

#[test]
fn maps_a_standalone_code_point_onto_a_term() {
    assert_eq!(
        prods("A ::= #x41")[0]["alts"][0][0],
        json!({ "kind": "term", "literal": "A", "caseSensitive": true })
    );
}

#[test]
fn accepts_both_quote_styles_for_a_literal() {
    let grammar = prods("A ::= 'a\"b' | \"c'd\"");
    assert_eq!(grammar[0]["alts"][0][0]["literal"], "a\"b");
    assert_eq!(grammar[0]["alts"][1][0]["literal"], "c'd");
}

#[test]
fn has_no_escape_sequences_inside_a_literal() {
    // W3C EBNF defines none, so a backslash is the backslash character
    // and `\n` is two characters.
    let literal = prods("A ::= \"\\n\"")[0]["alts"][0][0]["literal"]
        .as_str()
        .expect("a literal")
        .to_string();
    assert_eq!(literal, "\\n");
    assert_eq!(literal.chars().count(), 2);
}

// ---- W3C constructs, end to end --------------------------------------

/// Parse `input` with the grammar `src` and answer the matched source.
fn matched(src: &str, input: &str) -> String {
    let value =
        parse_with(src, input).unwrap_or_else(|error| panic!("{src:?} / {input:?}: {error}"));
    tree(&value)["src"].as_str().unwrap_or_default().to_string()
}

/// Assert that `input` is refused, and refused as UNEXPECTED input
/// rather than by a grammar that failed to compile.
fn rejects(src: &str, input: &str) {
    let parser = engine_for(src).unwrap_or_else(|error| panic!("{src:?}: {error}"));
    let error = parser
        .parse(input)
        .err()
        .unwrap_or_else(|| panic!("{src:?} / {input:?}: want reject, got accept"));
    assert!(
        error.to_string().contains("unexpected"),
        "{src:?} / {input:?}: want an `unexpected` rejection, got {error}"
    );
}

#[test]
fn alternation_takes_either_branch() {
    let src = "Greet ::= \"hi\" | \"hello\"";
    assert_eq!(matched(src, "hi"), "hi");
    assert_eq!(matched(src, "hello"), "hello");
    rejects(src, "nope");
}

#[test]
fn concatenation_takes_both_in_order() {
    let src = "Pair ::= \"a\" \"b\"";
    assert_eq!(matched(src, "ab"), "ab");
    rejects(src, "a");
    rejects(src, "ba");
}

#[test]
fn an_optional_element_may_be_present_or_absent() {
    let src = "G ::= \"hi\" \"there\"?";
    assert_eq!(matched(src, "hi"), "hi");
    assert_eq!(matched(src, "hi there"), "hithere");
    rejects(src, "hi nope");
}

#[test]
fn star_is_zero_or_more() {
    let src = "G ::= \"x\"* \"end\"";
    assert_eq!(matched(src, "end"), "end");
    assert_eq!(matched(src, "x end"), "xend");
    assert_eq!(matched(src, "x x x end"), "xxxend");
    rejects(src, "y end");
}

#[test]
fn plus_is_one_or_more() {
    let src = "G ::= \"x\"+ \"end\"";
    assert_eq!(matched(src, "x end"), "xend");
    assert_eq!(matched(src, "x x x end"), "xxxend");
    rejects(src, "end");
}

#[test]
fn a_group_binds_before_what_follows_it() {
    let src = "G ::= ( \"a\" | \"b\" ) \"c\"";
    assert_eq!(matched(src, "ac"), "ac");
    assert_eq!(matched(src, "bc"), "bc");
    rejects(src, "cc");
}

#[test]
fn grouping_composes_with_repetition() {
    let src = "G ::= ( \"a\" \"b\" )+ \"end\"";
    assert_eq!(matched(src, "a b end"), "abend");
    assert_eq!(matched(src, "a b a b end"), "ababend");
    rejects(src, "end");
}

#[test]
fn a_character_class_matches_one_character_from_the_set() {
    let src = "Digits ::= [0-9]+";
    assert_eq!(matched(src, "1"), "1");
    assert_eq!(matched(src, "1234"), "1234");
    rejects(src, "abc");
}

#[test]
fn a_class_reads_enumerated_members_and_hex_ranges() {
    assert_eq!(matched("Hex ::= [0-9abcdef]+", "1f"), "1f");
    rejects("Hex ::= [0-9abcdef]+", "g");
    assert_eq!(matched("P ::= [#x30-#x39]+", "42"), "42");
    rejects("P ::= [#x30-#x39]+", "x");
}

#[test]
fn a_negated_class_matches_anything_but_the_members() {
    let src = "S ::= \"<\" [^<>]+ \">\"";
    assert_eq!(matched(src, "<abc>"), "<abc>");
    rejects(src, "<a<b>");
}

#[test]
fn a_standalone_code_point_matches_that_code_point() {
    let src = "S ::= #x41 #x42";
    assert_eq!(matched(src, "AB"), "AB");
    rejects(src, "ab");
}

#[test]
fn literals_are_case_sensitive() {
    let src = "S ::= \"GET\"";
    assert_eq!(matched(src, "GET"), "GET");
    rejects(src, "get");
    rejects(src, "Get");
}

#[test]
fn built_in_lexer_tokens_are_referable_by_bare_name() {
    // Inherited from the shared compiler: TX / NR / ST / VL name the
    // engine's own lexer tokens. Not W3C EBNF, but the only sane way to
    // write a token-level grammar for this engine.
    let src = "Pair ::= \"{\" Key \":\" Val \"}\"\nKey ::= TX\nVal ::= NR | ST";
    let value = parse_with(src, "{a:1}").expect("parses");
    let kids: Vec<String> = tree(&value)["kids"]
        .as_array()
        .expect("kids")
        .iter()
        .map(|kid| kid["rule"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(kids, ["Key", "Val"]);
    let value = parse_with(src, "{a:\"x\"}").expect("parses");
    assert_eq!(tree(&value)["kids"][1]["src"], "\"x\"");
}

#[test]
fn comments_are_ignored() {
    let src = "/* leading */ G ::= \"a\" /* inline */ \"b\" /* trailing */";
    assert_eq!(matched(src, "ab"), "ab");
}

// ---- ISO 14977 spellings that are accepted ---------------------------

#[test]
fn the_iso_equals_works_as_the_definition_operator() {
    assert_eq!(matched("greet = \"hi\" | \"hello\"", "hi"), "hi");
}

#[test]
fn a_semicolon_terminates_a_production() {
    let names: Vec<String> = parse_ebnf("a = \"x\" ;\nb = \"y\" ;")
        .expect("parses")
        .productions
        .iter()
        .map(|production| production.name.clone())
        .collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn a_semicolon_on_the_last_production_only_is_still_fine() {
    let names: Vec<String> = parse_ebnf("a ::= \"x\"\nb ::= \"y\" ;")
        .expect("parses")
        .productions
        .iter()
        .map(|production| production.name.clone())
        .collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn a_comma_is_accepted_as_an_explicit_concatenation_separator() {
    // Juxtaposition already means sequence here, so the comma adds
    // nothing to the IR: the two spellings must agree exactly.
    assert_eq!(prods("a ::= \"x\" , \"y\""), prods("a ::= \"x\" \"y\""));
}

#[test]
fn iso_comments_are_ignored() {
    assert_eq!(
        matched("(* leading *) g = \"a\" (* inline *) \"b\"", "ab"),
        "ab"
    );
}

#[test]
fn a_multi_line_iso_comment_does_not_shift_error_line_numbers() {
    // The ISO comment is claimed by a match-token matcher, which runs
    // before the fixed matcher sees `(`. Without correct row
    // bookkeeping over the consumed text, every line number after a
    // multi-line comment would be wrong.
    let error = parse_ebnf("(* one\ntwo\nthree *)\na ::= \"x\"\nb ::= { \"y\" }")
        .expect_err("the ISO repetition is refused");
    assert_eq!(error.line, Some(5));
}

#[test]
fn the_iso_spelled_fixture_parses_the_same_language_as_the_w3c_one() {
    let w3c = engine_for(&fixture("expr.ebnf")).expect("compiles");
    let iso = engine_for(&fixture("iso-style.ebnf")).expect("compiles");
    for input in ["1", "1+2", "1+2*3", "(1+2)*3"] {
        assert_eq!(
            tree(&w3c.parse(input).expect("parses"))["src"],
            tree(&iso.parse(input).expect("parses"))["src"],
            "{input}"
        );
    }
}

// ---- documented rejections -------------------------------------------
//
// Each of these is an entry in the README's "not supported" list. The
// test asserts both that the construct is refused and that the message
// says which construct it was.

#[test]
fn rejects_subtraction_naming_the_operator() {
    let message = refusal("A ::= B - C\nB ::= \"b\"\nC ::= \"c\"");
    assert!(message.contains("subtraction ('-') at line 1"), "{message}");
    assert!(message.contains("no difference operator"), "{message}");
}

#[test]
fn rejects_subtraction_written_without_a_trailing_space() {
    assert!(
        refusal("A ::= B -C\nB ::= \"b\"").contains("subtraction"),
        "a hyphen that starts a token subtracts"
    );
}

#[test]
fn rejects_iso_special_sequences_naming_them() {
    assert!(
        refusal("A ::= ? anything at all ?").contains("special sequences"),
        "the message names the construct"
    );
}

#[test]
fn rejects_iso_bracket_repetition_naming_the_alternative() {
    let message = refusal("A ::= { \"x\" }");
    assert!(
        message.contains("ISO 14977 bracket repetition"),
        "{message}"
    );
    assert!(message.contains("write 'A*'"), "{message}");
}

#[test]
fn rejects_a_stray_bracket_explaining_the_character_class_reading() {
    let message = refusal("A ::= [a-z");
    assert!(message.contains("stray '['"), "{message}");
    assert!(message.contains("character class"), "{message}");
}

#[test]
fn rejects_abnf_style_prefix_repetition_naming_the_direction() {
    for src in ["A ::= *\"x\"", "A ::= +\"x\""] {
        assert!(
            refusal(src).contains("Repetition in this dialect is postfix"),
            "{src}"
        );
    }
}

#[test]
fn rejects_an_unclosed_group() {
    assert!(refusal("A ::= ( \"x\"").contains("unclosed group"));
}

#[test]
fn rejects_an_empty_literal() {
    assert!(refusal("A ::= \"\"").contains("empty string literal"));
}

#[test]
fn rejects_an_empty_or_reversed_character_class() {
    assert!(refusal("A ::= []").contains("empty character class"));
    assert!(refusal("A ::= [z-a]").contains("reversed range"));
}

#[test]
fn rejects_a_code_point_above_the_unicode_maximum() {
    assert!(refusal("A ::= #x110000").contains("not a Unicode code point"));
    assert!(refusal("A ::= [#x0-#x110000]").contains("not a Unicode code point"));
}

#[test]
fn rejects_a_malformed_symbol_name_naming_it() {
    assert!(
        refusal("Foo! ::= \"x\"").contains("'Foo!' at line 1, column 1 is not a valid symbol name")
    );
}

#[test]
fn rejects_a_rule_defined_twice_naming_the_rule() {
    assert!(refusal("A ::= \"x\"\nA ::= \"y\"").contains("rule 'A' is defined more than once"));
}

#[test]
fn rejects_two_alternatives_that_both_match_nothing_naming_the_rule() {
    // Genuinely ambiguous: the empty input has two derivations, and no
    // amount of lookahead distinguishes them.
    assert!(refusal("A ::= \"x\"? | \"y\"?")
        .contains("rule 'A' has 2 alternatives that each match nothing"));
    // One optional alternative is fine.
    assert!(convert("A ::= \"x\"? | \"y\"").is_ok());
}

#[test]
fn rejects_two_empty_deriving_alternatives_inside_a_group() {
    // A group is a choice like any other, so it carries the same
    // ambiguity. The check used to look at production-level alts only,
    // so every grouped spelling slipped through and mis-parsed.
    assert!(refusal("A ::= (\"x\"? | \"y\"?) \"y\"")
        .contains("a group in rule 'A' has 2 alternatives that each match nothing"));
    // Nested groups are reached too.
    assert!(ebnf_convert("A ::= ((\"x\"? | \"y\"?) \"z\") \"w\"", None).is_err());
    // One nullable branch in a group is fine.
    assert!(convert("A ::= (\"x\"? | \"y\") \"z\"").is_ok());
}

#[test]
fn rejects_an_empty_alternative_rather_than_reading_it_as_epsilon() {
    // Both dialects require an expression each side of `|`, and this
    // package refuses an empty literal, so accepting epsilon here made
    // the accepted language wider than the documented one.
    for src in ["A ::= | \"x\"", "A ::= \"x\" |", "A = ;", "A ::= ()"] {
        assert!(
            refusal(src).contains("empty alternative"),
            "expected {src:?} to be rejected as an empty alternative"
        );
    }
    assert!(convert("A ::= \"x\" | \"y\"").is_ok());
}

#[test]
fn rejects_a_leading_comma_which_iso_does_not_define() {
    // ISO's comma separates concatenated items; it is not a prefix.
    assert!(refusal("A = , \"x\";").contains("leading comma"));
    // Between two items it is the documented ISO spelling.
    assert!(convert("A = \"x\" , \"y\";").is_ok());
}

#[test]
fn accepts_only_the_lowercase_code_point_spelling() {
    // W3C specifies `#x`; `#X41` is not among the ISO spellings this
    // package documents accepting, so it must not be case-folded.
    assert!(convert("A ::= #x41").is_ok());
    assert!(ebnf_convert("A ::= #X41", None).is_err());
    assert!(convert("A ::= [#x20-#x7E]").is_ok());
    assert!(ebnf_convert("A ::= [#X20-#X7E]", None).is_err());
    // Hex digits themselves may be either case.
    assert!(convert("A ::= #xD7FF").is_ok());
    assert!(convert("A ::= #xd7ff").is_ok());
}

#[test]
fn rejects_a_reference_to_an_undefined_rule_naming_both_rules() {
    let error = ebnf_convert("A ::= B", None).expect_err("refused");
    assert!(matches!(error, EbnfError::Compile(_)), "{error}");
    assert!(
        error
            .to_string()
            .contains("ebnf: rule 'A' references unknown rule 'B'"),
        "{error}"
    );
}

#[test]
fn rejects_a_purely_left_recursive_rule_naming_the_rule() {
    let error = ebnf_convert("A ::= A \"x\"", None).expect_err("refused");
    assert!(matches!(error, EbnfError::Compile(_)), "{error}");
    assert!(
        error
            .to_string()
            .contains("ebnf: rule 'A' is purely left-recursive"),
        "{error}"
    );
}

#[test]
fn rejects_a_source_with_no_productions() {
    assert!(refusal("/* nothing but a comment */").contains("no productions found"));
    assert!(refusal("").contains("no productions found"));
}

#[test]
fn reports_line_and_column_on_a_parse_error() {
    let error = parse_ebnf("A ::= \"x\"\nB ::= { \"y\" }").expect_err("refused");
    assert_eq!(error.line, Some(2));
    assert!(error.column.is_some());
}

// ---- left recursion --------------------------------------------------

#[test]
fn rewrites_a_left_recursive_rule_into_a_seed_and_a_tail() {
    let grammar = parse_ebnf("E ::= E \"+\" T | T\nT ::= \"1\"").expect("parses");
    let rewritten = tabnas_ebnf::eliminate_left_recursion(&grammar).expect("rewrites");
    let e = rewritten
        .productions
        .iter()
        .find(|production| "E" == production.name)
        .expect("E survives");
    assert_eq!(e.alts.len(), 1);
    let json = serde_json::to_value(&e.alts[0]).expect("serializes");
    assert_eq!(json[0]["kind"], "term");
    assert_eq!(json[1]["kind"], "star");
}

#[test]
fn a_left_recursive_grammar_parses_a_whole_chain() {
    let src = "Expr ::= Expr \"+\" Term | Term\nTerm ::= NR";
    assert_eq!(tree(&parse_with(src, "1").expect("parses"))["rule"], "Expr");
    assert_eq!(matched(src, "1+2+3"), "1+2+3");
    let value = parse_with(src, "1+2+3").expect("parses");
    let kids: Vec<String> = tree(&value)["kids"]
        .as_array()
        .expect("kids")
        .iter()
        .map(|kid| kid["rule"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(kids, ["Term", "Term"]);
}

// ---- realistic grammars ----------------------------------------------

#[test]
fn expr_fixture_parses_arithmetic_with_precedence() {
    let src = fixture("expr.ebnf");
    assert_eq!(matched(&src, "1"), "1");
    assert_eq!(matched(&src, "1+2"), "1+2");
    assert_eq!(matched(&src, "1 + 2 - 3 * 4 / 5"), "1+2-3*4/5");
    assert_eq!(matched(&src, "(1+2)*3"), "(1+2)*3");
    rejects(&src, "1 +");
    rejects(&src, "* 1");
}

#[test]
fn expr_fixture_nests_term_under_expr_and_factor_under_term() {
    let parser = engine_for(&fixture("expr.ebnf")).expect("compiles");
    let out = tree(&parser.parse("1+2*3").expect("parses"));
    assert_eq!(out["rule"], "Expr");
    // The leading operand folds into the repeat's parent, so the one
    // surviving child is the `2*3` Term.
    let kids = out["kids"].as_array().expect("kids");
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["rule"], "Term");
    assert_eq!(kids[0]["src"], "2*3");
    assert_eq!(kids[0]["kids"][0]["rule"], "Factor");
}

#[test]
fn json_subset_fixture_parses_nested_json() {
    let src = fixture("json-subset.ebnf");
    let parser = engine_for(&src).expect("compiles");
    for input in [
        "1",
        "\"a\"",
        "true",
        "false",
        "null",
        "{}",
        "[]",
        "{\"a\":1}",
        "[1,2,3]",
        "{\"a\":[1,{\"b\":null}]}",
    ] {
        let value = tree(&parser.parse(input).expect("parses"));
        // The canonical suite strips `/\s/g`, which is ASCII here.
        // Spelled out rather than written `char::is_whitespace`, which
        // is Unicode-aware where the JavaScript class it stands in for
        // is not.
        let bare: String = input
            .chars()
            .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
            .collect();
        assert_eq!(value["src"].as_str().unwrap_or_default(), bare, "{input}");
    }
    rejects(&src, "{\"a\" 1}");
    rejects(&src, "[1,]");
}

#[test]
fn json_subset_fixture_builds_a_tree_over_the_rules_that_survive() {
    // Not every production becomes a node: the shared compiler folds a
    // rule whose body is a single token segment into its caller, so
    // `Object` and `Array` (each reached through a one-reference
    // alternative of `Value`) do not appear.
    let parser = engine_for(&fixture("json-subset.ebnf")).expect("compiles");
    let out = tree(&parser.parse("{\"a\":[1,2]}").expect("parses"));
    assert_eq!(out["rule"], "Json");

    let value = &out["kids"][0];
    assert_eq!(value["rule"], "Value");
    assert_eq!(value["src"], "{\"a\":[1,2]}");

    let member = &value["kids"][0];
    assert_eq!(member["rule"], "Member");
    assert_eq!(member["src"], "\"a\":[1,2]");

    let array = &member["kids"][0];
    assert_eq!(array["rule"], "Value");
    let sources: Vec<&str> = array["kids"]
        .as_array()
        .expect("kids")
        .iter()
        .map(|kid| kid["src"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(sources, ["1", "2"]);
}

#[test]
fn name_fixture_parses_xml_style_names_character_by_character() {
    let src = fixture("name.ebnf");
    assert_eq!(matched(&src, "abc"), "abc");
    assert_eq!(matched(&src, "_x-1.2"), "_x-1.2");
    rejects(&src, "1abc");
}

// ---- the install path ------------------------------------------------

#[test]
fn installs_the_grammar_on_the_instance() {
    let mut parser = tabnas::Tabnas::new();
    let spec = ebnf(&mut parser, "A ::= \"x\"", None).expect("installs");
    assert_eq!(
        spec.options.get("rule").and_then(|rule| rule.get("start")),
        Some(&json!("__start__"))
    );
    assert_eq!(tree(&parser.parse("x").expect("parses"))["rule"], "A");
}

#[test]
fn to_spec_builds_without_installing() {
    let mut parser = tabnas::Tabnas::new();
    let before = parser.rule_names().len();
    let spec = to_spec("A ::= \"x\"", None).expect("compiles");
    assert!(spec.rule.contains_key("A"));
    // The spec was built but not installed, so the instance is
    // untouched.
    assert_eq!(parser.rule_names().len(), before);
    // Whereas installing it does add the rule.
    ebnf(&mut parser, "A ::= \"x\"", None).expect("installs");
    assert!(parser.rule_names().iter().any(|name| "A" == name));
}

#[test]
fn each_instance_gets_its_own_grammar() {
    let a = engine_for("A ::= \"x\"").expect("compiles");
    let b = engine_for("B ::= \"y\"").expect("compiles");
    assert_eq!(tree(&a.parse("x").expect("parses"))["rule"], "A");
    assert_eq!(tree(&b.parse("y").expect("parses"))["rule"], "B");
    assert!(a.parse("y").is_err());
}

#[test]
fn the_plugin_installs_the_source_it_is_handed() {
    let mut parser = tabnas::Tabnas::new();
    let options = tabnas::Value::object(indexmap::IndexMap::from([(
        "src".to_string(),
        tabnas::Value::String("A ::= \"x\"".to_string()),
    )]));
    parser
        .use_plugin(tabnas_ebnf::plugin(), Some(options))
        .expect("installs");
    assert_eq!(tree(&parser.parse("x").expect("parses"))["rule"], "A");
}

#[test]
fn the_plugin_with_no_source_installs_nothing() {
    // The grammar this crate installs is whatever the caller hands over,
    // so a bare `use_plugin` has nothing to do and must not fail.
    let mut parser = tabnas::Tabnas::new();
    parser
        .use_plugin(tabnas_ebnf::plugin(), None)
        .expect("installs");
}

#[test]
fn the_rule_table_is_handed_out_as_data() {
    let rules = tabnas_ebnf::ebnf_rules();
    let table = rules.as_object().expect("a rule map");
    for name in ["ebnf", "prod", "alts", "seq", "item", "post", "atom"] {
        assert!(table.contains_key(name), "the table declares {name}");
    }
    // A FRESH document each call, so a caller may take it apart without
    // disturbing the parser this crate builds from the same text.
    let mut mine = rules.clone();
    mine.as_object_mut().expect("a map").clear();
    assert!(!tabnas_ebnf::ebnf_rules()
        .as_object()
        .expect("a map")
        .is_empty());
}

// ---- the empty input is decided from the grammar ---------------------
//
// Whether the empty string is in the language is settled at compile
// time, not by the rules: the engine short-circuits `''` before the
// parse loop starts, so no rule ever sees it.

#[test]
fn the_empty_input_is_refused_where_the_grammar_consumes() {
    for src in [
        "S ::= \"a\"",
        "S ::= \"a\"+",
        "S ::= \"a\" \"b\"",
        "S ::= ( \"a\" | \"b\" )",
        "S ::= [a-z]",
    ] {
        assert!(!accepts_empty(src), "{src:?} must refuse the empty input");
    }
}

#[test]
fn the_empty_input_is_accepted_where_the_grammar_is_nullable() {
    for src in [
        "S ::= \"a\"*",
        "S ::= \"a\"?",
        "S ::= \"a\"* \"b\"*",
        "S ::= ( \"a\" | \"b\" )?",
    ] {
        assert!(accepts_empty(src), "{src:?} must accept the empty input");
    }
}

#[test]
fn nullability_follows_through_other_rules() {
    // A least fixed point over the rules, not a property of one
    // production read alone.
    assert!(accepts_empty("S ::= A\nA ::= B\nB ::= \"a\"*"));
    assert!(!accepts_empty("S ::= A\nA ::= B\nB ::= \"a\""));
    assert!(accepts_empty("S ::= A B\nA ::= \"x\"?\nB ::= \"y\"?"));
}

#[test]
fn nullability_is_asked_of_the_start_rule_whichever_that_is() {
    let src = "S ::= \"a\"\nT ::= \"b\"*";
    assert!(!accepts_empty(src));
    let options = EbnfConvertOptions {
        start: Some("T".to_string()),
        ..EbnfConvertOptions::default()
    };
    let spec = convert_with(src, &options).expect("compiles");
    assert!(install(&spec).expect("installs").parse("").is_ok());
}

#[test]
fn a_recursive_rule_that_always_consumes_is_not_nullable() {
    assert!(!accepts_empty(
        "S ::= A \"x\" | B \"y\"\nA ::= \"a\" A | \"a\"\nB ::= \"a\" B | \"a\""
    ));
}

#[test]
fn both_documented_pipelines_decide_the_empty_input_the_same_way() {
    // The two-step form is in the guide and the reference, and it
    // reaches the shared emitter directly, so a decision applied only
    // inside `ebnf_convert` would leave the two public paths
    // disagreeing about which strings parse.
    for (src, empty) in [("S ::= \"a\"", false), ("S ::= \"a\"*", true)] {
        let one = convert(src).expect("compiles");
        let two = emit_grammar_spec(&parse_ebnf(src).expect("parses"), None).expect("emits");
        for (spec, label) in [(one, "one-step"), (two, "two-step")] {
            assert_eq!(
                spec.options
                    .get("lex")
                    .and_then(|lex| lex.get("empty"))
                    .and_then(JsonValue::as_bool),
                Some(empty),
                "{src:?} ({label})"
            );
        }
    }
}
