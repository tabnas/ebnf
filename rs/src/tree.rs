// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! An EBNF document read as the schema `grammar-rules`: its rules as the
//! text writes them, uncompiled and in source order (tabnas/bnf's
//! `docs/grammar-rules.md`, and section 13.1 of alchemy's
//! `docs/streaming-plan.md`). The Rust port of `ts/src/tree.ts`: the tree
//! a host translates, whose events the part, `alchemy/transduce.alc`,
//! makes from the rule passes of [`make`].
//!
//! The tree parser reads EBNF with the meta-grammar the IR parser reads it
//! with, its lexer and every alternate as they are, and actions of its own
//! under the names the IR parser's take, so the rule passes a part reads
//! are the meta-grammar's. A token whose text needs reading (a class, a
//! code point) is given what it reads as its value by the action that
//! reads it, since a part reads a token's value and never the work an
//! action does with it.
//!
//! The tree holds what the schema allows, and EBNF what this crate reads,
//! so the parser refuses the rest, each with a code of its own: a name
//! that is not one (`ebnf_name`), an empty string (`ebnf_empty_literal`),
//! a code point past `#x10FFFF` or with no digits (`ebnf_code_point`), a
//! class range that ends below where it starts (`ebnf_class_range`), an
//! empty class (`ebnf_empty_class`), a stray bracket (`ebnf_char_class`),
//! an empty alternative (`ebnf_empty_alternative`), a leading comma
//! (`ebnf_leading_comma`), a subtraction (`ebnf_subtraction`), a special
//! sequence or a `?` where an element is due (`ebnf_special_sequence`),
//! ISO bracket repetition (`ebnf_iso_repetition`), a `*` or `+` where an
//! element is due (`ebnf_dangling_postfix`) and a group left open
//! (`ebnf_unclosed`). The IR parser's cap on nested groups holds here too
//! (`ebnf_group_depth`, DIVERGENCE.md), since a tree nests once per group
//! and its drop recurses.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use tabnas::{ActionError, Context, LexMatcher, Lexer, Rule, Tabnas, Value};

use crate::converter::EbnfParseError;
use crate::parser_ebnf::{
    build_parser, open_token, push_node, set_node, token_string, valid_name, DEPTH_CODE,
    DEPTH_MESSAGE, MAX_GROUP_DEPTH,
};

/// The codes the tree parser refuses a source with, beside the engine's:
/// each with its message and its hint.
const TREE_ERRORS: [(&str, &str, &str); 13] = [
    (
        "ebnf_name",
        "the name {src} is not a symbol name: one starts with a letter or an underscore, and \
         goes on with letters, digits, _, . and -",
        "Spell a name as a letter or an underscore, then letters, digits, _, . and -.",
    ),
    (
        "ebnf_empty_literal",
        "the string {src} is empty, and EBNF has no epsilon terminal",
        "Write A? to make A optional.",
    ),
    (
        "ebnf_code_point",
        "the code point in {src} has no hex digits, or names no Unicode code point (the \
         largest is #x10FFFF)",
        "A code point is #x and its hex digits, no larger than #x10FFFF.",
    ),
    (
        "ebnf_class_range",
        "the character class {src} has a range that ends below where it starts",
        "Write a range from its lower end to its higher, as [a-z].",
    ),
    (
        "ebnf_empty_class",
        "the character class {src} is empty, and matches nothing",
        "A class holds a character or a range at least.",
    ),
    (
        "ebnf_char_class",
        "the {src} opens or closes no character class: a class opens and closes on one line, \
         and holds no ]",
        "Write a literal ] as the string \"]\", and the ISO option [ A ] as A?.",
    ),
    (
        "ebnf_empty_alternative",
        "an alternative is empty, and EBNF has no epsilon terminal",
        "Write A? rather than A |.",
    ),
    (
        "ebnf_leading_comma",
        "the comma separates nothing: ISO 14977 writes it between two items",
        "Write A , B, with an item on each side of the comma.",
    ),
    (
        "ebnf_subtraction",
        "the subtraction {src} is not supported: the tree has no difference of two languages",
        "Write a difference of single characters as a negated class, as [^abc].",
    ),
    (
        "ebnf_special_sequence",
        "the {src} is where an element is due: a postfix ? follows an element, and ISO 14977 \
         special sequences are not supported",
        "Write A? after the element it repeats.",
    ),
    (
        "ebnf_iso_repetition",
        "the bracket {src} is ISO 14977 repetition, which this dialect does not read",
        "Write A* for { A }, and A? for the ISO [ A ].",
    ),
    (
        "ebnf_dangling_postfix",
        "the {src} is where an element is due: repetition in this dialect is postfix",
        "Write A* and A+, after the element they repeat.",
    ),
    (
        "ebnf_unclosed",
        "a group is not closed",
        "Close the group with ).",
    ),
];

