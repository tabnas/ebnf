// The engine's error carries a code, position, hint and a formatted
// report, so `Result<_, TabnasError>` trips clippy's `result_large_err`;
// an integration test is a crate of its own, so it allows it again.
#![allow(clippy::result_large_err)]

// A parse from a reader, a window of the source at a time (admin ADR-30;
// tabnas/parser's doc/chunked-source.md, "Proving it"), against the same
// text parsed whole. `make()`, the tree parser, declares `lex.window`: the
// meta-grammar's match tokens and its ISO comments are read behind `ebnf-reach`, a matcher
// that reads the window on until the character that decides where their
// token ends lies inside it, and every other token is the engine's.
//
// Every row of the shared fixture, every grammar of `ts/test/grammar` and
// the source of every case of the oracle (`rs/tests/oracle/ebnf-ir.json`) is parsed whole and from a reader
// that hands its bytes over in pieces of 1, 3 and 64 bytes, under the
// engine's window and under windows whose margin and block are a few
// characters, so that every token meets the window's edge; a long
// document, whose long tokens of every kind run past the engine's window,
// in pieces of 4 KiB too. The value, the tokens the lexer gives, the rule
// passes, and an error's code, position, row, column and length must be
// the same. An error's excerpt is not compared: a parse from a reader
// shows a long line only as far back as the window reaches
// (tabnas/parser's DIVERGENCE.md).
//
// ts/test/window.test.js and go/window_test.go hold the TypeScript and Go
// ports to the same.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tabnas::{RuleState, Tabnas, TabnasError, Value, WindowSizes};

/// The repository root, one level above this crate.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// A reader that hands its bytes over at most `size` at a time.
struct Pieces<'a> {
    bytes: &'a [u8],
    at: usize,
    size: usize,
}

impl Read for Pieces<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.size.min(buf.len()).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// A read of a source: the pieces a reader hands its bytes over in, and the
/// window it is read through, the engine's own (64 KiB) where it is `None`,
/// or one whose margin and block are a few characters, which the hidden
/// `parse_reader_with_sizes` sets, its reads as small as the pieces.
type Reading = (usize, Option<(usize, usize)>);

const PIECES: [usize; 3] = [1, 3, 64];
const WINDOWS: [Option<(usize, usize)>; 4] = [None, Some((1, 1)), Some((3, 2)), Some((64, 64))];

/// A fixture row or a grammar is read in every pieces size through every
/// window.
fn reads() -> Vec<Reading> {
    PIECES
        .iter()
        .flat_map(|&size| WINDOWS.iter().map(move |&window| (size, window)))
        .collect()
}

/// A long document in pieces of 1, 3, 64 and 4096 bytes through the
/// engine's window, whose edges its long tokens straddle, and through a
/// window of 64 and 64 characters.
const LONG_READS: [Reading; 5] = [
    (1, None),
    (3, None),
    (64, None),
    (4096, None),
    (64, Some((64, 64))),
];

type Log = Arc<Mutex<(Vec<String>, Vec<String>)>>;

/// The tree parser with a log of the tokens the lexer gives and the rule
/// passes.
fn logged(log: &Log) -> Tabnas {
    let mut parser = tabnas_ebnf::make();
    let tokens = Arc::clone(log);
    parser.subscribe_tokens(move |token| {
        tokens.lock().unwrap().0.push(format!(
            "{} {:?} {} {} {}:{}",
            token.tin, token.src, token.val, token.site.pos, token.site.ri, token.site.ci
        ));
    });
    let rules = Arc::clone(log);
    parser.subscribe_rule_done(move |rule, _context, done| {
        let state = match done.state {
            RuleState::Open => "open",
            RuleState::Close => "close",
        };
        rules
            .lock()
            .unwrap()
            .1
            .push(format!("{} {state} {}", rule.name.as_str(), rule.d));
    });
    parser
}

/// What a parse showed: its value, or its error without the excerpt; and
/// the tokens and rule passes `log` gathered, which is empty again after.
fn seen(log: &Log, parsed: Result<Value, TabnasError>) -> (String, Vec<String>, Vec<String>) {
    let outcome = match parsed {
        Ok(value) => format!("ok {value}"),
        Err(e) => format!(
            "err {} at {} {}:{} {} {:?}",
            e.code, e.pos, e.row, e.col, e.len, e.token
        ),
    };
    let (tokens, rules) = std::mem::take(&mut *log.lock().unwrap());
    (outcome, tokens, rules)
}

