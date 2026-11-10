// The tree a host translates, `parse_ebnf_tree`: an EBNF document read as
// the schema grammar-rules. `test/spec/tree.tsv` holds each grammar to its
// tree's JSON text, member order included, as `ts/test/tree.test.js` and
// `go/tree_test.go` hold theirs; the order is part of the contract, since
// a host streams the members in that order and the part makes the same
// events from the rule passes of `make()`.
//
// Repetition is replacement (AGENTS.md): the rules of a grammar, the
// alternatives of a rule or a group and the elements of an alternative
// add no rule depth however many there are.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The repository's root, above `rs/`.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A tree's JSON text as JavaScript writes it: its members in order, and a
/// whole number without a fraction.
fn text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9.007_199_254_740_992e15 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        serde_json::Value::Array(items) => {
            let items: Vec<String> = items.iter().map(text).collect();
            format!("[{}]", items.join(","))
        }
        serde_json::Value::Object(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(key, value)| {
                    format!("{}:{}", serde_json::Value::String(key.clone()), text(value))
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        other => other.to_string(),
    }
}

/// A row of `test/spec/tree.tsv`.
struct Row {
    line: usize,
    grammar: String,
    expected: String,
}

/// A grammar cell's `\n`, `\r`, `\t` and `\\` are the characters they
/// name, as tabnas-support reads a cell; any other backslash is itself.
fn unescape(cell: &str) -> String {
    let mut out = String::new();
    let mut chars = cell.chars().peekable();
    while let Some(c) = chars.next() {
        if '\\' == c {
            let named = match chars.peek() {
                Some('n') => Some('\n'),
                Some('r') => Some('\r'),
                Some('t') => Some('\t'),
                Some('\\') => Some('\\'),
                _ => None,
            };
            if let Some(named) = named {
                chars.next();
                out.push(named);
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The fixture rows: a header, then a grammar and its expected cell to a
/// line, a line starting with `#` a comment; the expected cell is the
/// tree's JSON text, or `ERROR:<code>`.
fn rows() -> Vec<Row> {
    let raw = fs::read_to_string(root().join("test").join("spec").join("tree.tsv"))
        .expect("test/spec/tree.tsv");
    let mut out = Vec::new();
    for (i, line) in raw.split('\n').enumerate() {
        if 0 == i || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (grammar, expected) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("tree.tsv:{} has no expected cell", i + 1));
        out.push(Row {
            line: i + 1,
            grammar: unescape(grammar),
            expected: expected.to_string(),
        });
    }
    out
}

#[test]
fn tree_spec() {
    let mut failures = Vec::new();
    for row in rows() {
        let read = tabnas_ebnf::parse_ebnf_tree(&row.grammar);
        if let Some(want) = row.expected.strip_prefix("ERROR:") {
            match read {
                Err(error) if error.code.as_deref() == Some(want) => {}
                other => failures.push(format!(
                    "tree.tsv:{} {:?}: {:?}, want {want}",
                    row.line,
                    row.grammar,
                    other.map(|tree| text(&tree.to_json()))
                )),
            }
            continue;
        }
        match read {
            Ok(tree) => {
                let json = tree.to_json();
                if let Err(error) = tabnas_bnf::check_rules(&json) {
                    failures.push(format!(
                        "tree.tsv:{} {:?}: not a grammar-rules tree: {error}",
                        row.line, row.grammar
                    ));
                }
                let got = text(&json);
                if got != row.expected {
                    failures.push(format!(
                        "tree.tsv:{} {:?}\n  got:      {got}\n  expected: {}",
                        row.line, row.grammar, row.expected
                    ));
                }
            }
            Err(error) => {
                failures.push(format!("tree.tsv:{} {:?}: {error}", row.line, row.grammar))
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every grammar of the fixtures and the corpora that the tree parser
/// reads is a tree of the schema.
#[test]
fn every_tree_the_reader_builds_is_a_grammar_rules_tree() {
    let mut sources: Vec<String> = rows().into_iter().map(|row| row.grammar).collect();
    for entry in
        fs::read_dir(root().join("ts").join("test").join("grammar")).expect("ts/test/grammar")
    {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|ext| ext == "ebnf") {
            sources.push(fs::read_to_string(path).expect("a grammar"));
        }
    }
    let oracle: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            root()
                .join("rs")
                .join("tests")
                .join("oracle")
                .join("ebnf-ir.json"),
        )
        .expect("the oracle"),
    )
    .expect("the oracle is JSON");
    for case in oracle.as_array().expect("its cases") {
        sources.push(case["src"].as_str().expect("a grammar").to_string());
    }
    let mut read = 0;
    for src in &sources {
        let Ok(tree) = tabnas_ebnf::parse_ebnf_tree(src) else {
            continue;
        };
        if let Err(error) = tabnas_bnf::check_rules(&tree.to_json()) {
            panic!("{src:?}: {error}");
        }
        read += 1;
    }
    assert!(60 < read, "{read} trees read");
}

#[test]
fn make_builds_a_fresh_parser_whose_value_is_the_tree() {
    let tree = tabnas_ebnf::make()
        .parse("x ::= \"y\"\n")
        .expect("a grammar");
    let want = tabnas_ebnf::parse_ebnf_tree("x ::= \"y\"\n").expect("a grammar");
    assert_eq!(text(&tree.to_json()), text(&want.to_json()));
}

/// The deepest rule a parse of `src` reaches.
fn depth(src: &str) -> usize {
    let mut parser = tabnas_ebnf::make();
    let deepest = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&deepest);
    parser.subscribe_rule_done(move |rule, _context, done| {
        if !done.probe {
            seen.fetch_max(rule.d, Ordering::Relaxed);
        }
    });
    parser.parse(src).expect("the document parses");
    deepest.load(Ordering::Relaxed)
}

#[test]
fn repetition_adds_no_rule_depth() {
    const N: usize = 10_000;
    type Document = fn(usize) -> String;
    let repeated: [(&str, Document); 7] = [
        ("rules of a grammar", |n| {
            let rules: Vec<String> = (0..n).map(|i| format!("r{i} ::= \"x\"\n")).collect();
            rules.concat()
        }),
        ("ISO rules of a grammar", |n| {
            let rules: Vec<String> = (0..n).map(|i| format!("r{i} = \"x\" ;\n")).collect();
            rules.concat()
        }),
        ("alternatives of a rule", |n| {
            let items: Vec<String> = (0..n).map(|i| format!("r{i}")).collect();
            format!("a ::= {}\n", items.join(" | "))
        }),
        ("elements of an alternative", |n| {
            let items: Vec<String> = (0..n).map(|i| format!("r{i}*")).collect();
            format!("a ::= {}\n", items.join(" "))
        }),
        ("ISO elements of an alternative", |n| {
            let items: Vec<String> = (0..n).map(|i| format!("r{i}")).collect();
            format!("a = {} ;\n", items.join(" , "))
        }),
        ("alternatives of a group", |n| {
            let items: Vec<String> = (0..n)
                .map(|i| format!("[{}]", char::from(b'a' + (i % 26) as u8)))
                .collect();
            format!("a ::= ( {} )\n", items.join(" | "))
        }),
        ("elements of a group", |n| {
            format!("a ::= ( {})?\n", "\"x\" ".repeat(n))
        }),
    ];
    for (what, document) in repeated {
        // Two items, since a list's first item can sit apart from its
        // repeated ones: the loop's own item is the second.
        assert_eq!(depth(&document(N)), depth(&document(2)), "{what}");
    }
}

/// The IR parser's cap on nested groups holds for the tree too
/// (DIVERGENCE.md): a tree nests once per group, and its drop recurses.
#[test]
fn nesting_past_the_group_cap_is_refused() {
    let at = |n: usize| format!("a ::= {}\"x\"{}\n", "( ".repeat(n), " )".repeat(n));
    assert!(tabnas_ebnf::parse_ebnf_tree(&at(129)).is_ok());
    let refused = tabnas_ebnf::parse_ebnf_tree(&at(130)).expect_err("past the cap");
    assert_eq!(refused.code.as_deref(), Some("ebnf_group_depth"));
}