/// An object of the tree, its members in the order given.
fn tree_object(fields: Vec<(&str, Value)>) -> Value {
    let mut map = IndexMap::new();
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::object(map)
}

/// The error a code raises, with its message as the detail.
fn refuse(code: &str) -> ActionError {
    let message = TREE_ERRORS
        .iter()
        .find(|(name, _, _)| *name == code)
        .map_or(code, |(_, message, _)| message);
    ActionError::new(code, message.to_string())
}

/// The code point a run of hex digits names, or `None` where it names
/// none.
fn code_point_of(hex: &str) -> Option<u32> {
    u64::from_str_radix(hex, 16)
        .ok()
        .filter(|n| *n <= 0x10_FFFF)
        .and_then(|n| u32::try_from(n).ok())
}

/// A class's member, its bounds code points.
fn member(from: u32, to: u32) -> Value {
    tree_object(vec![
        ("from", Value::Number(f64::from(from))),
        ("to", Value::Number(f64::from(to))),
    ])
}

/// A class's negation and members, read as the IR parser reads them (`^`
/// first negates it, `#xNN` is a code point, `a-b` is a range, and any
/// other `-` is a member of its own), or the code it is refused with.
fn class_value(src: &str) -> Result<(bool, Vec<Value>), &'static str> {
    let inner = src.get(1..src.len().saturating_sub(1)).unwrap_or_default();
    let (negated, body) = match inner.strip_prefix('^') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    if body.is_empty() {
        return Err("ebnf_empty_class");
    }
    // A code point, or `None` for a `-`.
    let bytes = body.as_bytes();
    let mut items: Vec<Option<u32>> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let c = bytes[i];
        if b'#' == c && Some(&b'x') == bytes.get(i + 1) {
            let mut j = i + 2;
            while j < body.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            let point = if j == i + 2 {
                None
            } else {
                code_point_of(&body[i + 2..j])
            };
            items.push(Some(point.ok_or("ebnf_code_point")?));
            i = j;
            continue;
        }
        if b'-' == c {
            items.push(None);
            i += 1;
            continue;
        }
        let ch = body[i..].chars().next().unwrap_or_default();
        items.push(Some(u32::from(ch)));
        i += ch.len_utf8();
    }
    let mut members = Vec::new();
    let mut k = 0;
    while k < items.len() {
        let Some(low) = items[k] else {
            members.push(member(0x2D, 0x2D));
            k += 1;
            continue;
        };
        if let (Some(None), Some(Some(high))) = (items.get(k + 1), items.get(k + 2)) {
            if *high < low {
                return Err("ebnf_class_range");
            }
            members.push(member(low, *high));
            k += 3;
            continue;
        }
        members.push(member(low, low));
        k += 1;
    }
    Ok((negated, members))
}

/// Give the first token a rule matched in its open state its value.
fn set_open_val(rule: &mut Rule, val: Value) {
    if let Some(first) = Rc::make_mut(&mut rule.o).first_mut() {
        first.val = val;
    }
}

/// A symbol name read from the first token a rule matched, or the
/// refusal of a token that spells none.
fn named(rule: &mut Rule, context: &mut tabnas::Context) -> Result<Option<String>, ActionError> {
    let Some(token) = open_token(rule, 0) else {
        return Ok(None);
    };
    let name = token_string(&token, rule, context);
    if !valid_name(&name) {
        return Err(refuse("ebnf_name"));
    }
    Ok(Some(name))
}

