// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// Performance regression guard.
//
// Both checks are machine-INDEPENDENT: each compares two ways of doing
// the same work on the SAME machine in the SAME run, so a slow or busy
// box cannot make them flaky. There is deliberately NO absolute
// wall-clock budget.

mod common;

use std::time::Instant;

use common::engine_for;
use tabnas_ebnf::parse_ebnf;

/// A grammar with enough shape to cost something: a repetition, a group,
/// an option, a character class and several productions.
const PERF_GRAMMAR: &str = concat!(
    "List   ::= \"[\" Item ( \",\" Item )* \"]\"\n",
    "Item   ::= Word | Number | List\n",
    "Word   ::= [a-z]+\n",
    "Number ::= \"-\"? [0-9]+\n"
);

const PERF_INPUT: &str = "[abc,12,[x,-3,[y]],z]";

/// How many repetitions each half of a comparison does. Small enough
/// that `cargo test` on the unoptimised profile stays quick, large
/// enough to average out scheduler noise.
const PERF_N: u32 = 30;

/// Compile ONCE and parse many, which is what the documentation
/// recommends for bulk work. Recompiling per document is the
/// anti-pattern, and it has to stay dramatically more expensive, or the
/// advice is wrong.
#[test]
fn compile_once_parse_many() {
    let parser = engine_for(PERF_GRAMMAR).expect("compiles");

    // Warm both paths, and check the parse result en route.
    for _ in 0..3 {
        parser.parse(PERF_INPUT).expect("parses");
        engine_for(PERF_GRAMMAR).expect("compiles");
    }

    let start = Instant::now();
    for _ in 0..PERF_N {
        parser.parse(PERF_INPUT).expect("parses");
    }
    let reuse = start.elapsed().as_secs_f64();

    let start = Instant::now();
    for _ in 0..PERF_N {
        let fresh = engine_for(PERF_GRAMMAR).expect("compiles");
        fresh.parse(PERF_INPUT).expect("parses");
    }
    let rebuild = start.elapsed().as_secs_f64();

    assert!(
        reuse * 4.0 < rebuild,
        "reusing a compiled grammar ({reuse:.4}s for {PERF_N}) should be far cheaper than \
         recompiling per document ({rebuild:.4}s); compiling is the expensive half and the \
         documentation says so"
    );
}

/// The meta-parser that reads EBNF is built once and shared, so the
/// hundredth compile costs what the tenth did. A per-call rebuild, or
/// state accumulating on the shared instance, would show up as a second
/// half slower than the first.
#[test]
fn repeated_compiles_do_not_accumulate() {
    // Warm the cached instance so the first measured batch does not pay
    // for building it.
    for _ in 0..3 {
        parse_ebnf(PERF_GRAMMAR).expect("parses");
    }

    let start = Instant::now();
    for _ in 0..PERF_N {
        parse_ebnf(PERF_GRAMMAR).expect("parses");
    }
    let first = start.elapsed().as_secs_f64().max(1e-6);

    let start = Instant::now();
    for _ in 0..PERF_N {
        parse_ebnf(PERF_GRAMMAR).expect("parses");
    }
    let second = start.elapsed().as_secs_f64();

    assert!(
        second < first * 3.0,
        "the second batch of {PERF_N} compiles took {second:.4}s against the first's \
         {first:.4}s; repeated compiles must not get slower"
    );
}

/// The shared meta-parser is used from several threads at once.
///
/// `Tabnas` parses through `&self` and is `Send + Sync`, and this crate
/// keeps one cached instance for every caller, so per-parse state has to
/// live on the rules and the context. Anything kept on the shared
/// instance would show up here as a wrong answer rather than as a
/// compile failure.
#[test]
fn the_shared_meta_parser_is_used_from_several_threads() {
    let sources = [
        "A ::= \"x\" B\nB ::= [a-z]+",
        "C ::= ( \"p\" | \"q\" )* \"end\"",
        "D ::= #x41 #x42",
        "E ::= \"bad\" (",
    ];
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let src = sources[index % sources.len()].to_string();
            std::thread::spawn(move || (src.clone(), parse_ebnf(&src).is_ok()))
        })
        .collect();
    for handle in handles {
        let (src, ok) = handle.join().expect("the thread finished");
        assert_eq!(ok, !src.ends_with('('), "{src:?} answered differently");
    }
}
