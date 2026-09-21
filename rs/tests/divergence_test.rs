// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// Where this port answers something the canonical TypeScript does not,
// for the same input. `DIVERGENCE.md` at the repository root carries the
// measured table, the reason and who owns the repair; this file is the
// executable half.
//
// There is no `test/spec` register here to carry a `rust` column: this
// repository ships no shared fixtures at all (`AGENTS.md`, "Error
// codes"), so each divergence is pinned by a test instead, and asserted
// in BOTH directions -- the behaviour recorded, and the canonical
// behaviour it differs from -- so a port that starts agreeing fails as
// loudly as one that starts disagreeing.
//
// The file also carries the Rust half of `go/divergence_test.go`, the
// cases where the Go port and TypeScript were found to disagree and were
// ALIGNED. Those are agreements, not divergences: they belong here
// because the file the Go port keeps them in is the one a reader looks
// in, and each names its Go counterpart by exact test name so a grep
// from either side lands on a real test.
//
// Every number below was MEASURED on 2026-09-21 by running all three
// implementations over the input in the comment; nothing is inferred
// from reading the source.

mod common;

use common::{convert, engine_for};
use serde_json::Value as JsonValue;
use tabnas_ebnf::parse_ebnf;

/// The IR of one source, as JSON.
fn ir(src: &str) -> JsonValue {
    serde_json::to_value(parse_ebnf(src).unwrap_or_else(|error| panic!("{src:?}: {error}")))
        .expect("the IR serializes")
}

// ---- 1. a code point naming a lone surrogate ------------------------

/// `#xD800` names one half of a UTF-16 surrogate pair. A JavaScript
/// string can hold one; a Rust `String` cannot.
///
/// Measured: TypeScript `literal` is U+D800, Go and Rust U+FFFD.
#[test]
fn a_surrogate_code_point_becomes_the_replacement_character() {
    let element = ir("A ::= #xD800")["productions"][0]["alts"][0][0].clone();
    assert_eq!(
        element["literal"], "\u{FFFD}",
        "a lone surrogate has no Rust representation, so it becomes U+FFFD"
    );
    // The divergence is only about the character. Everything else about
    // the element, the span included, is what TypeScript answers.
    assert_eq!(element["kind"], "term");
    assert_eq!(element["caseSensitive"], true);
    assert_eq!(element["sp"]["s"], 6);
    assert_eq!(element["sp"]["e"], 12);
    assert_eq!(element["sp"]["c"], 7);
}

/// The same boundary inside a CHARACTER CLASS, which reaches different
/// machinery: a class lowers to a regular expression, and the `regex`
/// crate refuses an escape naming a surrogate outright where JavaScript
/// and Go both compile one.
///
/// Measured: TypeScript emits `[\ud800]` and installs it; Go emits
/// `[\x{d800}]` and installs it; this port trims the surrogate block off
/// each end, which leaves the set of characters the class matches
/// exactly as written, because no input a Rust parser is handed can hold
/// a surrogate.
#[test]
fn a_surrogate_inside_a_class_is_trimmed_rather_than_emitted() {
    let pattern = |src: &str| {
        ir(src)["productions"][0]["alts"][0][0]["pattern"]
            .as_str()
            .expect("a class lowers to a regex element")
            .to_string()
    };
    // A range with a surrogate at one end keeps every character it named.
    assert_eq!(pattern("A ::= [#x0-#xD800]"), "[\\u0000-\\ud7ff]");
    assert_eq!(
        pattern("A ::= [#xD800-#x10FFFF]"),
        "[\\u{e000}-\\u{10ffff}]"
    );
    // A surrogate beside a real member simply goes.
    assert_eq!(pattern("A ::= [#x41#xD800]"), "[\\u0041]");
    // A class naming nothing else falls back to U+FFFD, which is what
    // the standalone `#xD800` path answers.
    assert_eq!(pattern("A ::= [#xD800]"), "[\\ufffd]");
    assert_eq!(pattern("A ::= [#xD800-#xDFFF]"), "[\\ufffd]");
    // ...and NEGATED, the complement of nothing is everything.
    assert_eq!(pattern("A ::= [^#xD800]"), "[\\u{0}-\\u{10ffff}]");
}