/// Register the tree parser's actions under the names the meta-grammar's
/// document gives them: what `ts/src/tree.ts`'s `TREE` builds.
fn register_tree_refs(parser: &mut Tabnas) {
    parser.state_action_ref("@ebnf-bo", |rule, _context| {
        set_node(
            rule,
            tree_object(vec![
                ("type", Value::String("grammar".into())),
                ("rules", Value::array(Vec::new())),
            ]),
        );
        Ok(())
    });

    parser.action_with_context("@prod-name", |rule, context| {
        if let Some(name) = named(rule, context)? {
            rule.u_mut().insert("name".into(), Value::String(name));
        }
        Ok(())
    });
    parser.state_action_ref("@prod-bc", |rule, _context| {
        if rule.child_node.is_undefined() {
            return Ok(());
        }
        let name = match rule.u.get("name") {
            Some(Value::String(name)) => name.clone(),
            _ => String::new(),
        };
        let entry = tree_object(vec![
            ("type", Value::String("rule".into())),
            ("name", Value::String(name)),
            ("alternatives", rule.child_node.clone()),
        ]);
        if let Some(grammar) = rule.node.borrow_mut().as_object_mut() {
            if let Some(rules) = grammar.get_mut("rules").and_then(Value::as_array_mut) {
                rules.push(entry);
            }
        }
        Ok(())
    });

    parser.state_action_ref("@alts-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });
    parser.state_action_ref("@alts-bc", |rule, _context| {
        if rule.child_node.is_undefined() {
            return Ok(());
        }
        let sequence = rule.child_node.clone();
        push_node(rule, sequence);
        Ok(())
    });

    parser.state_action_ref("@seq-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });
    for (name, code) in [
        ("@fail-empty-alt", "ebnf_empty_alternative"),
        ("@fail-leading-comma", "ebnf_leading_comma"),
        ("@fail-special-sequence", "ebnf_special_sequence"),
        ("@fail-subtraction", "ebnf_subtraction"),
        ("@fail-iso-repetition", "ebnf_iso_repetition"),
        ("@fail-char-class", "ebnf_char_class"),
        ("@fail-dangling-postfix", "ebnf_dangling_postfix"),
        ("@fail-unclosed-group", "ebnf_unclosed"),
    ] {
        parser.action_with_context(name, move |_rule, _context| Err(refuse(code)));
    }

    parser.alt_condition("@item-first", |rule, _context| {
        !matches!(rule.u.get("got"), Some(Value::Bool(true)))
    });
    parser.action_with_context("@item-take", |rule, _context| {
        let atom = rule.child_node.clone();
        let bag = rule.u_mut();
        bag.insert("got".into(), Value::Bool(true));
        bag.insert("atom".into(), atom);
        Ok(())
    });
    // The atom, its repetitions appended, onto the alternative.
    parser.state_action_ref("@item-bc", |rule, _context| {
        if !matches!(rule.u.get("got"), Some(Value::Bool(true))) {
            return Ok(());
        }
        let Some(mut element) = rule.u.get("atom").cloned().filter(|a| !a.is_undefined()) else {
            return Ok(());
        };
        let reps = match &rule.child_node {
            Value::Array(items) => items.as_ref().clone(),
            _ => Vec::new(),
        };
        if !reps.is_empty() {
            if let Some(members) = element.as_object_mut() {
                members.insert("repeat".into(), Value::array(reps));
            }
        }
        push_node(rule, element);
        Ok(())
    });

    // The postfix operators, innermost first, each a repetition.
    parser.state_action_ref("@post-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });
    for (name, max) in [
        ("@post-opt", Some(1.0)),
        ("@post-star", None),
        ("@post-plus", None),
    ] {
        let min = if "@post-plus" == name { 1.0 } else { 0.0 };
        parser.action_with_context(name, move |rule, _context| {
            let mut fields = vec![("min", Value::Number(min))];
            if let Some(max) = max {
                fields.push(("max", Value::Number(max)));
            }
            push_node(rule, tree_object(fields));
            Ok(())
        });
    }
    parser.state_action_ref("@post-bc", |rule, _context| {
        let reps = match &rule.child_node {
            Value::Array(items) => items.as_ref().clone(),
            _ => Vec::new(),
        };
        for rep in reps {
            push_node(rule, rep);
        }
        Ok(())
    });

    parser.state_action_ref("@atom-bo", |rule, _context| {
        set_node(rule, Value::Undefined);
        rule.u_mut().insert("group".into(), Value::Bool(false));
        Ok(())
    });
    parser.action_with_context("@atom-st", |rule, context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let text = token_string(&token, rule, context);
        if text.is_empty() {
            return Err(refuse("ebnf_empty_literal"));
        }
        set_node(
            rule,
            tree_object(vec![
                ("type", Value::String("literal".into())),
                ("text", Value::String(text)),
                ("caseSensitive", Value::Bool(true)),
            ]),
        );
        Ok(())
    });
    // The class's negation and members, which its token carries from here.
    parser.action_with_context("@atom-cc", |rule, _context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let (negated, members) = class_value(token.src.as_str()).map_err(refuse)?;
        set_open_val(
            rule,
            tree_object(vec![
                ("negated", Value::Bool(negated)),
                ("members", Value::array(members.clone())),
            ]),
        );
        set_node(
            rule,
            tree_object(vec![
                ("type", Value::String("class".into())),
                ("negated", Value::Bool(negated)),
                ("members", Value::array(members)),
            ]),
        );
        Ok(())
    });
    // The code point, which its token carries from here.
    parser.action_with_context("@atom-hx", |rule, _context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let value = token
            .src
            .as_str()
            .get(2..)
            .and_then(code_point_of)
            .ok_or_else(|| refuse("ebnf_code_point"))?;
        set_open_val(rule, Value::Number(f64::from(value)));
        set_node(
            rule,
            tree_object(vec![
                ("type", Value::String("char".into())),
                ("value", Value::Number(f64::from(value))),
            ]),
        );
        Ok(())
    });
    parser.action_with_context("@atom-tx", |rule, context| {
        if let Some(name) = named(rule, context)? {
            set_node(
                rule,
                tree_object(vec![
                    ("type", Value::String("reference".into())),
                    ("name", Value::String(name)),
                ]),
            );
        }
        Ok(())
    });
    parser.action_with_context("@atom-lp", |rule, _context| {
        if MAX_GROUP_DEPTH < rule.d {
            return Err(ActionError::new(DEPTH_CODE, DEPTH_MESSAGE));
        }
        rule.u_mut().insert("group".into(), Value::Bool(true));
        Ok(())
    });
    parser.alt_condition("@atom-group-c", |rule, _context| {
        matches!(rule.u.get("group"), Some(Value::Bool(true)))
    });
    parser.action_with_context("@atom-group-close", |rule, _context| {
        let alternatives = rule.child_node.clone();
        set_node(
            rule,
            tree_object(vec![
                ("type", Value::String("group".into())),
                ("alternatives", alternatives),
            ]),
        );
        Ok(())
    });
}