/// `text` parsed whole and from a reader in each of `reads`; the count of
/// reader parses compared.
fn differential(parser: &Tabnas, log: &Log, at: &str, text: &str, reads: &[Reading]) -> usize {
    let whole = seen(log, parser.parse(text));
    for &(size, window) in reads {
        let reader = Pieces {
            bytes: text.as_bytes(),
            at: 0,
            size,
        };
        let parsed = match window {
            None => parser.parse_reader(reader),
            Some((margin, block)) => parser.parse_reader_with_sizes(
                reader,
                WindowSizes {
                    margin,
                    block,
                    chunk: size,
                },
            ),
        };
        let read = seen(log, parsed);
        assert_eq!(read, whole, "{at} in pieces of {size}, window {window:?}");
    }
    reads.len()
}

/// The grammar cell of every row of the shared fixture, its `\\n`, `\\r`,
/// `\\t` and `\\\\` the characters they name.
fn rows() -> Vec<(String, String)> {
    let file = repo_root().join("test").join("spec").join("tree.tsv");
    let text = fs::read_to_string(&file).expect("test/spec/tree.tsv reads");
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let grammar = line.split('\t').next().unwrap_or_default();
        out.push((format!("tree.tsv:{}", index + 1), unescape(grammar)));
    }
    out
}

/// A fixture cell with its `\\n`, `\\r`, `\\t` and `\\\\` read as the
/// characters they name, and every other backslash as itself.
fn unescape(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut chars = cell.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    out.push('\n');
                    chars.next();
                }
                Some('r') => {
                    out.push('\r');
                    chars.next();
                }
                Some('t') => {
                    out.push('\t');
                    chars.next();
                }
                Some('\\') => {
                    out.push('\\');
                    chars.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Every file under `dir` whose name ends with `ext`, in a stable order.
fn files(dir: &Path, ext: &str, into: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, ext, into);
        } else if path.to_string_lossy().ends_with(ext) {
            into.push(path);
        }
    }
}

/// A grammar of every kind of token, the long ones longer than the engine's
/// 64 KiB window, and characters past ASCII among them, so that tokens of
/// every kind straddle the window's edges as a reader hands the text over.
/// The same document as `longDocument` in ts/test/window.test.js and
/// go/window_test.go.
fn long_document() -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "/* {}*/\n",
        "a comment past the window, \u{e9} \u{1d11e} * / ".repeat(2000)
    ));
    parts.push(format!(
        "(* {}*)\n",
        "an ISO comment past the window, \u{fc} ( * \n".repeat(1800)
    ));
    parts.push(format!(
        "long ::= \"{}\"\n",
        "a string past the window \u{e7} ' ".repeat(2800)
    ));
    parts.push(format!(
        "single ::= '{}'\n",
        "a quoted string past the window \" ".repeat(2400)
    ));
    parts.push(format!(
        "klass ::= [{}]\n",
        "a-z0-9_#x41#xE9-#xFF\u{f1}".repeat(2600)
    ));
    parts.push(format!("named ::= {}\n", "n".repeat(70000)));
    parts.push(format!("{} ::= #x41\n", "n".repeat(70000)));
    for i in 0..4000 {
        let hex = format!("{:X}", 0x41 + (i % 26));
        parts.push(format!(
            "r{i} ::= #x{hex} | [a-z#x{hex}] | \"s{i}\" | 'q{i}' | (r{i} | \"x\")* | n{i}+ | \"y\"? /* c{i} \u{e9} */\n"
        ));
        parts.push(format!(
            "s{i} = \"a\" , \"b\" , ( \"c\" | \"d\" ) ; (* iso {i} *)\n"
        ));
        parts.push(format!("t{i} ::= [^\"#x0A] ( u{i} v{i} )+\n"));
    }
    parts.push("end ::= \"done\"\n".into());
    parts.concat()
}

#[test]
fn make_reads_a_reader_a_window_at_a_time() {
    assert!(tabnas_ebnf::make().options.lex.window);
}

#[test]
fn every_fixture_row_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(&log);
    let rows = rows();
    assert!(rows.len() > 30, "{}", rows.len());
    let mut compared = 0;
    for (index, (at, grammar)) in rows.iter().enumerate() {
        compared += differential(&parser, &log, at, grammar, &reads());
        if index % 20 == 0 || index + 1 == rows.len() {
            eprintln!(
                "window fixtures: {} of {} rows ({}%), {compared} reader parses",
                index + 1,
                rows.len(),
                (index + 1) * 100 / rows.len()
            );
        }
    }
    assert_eq!(compared, rows.len() * reads().len());
}