/// The half of that boundary which is NOT a divergence, and the reason
/// the entry exists: every one of these grammars now INSTALLS, as it
/// does in both other runtimes. Emitting the surrogate escape converted
/// cleanly and then failed here, with the regex crate's own wording
/// about an internal token name.
#[test]
fn a_class_naming_a_surrogate_still_installs() {
    for src in [
        "A ::= [#xD800]",
        "A ::= [#xD800-#xDFFF]",
        "A ::= [#x41#xD800]",
        "A ::= [^#xD800]",
        "A ::= [#x0-#xD800]",
        "A ::= [#xD800-#x10FFFF]",
    ] {
        engine_for(src).unwrap_or_else(|error| panic!("{src:?} must install: {error}"));
    }
}

/// The CONTROL: a class that spans the surrogate block has scalar ends,
/// so it is left exactly as written. Without this, "trim surrogates"
/// could widen into "rewrite any class that touches the block".
#[test]
fn a_class_spanning_the_surrogate_block_is_left_alone() {
    let element = ir("A ::= [#xD7FF-#xE000]")["productions"][0]["alts"][0][0].clone();
    assert_eq!(
        element["pattern"], "[\\ud7ff-\\ue000]",
        "both ends are scalar values, so nothing is trimmed"
    );
    // ...and the ordinary classes are untouched, spelling and flags both.
    for (src, pattern, flags) in [
        ("A ::= [a-z]", "[\\u0061-\\u007a]", ""),
        ("A ::= [^<&]", "[^\\u003c\\u0026]", "u"),
        ("A ::= [#x20-#x7E]", "[\\u0020-\\u007e]", ""),
        ("A ::= [#x10000-#x10FFFF]", "[\\u{10000}-\\u{10ffff}]", "u"),
    ] {
        let element = ir(src)["productions"][0]["alts"][0][0].clone();
        assert_eq!(element["pattern"], pattern, "{src}");
        assert_eq!(element["flags"], flags, "{src}");
    }
}

/// The CONTROL for the test above: a code point Unicode does have is
/// carried through exactly, so "surrogates become U+FFFD" cannot widen
/// into "anything unusual does".
#[test]
fn every_other_code_point_survives_intact() {
    for (src, want) in [
        ("A ::= #x41", "A"),
        ("A ::= #xE9", "\u{E9}"),
        ("A ::= #xD7FF", "\u{D7FF}"),
        ("A ::= #xE000", "\u{E000}"),
        ("A ::= #x10FFFF", "\u{10FFFF}"),
    ] {
        assert_eq!(
            ir(src)["productions"][0]["alts"][0][0]["literal"],
            want,
            "{src}"
        );
    }
}

// ---- 2. span offsets count bytes ------------------------------------

/// A span records where an element came from in the units the
/// front-end's own engine tokens use: a UTF-16 code unit in TypeScript,
/// a byte in Go and here.
///
/// Measured over `A ::= "é" B\nB ::= "y"`, the span of the `B`
/// reference:
///
/// | | TypeScript | Go | Rust |
/// |---|---|---|---|
/// | `s`, `e` | 10, 11 | 11, 12 | 11, 12 |
/// | `c` | 11 | 12 | 11 |
#[test]
fn span_offsets_count_bytes_not_utf16_code_units() {
    let reference = ir("A ::= \"é\" B\nB ::= \"y\"")["productions"][0]["alts"][0][1].clone();
    assert_eq!(reference["name"], "B");
    assert_eq!(reference["sp"]["s"], 11, "TypeScript answers 10 here");
    assert_eq!(reference["sp"]["e"], 12, "TypeScript answers 11 here");
}

/// What the divergence does NOT cost: slicing the original source with a
/// span gives the same TEXT in every runtime, which is what a consumer
/// wants a span for.
#[test]
fn a_span_still_slices_the_right_text_through_a_non_ascii_source() {
    let src = "A ::= \"é\" B\nB ::= \"y\"";
    let reference = ir(src)["productions"][0]["alts"][0][1].clone();
    let start = reference["sp"]["s"].as_u64().expect("an offset") as usize;
    let end = reference["sp"]["e"].as_u64().expect("an offset") as usize;
    assert_eq!(&src[start..end], "B");
}

// ---- 3. a column counts Unicode scalars -----------------------------