/// A fresh parser that reads an EBNF document as the tree: the parser a
/// host streams a document through with the part, whose rule passes the
/// part reads, and whose value is the tree (`Undefined` for a source that
/// makes no pass).
///
/// ```
/// let tree = tabnas_ebnf::make().parse("a ::= b").unwrap();
/// assert_eq!(tree.to_json()["rules"][0]["name"], "a");
/// ```
///
/// It reads a source a window at a time when it is parsed from a reader
/// ([`Tabnas::parse_reader`], `lex.window`): its matchers read the text only
/// through the window, the meta-grammar's match tokens behind
/// [`ebnf_reach`].
#[must_use]
pub fn make() -> Tabnas {
    // The meta-grammar is this crate's own, so it always installs.
    let mut parser = build_parser(register_tree_refs, &TREE_ERRORS)
        .expect("ebnf: the tree parser installs its own meta-grammar");
    parser
        .set_options(|options| {
            options.lex.window = true;
            options.lex.matchers.insert(
                "ebnf-reach".to_string(),
                reach_matcher("ebnf-reach", ebnf_reach),
            );
        })
        .expect("ebnf: the tree parser takes the window and its reach");
    parser
}

/// Whether the character that decides how far EBNF's own matchers read lies
/// in `window`, the text from the cursor, and when it does not, how many
/// characters they would read there. `::=` reads three characters; a
/// character class reads to the `]` that closes it or the line's end, where
/// an unclosed class fails; a code point reads `#x` and its hex digits and
/// the character after; `(` reads the character after, and an ISO comment,
/// `(*`, to the `*)` that closes it. Every other token is the engine's,
/// whose matchers read the window themselves.
fn ebnf_reach(window: &str) -> Result<(), usize> {
    let mut chars = window.chars().peekable();
    match chars.next() {
        None => Err(0),
        Some(':') => fixed(window, 3),
        Some('[') => {
            let mut read = 1;
            if chars.peek() == Some(&'^') {
                chars.next();
                read += 1;
            }
            run(chars, read, |c| c != ']' && c != '\n')
        }
        Some('#') => {
            let mut read = 1;
            if chars.peek() == Some(&'x') {
                chars.next();
                read += 1;
            }
            run(chars, read, |c| c.is_ascii_hexdigit())
        }
        Some('(') => match chars.next() {
            None => Err(1),
            Some('*') => match window[2..].find("*)") {
                Some(_) => Ok(()),
                None => Err(window.chars().count()),
            },
            Some(_) => Ok(()),
        },
        Some(_) => Ok(()),
    }
}