#[test]
fn every_grammar_and_oracle_source_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(&log);
    let mut paths = Vec::new();
    files(
        &repo_root().join("ts").join("test").join("grammar"),
        ".ebnf",
        &mut paths,
    );
    let mut documents: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let text = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (path.display().to_string(), text)
        })
        .collect();
    let oracle = repo_root()
        .join("rs")
        .join("tests")
        .join("oracle")
        .join("ebnf-ir.json");
    let cases: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(oracle).expect("the oracle reads"))
            .expect("the oracle is JSON");
    for (index, case) in cases
        .as_array()
        .expect("the oracle is a list")
        .iter()
        .enumerate()
    {
        let src = case["src"].as_str().expect("an oracle case has a src");
        documents.push((format!("oracle case {index}"), src.to_string()));
    }
    assert!(documents.len() > 60, "{}", documents.len());
    let mut compared = 0;
    for (at, text) in &documents {
        compared += differential(&parser, &log, at, text, &reads());
    }
    assert_eq!(compared, documents.len() * reads().len());
}

#[test]
fn a_long_document_parses_the_same_from_a_reader() {
    let log: Log = Arc::default();
    let parser = logged(&log);
    let text = long_document();
    assert!(text.len() > 512 * 1024, "{}", text.len());
    let tree = parser.parse(&text).expect("the long document parses");
    log.lock().unwrap().0.clear();
    log.lock().unwrap().1.clear();
    assert_eq!(
        tree.to_json()["rules"].as_array().map(Vec::len),
        Some(12006)
    );
    assert_eq!(
        differential(&parser, &log, "the long document", &text, &LONG_READS),
        LONG_READS.len()
    );
}

#[test]
fn a_long_document_fails_the_same_from_a_reader() {
    // A token that runs past the engine's window and is then refused, or
    // never ends: the error is the whole text's, code, place and token.
    let log: Log = Arc::default();
    let parser = logged(&log);
    let pad = format!("/* {}*/\na ::= \"x\"\n", "padding \u{fc} ".repeat(9000));
    let bad = [
        format!("{pad}b ::= [{}\nc ::= \"y\"\n", "a-z".repeat(30000)),
        format!("{pad}b ::= \"{}\n", "unclosed string ".repeat(6000)),
        format!("{pad}b ::= ( c | {}\n", "d ".repeat(40000)),
        format!("{pad}(* {}\n", "an unclosed ISO comment ".repeat(4000)),
        format!("{pad}b ::= c - {}\n", "d ".repeat(40000)),
        format!("{pad}b ::= [z-a] {}\n", "e ".repeat(40000)),
        format!("{pad}b ::= {}#x{}\n", "f ".repeat(40000), "4".repeat(70000)),
        format!("{pad}{} ::= \"g\" ]\n", "j".repeat(70000)),
    ];
    let mut compared = 0;
    for (index, text) in bad.iter().enumerate() {
        let whole = seen(&log, parser.parse(text));
        assert!(
            whole.0.starts_with("err "),
            "{index}: {}",
            &whole.0[..200.min(whole.0.len())]
        );
        compared += differential(
            &parser,
            &log,
            &format!("long error document {index}"),
            text,
            &LONG_READS,
        );
    }
    assert_eq!(compared, bad.len() * LONG_READS.len());
}

#[test]
fn a_long_document_parses_from_a_reader_in_time_linear_in_its_length() {
    // Ten times the rules, read through the engine's window in pieces of 4
    // KiB, take about ten times as long, never the square of it.
    let parser = tabnas_ebnf::make();
    let document = |rules: usize| -> String {
        let mut text = String::new();
        for i in 0..rules {
            text.push_str(&format!("r{i} ::= [A-Z] | \"b{i}\" | #x41 /* d */\n"));
        }
        text
    };
    let time = |text: &str| -> f64 {
        let started = Instant::now();
        parser
            .parse_reader(Pieces {
                bytes: text.as_bytes(),
                at: 0,
                size: 4096,
            })
            .expect("the document parses");
        started.elapsed().as_secs_f64()
    };
    let (small, large) = (document(2000), document(20000));
    time(&small);
    let (one, ten) = (time(&small), time(&large));
    eprintln!(
        "window linear time: {} rules {one:.3}s, {} rules {ten:.3}s, {:.1} times",
        2000,
        20000,
        ten / one
    );
    assert!(ten < 30.0 * one, "{one:.3}s and {ten:.3}s");
}