/// A column is the engine's, and this engine counts Unicode scalar
/// values where TypeScript counts UTF-16 code units. The two agree
/// everywhere in the Basic Multilingual Plane and part company past it;
/// Go counts bytes and so parts company at any non-ASCII character.
///
/// Measured over `A ::= "😀" B\nB ::= "y"`, the span of the `B`
/// reference:
///
/// | | TypeScript | Go | Rust |
/// |---|---|---|---|
/// | `c` | 12 | 14 | 11 |
///
/// and over the same source with `é` in place of the emoji: 11, 12, 11.
#[test]
fn a_column_counts_scalars_so_it_agrees_with_typescript_below_the_bmp() {
    let reference = ir("A ::= \"é\" B\nB ::= \"y\"")["productions"][0]["alts"][0][1].clone();
    assert_eq!(
        reference["sp"]["c"], 11,
        "a BMP character is one unit in both runtimes"
    );
}

#[test]
fn a_column_parts_company_with_typescript_past_the_bmp() {
    let reference = ir("A ::= \"😀\" B\nB ::= \"y\"")["productions"][0]["alts"][0][1].clone();
    assert_eq!(
        reference["sp"]["c"], 11,
        "TypeScript answers 12: an astral character is two UTF-16 units there and one scalar here"
    );
}

/// The rest of the UTF-16-versus-scalar boundary, checked rather than
/// assumed, because the rest of it turns out NOT to diverge.
///
/// A JavaScript string is UTF-16 code units and a Rust `String` is
/// scalar values, which in other ports of this fleet has meant a paired
/// escape read as two replacements, a name test that reads a surrogate
/// half, and a truncation that splits a pair. None of those apply here:
/// W3C EBNF defines no escape sequences at all, a symbol name is tested
/// character by character against an ASCII set in both runtimes, and
/// nothing truncates. The measured differences are the three above.
#[test]
fn the_rest_of_the_utf16_boundary_does_not_diverge() {
    // An astral character written literally in a literal survives as
    // ONE character, not as two replacements.
    let literal = ir("A ::= \"\u{1F600}\"")["productions"][0]["alts"][0][0]["literal"]
        .as_str()
        .expect("a literal")
        .to_string();
    assert_eq!(literal, "\u{1F600}");
    assert_eq!(literal.chars().count(), 1);

    // An astral character written literally in a CLASS survives as one
    // member, which is what makes the class switch to braced escapes.
    // Measured: TypeScript answers the same pattern and the same flags,
    // and only the span offsets differ (entry 2 above).
    let class = ir("A ::= [\u{1F600}-\u{1F601}]")["productions"][0]["alts"][0][0].clone();
    assert_eq!(class["pattern"], "[\\u{1f600}-\\u{1f601}]");
    assert_eq!(class["flags"], "u");

    // A name starting with a non-ASCII character is refused in both
    // runtimes, with the same message: the canonical `^[A-Za-z_]`
    // cannot match a surrogate half either.
    for (src, name) in [("\u{1F600} ::= \"x\"", "\u{1F600}"), ("é ::= \"x\"", "é")] {
        let error = parse_ebnf(src).expect_err("a non-ASCII name is refused");
        assert!(
            error.message.starts_with(&format!(
                "ebnf: '{name}' at line 1, column 1 is not a valid symbol"
            )),
            "{}",
            error.message
        );
    }
}

// ---- 4. nested groups are refused sooner ----------------------------

/// A grammar arrives from outside the system, the parse tree nests once
/// per bracket, and a Rust stack that runs out ABORTS the process rather
/// than unwinding. Two caps apply, and the lower one is the shared
/// compiler's.
fn nested(depth: usize) -> String {
    format!("top ::= {}\"x\"{}", "( ".repeat(depth), " )".repeat(depth))
}

#[test]
fn a_grammar_nested_past_the_compilers_limit_is_refused_by_name() {
    // The shared compiler's own limit, inherited from `tabnas-bnf`:
    // TypeScript accepts these depths.
    assert!(convert(&nested(127)).is_ok(), "127 deep still compiles");
    let error = convert(&nested(128)).expect_err("128 deep is refused");
    assert!(
        error.to_string().contains("nests elements more than 128"),
        "the refusal names the limit: {error}"
    );
}