/// Whether the unit after `fixed` characters of `window` is in it: a token
/// of `fixed` characters is decided once they are held; and when they are
/// not, the characters read.
fn fixed(window: &str, fixed: usize) -> Result<(), usize> {
    let held = window.chars().take(fixed).count();
    if held == fixed {
        Ok(())
    } else {
        Err(held)
    }
}

/// Whether a character outside `within` follows the run of `chars` that
/// `within` holds, which decides where the run ends; and when none does,
/// the characters read, `read` before `chars` among them.
fn run(
    chars: impl Iterator<Item = char>,
    mut read: usize,
    within: impl Fn(char) -> bool,
) -> Result<(), usize> {
    for c in chars {
        if !within(c) {
            return Ok(());
        }
        read += 1;
    }
    Err(read)
}

/// A lexer matcher that runs before every other one and cuts no token: in a
/// parse from a reader a window at a time, the text after the cursor is the
/// window's, which may end inside a token, so that a match token's
/// expression would match a shorter token, or none, where the whole text
/// has a longer one. It reads the window on, twice as far each time, until
/// the character that decides where the token ends lies inside it (`reach`)
/// or the source has ended, so that every matcher after it sees what the
/// whole text shows it. A whole text is all read.
///
/// The cut it guards is the meta-grammar's own: the ISO comment matcher
/// reads `(*` by position and declines a `(` the window ends at, and the
/// fixed-token matcher then cuts `(`; a class's or a code point's
/// expression likewise settles for what the window shows. Neither can say
/// that it wanted more text, so the engine, which cuts a token again only
/// after a matcher read to the window's end (tabnas/parser#326), cannot
/// know to: this matcher is the fix, not a workaround.
fn reach_matcher(name: &str, reach: fn(&str) -> Result<(), usize>) -> LexMatcher {
    LexMatcher {
        name: name.to_string(),
        order: 900_000.0,
        matcher: None,
        imperative: Some(Arc::new(
            move |lexer: &mut Lexer<'_>, _rule: &mut Rule, _context: &mut Context| {
                if !lexer.windowed() {
                    return None;
                }
                let mut ahead = 1;
                loop {
                    let held = lexer.fill(ahead);
                    match reach(lexer.window()) {
                        Ok(()) => return None,
                        Err(_) if !held => return None,
                        Err(read) => ahead = (read + 1).max(2 * ahead),
                    }
                }
            },
        )),
        factory: None,
    }
}

/// An EBNF document read whole as the tree. A source the meta-grammar
/// refuses, or the tree cannot hold, fails with an [`EbnfParseError`]
/// naming the line and column, its `code` the engine's.
///
/// ```
/// let tree = tabnas_ebnf::parse_ebnf_tree("a ::= [a-z]+\n").unwrap();
/// let element = &tree.to_json()["rules"][0]["alternatives"][0][0];
/// assert_eq!(element["members"][0]["to"], 122.0);
/// assert_eq!(element["repeat"][0]["min"], 1.0);
/// ```
pub fn parse_ebnf_tree(src: &str) -> Result<Value, EbnfParseError> {
    static PARSER: OnceLock<Tabnas> = OnceLock::new();
    let parser = PARSER.get_or_init(make);
    match parser.parse(src) {
        Ok(Value::Undefined) | Ok(Value::Null) => Ok(tree_object(vec![
            ("type", Value::String("grammar".into())),
            ("rules", Value::array(Vec::new())),
        ])),
        Ok(tree) => Ok(tree),
        Err(error) => {
            let line = error.row;
            let column = error.col;
            let location = if 0 != line && 0 != column {
                format!(" at line {line}, column {column}")
            } else {
                String::new()
            };
            let rendered = error.to_string();
            let raw = match rendered.find('\n') {
                Some(at) => &rendered[..at],
                None => rendered.as_str(),
            };
            Err(EbnfParseError {
                message: format!("ebnf: parse error{location}: {raw}"),
                line: (0 != line).then_some(line),
                column: (0 != column).then_some(column),
                code: Some(error.code.clone()),
            })
        }
    }
}