#[test]
fn a_grammar_nested_past_this_front_ends_cap_is_refused_before_it_is_built() {
    // This crate's own RULE-STACK cap, on the FRONT END, so nothing that
    // deep is ever built. TypeScript raises a catchable stack-overflow
    // error several thousand levels further on; Go accepts every depth
    // tried.
    let error = parse_ebnf(&nested(5000)).expect_err("5000 deep is refused");
    assert_eq!(
        error.message,
        "ebnf: grammar nests too deeply (more than 520 rule levels, about 130 nested groups)"
    );
    // ...and the front-end still reads everything the shared compiler
    // would accept, so the cap never takes the better diagnostic away.
    assert!(parse_ebnf(&nested(129)).is_ok());
}

/// The other cap, on how deep the TREE the parse builds may nest.
///
/// A group costs four rule levels and a postfix operator costs one, so
/// the rule-stack cap above never sees a run of operators: `A ::= "x"`
/// and four hundred of them nested the IR 401 deep at a rule depth of
/// 406, and ABORTED the process inside `serde_json::from_value`.
///
/// Measured: TypeScript parses 3000 stacked operators and raises a
/// catchable `Maximum call stack size exceeded` at 5000; Go parses
/// 100000; this port refuses the 130th.
#[test]
fn a_run_of_postfix_operators_is_refused_before_it_is_built() {
    let stacked = |count: usize| format!("A ::= \"x\"{}", "?".repeat(count));
    assert!(
        parse_ebnf(&stacked(129)).is_ok(),
        "129 operators nest 130 deep, which is the cap"
    );
    for count in [130, 3_000, 5_000] {
        let error = parse_ebnf(&stacked(count)).expect_err("past the cap");
        assert_eq!(
            error.message,
            "ebnf: grammar nests elements more than 130 deep, which is past what this front-end \
             will build. Split the rule into named rules.",
            "{count} operators"
        );
    }
}

/// Groups and postfix operators nest the same tree, so one budget covers
/// both. This is the case a per-construct cap misses: 129 nested groups
/// sit exactly AT the group cap, and two operators per group then nest
/// the IR 388 deep, which aborted.
///
/// Measured: TypeScript and Go parse every row; this port admits 43
/// groups of two and refuses 44.
#[test]
fn groups_and_postfix_operators_are_counted_against_one_cap() {
    let mixed = |depth: usize, ops: usize| {
        format!(
            "top ::= {}\"x\"{}",
            "( ".repeat(depth),
            format!(" ){}", "?".repeat(ops)).repeat(depth)
        )
    };
    assert!(
        parse_ebnf(&mixed(43, 2)).is_ok(),
        "1 + 43 * (one group + two operators) == 130, exactly the cap"
    );
    for (depth, ops) in [(44, 2), (129, 2)] {
        let error = parse_ebnf(&mixed(depth, ops)).expect_err("past the cap");
        assert!(
            error.message.contains("nests elements more than 130"),
            "{depth} groups of {ops}: {}",
            error.message
        );
    }
}

/// The CONTROL for both caps: the recursions they do NOT bound.
///
/// Alternation and concatenation build a FLAT list however wide they
/// run, and this dialect has no prefix operator at all, so neither
/// needs a cap and neither may quietly acquire one.
#[test]
fn width_is_not_depth_and_carries_no_cap() {
    let wide: Vec<String> = (0..2_000).map(|index| format!("\"t{index}\"")).collect();
    let grammar = parse_ebnf(&format!("A ::= {}", wide.join(" | "))).expect("parses");
    assert_eq!(grammar.productions[0].alts.len(), 2_000);
    let grammar = parse_ebnf(&format!("A ::= {}", wide.join(" "))).expect("parses");
    assert_eq!(grammar.productions[0].alts[0].len(), 2_000);
}

// ---- 5. a failure is returned, never raised -------------------------

/// `parse_ebnf`, `ebnf_convert`, `to_spec` and `ebnf` all answer a
/// `Result`. TypeScript throws `EbnfParseError` or the shared compiler's
/// own error, and also DECORATES an engine instance with a callable
/// `tn.ebnf` member; Rust has neither exceptions nor dynamic instance
/// properties, so the install path is the free function
/// `ebnf(&mut parser, src, opts)`.
#[test]
fn every_entry_point_answers_a_result() {
    let src = "A ::= ( \"x\"";
    assert!(parse_ebnf(src).is_err());
    assert!(tabnas_ebnf::ebnf_convert(src, None).is_err());
    assert!(tabnas_ebnf::to_spec(src, None).is_err());
    let mut parser = tabnas::Tabnas::new();
    assert!(tabnas_ebnf::ebnf(&mut parser, src, None).is_err());
    // ...and the failure carries the same diagnostic text in each.
    assert_eq!(
        parse_ebnf(src).expect_err("refused").message,
        "ebnf: unclosed group \u{2014} '(' has no matching ')' at line 1, column 7."
    );
}

// ---- the aligned cases, mirroring go/divergence_test.go -------------

/// A raw control character inside a string literal is rejected by every
/// port, at the character's own column.
///
/// Go accepted it until 2026-08-19; TypeScript rejects it through the
/// engine lexer as `unprintable`, and so does this port, because it
/// reads EBNF with that same lexer.
///
/// Go mirror: `TestAlignedControlCharInStringIsRejected`.
#[test]
fn aligned_a_control_character_in_a_string_is_rejected() {
    let error = parse_ebnf("g = \"a\u{1}b\" ;").expect_err(
        "TypeScript rejects a control character, and this port accepting it is an accept/reject \
         split",
    );
    assert_eq!(
        error.column,
        Some(7),
        "TypeScript reports column 7, the character's own position: {}",
        error.message
    );
}

/// The CONTROL for the test above. Without it, "reject control
/// characters" could tighten into "reject anything unusual" and stay
/// green. Space and DEL are legal string body in TypeScript, so they
/// must stay legal here.
///
/// Go mirror: `TestAlignedStringBodyBoundary`.
#[test]
fn aligned_space_and_del_remain_legal_string_body() {
    for (name, src) in [("space", "g = \"a b\" ;"), ("del", "g = \"a\u{7F}b\" ;")] {
        assert!(
            parse_ebnf(src).is_ok(),
            "{name} is legal string body in TypeScript and must stay legal here"
        );
    }
}

/// An unterminated string is reported at the OPENING QUOTE, not at end
/// of source. Go reported column 11 for `g = "abc ;` until it was
/// aligned; TypeScript says 5.
///
/// Go mirror: `TestAlignedUnterminatedStringColumn`.
#[test]
fn aligned_an_unterminated_string_reports_the_opening_quote() {
    let error = parse_ebnf("g = \"abc ;").expect_err("an unterminated string is an error");
    assert_eq!(
        error.column,
        Some(5),
        "column 11 is end of source, which is where the Go port used to point: {}",
        error.message
    );
}

/// The third case measured that day, and the one that already agreed.
/// Kept so a wholesale position change cannot hide inside the two that
/// were repaired.
///
/// Go mirror: `TestAlignedSyntaxErrorColumn`.
#[test]
fn aligned_a_syntax_error_reports_the_same_column() {
    let error = parse_ebnf("g = ;").expect_err("an empty alternative is an error");
    assert_eq!(error.column, Some(5), "{}", error.message);
}

/// The meta-grammar's rule table names one rule differently from the
/// canonical one, and nothing else about the IR changes.
///
/// `@elem-bc` is one of the ENGINE's own builtin action references, and
/// a builtin wins the name lookup, so a rule named `elem` would silently
/// run the engine's list-element push instead of this crate's closure.
/// The canonical front-end hands the engine closures rather than named
/// references, so the collision cannot arise there.
#[test]
fn the_rule_table_calls_the_element_rule_item() {
    let rules = tabnas_ebnf::ebnf_rules();
    let table = rules.as_object().expect("a rule map");
    assert!(table.contains_key("item"), "the rule is named `item` here");
    assert!(
        !table.contains_key("elem"),
        "`elem` is an engine builtin name and must not be used"
    );
    // The IR is unaffected, which is the part that is a contract.
    assert_eq!(
        ir("A ::= \"x\"?")["productions"][0]["alts"][0][0]["kind"],
        "opt"
    );
    // ...and a grammar that uses `elem` as a rule NAME of its own still
    // compiles: the collision is inside the meta-grammar, never in the
    // EBNF a caller writes.
    let parser = engine_for("doc ::= elem+\nelem ::= [a-z]").expect("compiles");
    assert!(parser.parse("abc").is_ok());
}
