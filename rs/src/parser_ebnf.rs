// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! The EBNF grammar itself, expressed as a tabnas grammar document and
//! installed on a bare engine. The Rust port of the TypeScript
//! `ebnfRules` table plus `getEbnfParser` in `ts/src/converter.ts`.
//!
//! The converter eats its own dog food: EBNF source is read by a tabnas
//! instance whose grammar is the document below, and the parse AST is
//! assembled by the action closures in [`register_refs`].
//!
//! This is the shape the canonical TypeScript takes, and the shape this
//! port follows. The Go port is a hand-written recursive-descent scanner
//! instead, a divergence its own file header states; the IR is the
//! contract, and all three are held to the same accept and reject table.
//!
//! Token vocabulary (mirrors the TypeScript comment):
//!
//! | Token | Means |
//! |---|---|
//! | `#DEF` | `::=`, the W3C definition operator |
//! | `#DEFE` | `=`, the ISO definition operator |
//! | `#DEFOP` | the token set of the two above |
//! | `#ALT` | `|`, alternation |
//! | `#LP` / `#RP` | `(` and `)`, grouping |
//! | `#STAR` / `#PLUS` / `#QM` | `*`, `+` and `?`, postfix repetition |
//! | `#CA` | `,`, ISO concatenation: accepted and ignored |
//! | `#SC` | `;`, the ISO terminator: accepted and ignored |
//! | `#OS` / `#CS` | `[` and `]`, only ever seen when a class is malformed |
//! | `#OB` / `#CB` | `{` and `}`, only ever seen in ISO repetition |
//! | `#CC` | `[…]`, a complete W3C character class |
//! | `#HX` | `#xNN`, a standalone code point |
//! | `#SUB` | `-`, the subtraction operator |
//! | `#CM` | `(* … *)`, an ISO comment, skipped like any other comment |
//! | `#TX` | a bare identifier |
//! | `#ST` | a quoted string literal |
//! | `#ZZ` | end of source |

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use regex::Regex;
use serde_json::Value as JsonValue;
use tabnas::{
    ActionError, Context, MatchToken, MatchTokenMatcher, MatchTokenResult, Options, Rule, Tabnas,
    TabnasError, Token, Value,
};

use crate::classes::{hex_term, object, parse_char_class};
use crate::converter::{at, EbnfParseError, Loc};

/// How deep the rule stack may go while reading one EBNF source.
///
/// A grammar file is untrusted input, the parse AST nests once per
/// group, and both the walks over it and the engine's own value drop are
/// recursive: a Rust stack that runs out ABORTS the process rather than
/// unwinding. So the depth is refused here, before anything that deep is
/// built.
///
/// MEASURED, not guessed. On the unoptimised profile, in one of the 2
/// MiB threads `cargo test` runs a test on, reading a source of 240
/// nested groups succeeds and 250 overflows the stack. The cap is set
/// well inside that, at 129 groups.
///
/// The limit counts RULE levels, which is what the engine hands an
/// action. A group costs four of them (`atom`, `alts`, `seq`, `item`),
/// and the first `(` of a production sits at level six, so the Nth open
/// group is at level `4N + 2`: 520 admits 129 of them and refuses the
/// 130th.
///
/// 129 is deliberately just past 128, which is the shared compiler's own
/// limit on element nesting. A grammar of 128 nested groups is therefore
/// refused BY THE COMPILER, naming the rule and the limit, which is the
/// better diagnostic; this cap only catches what is deeper still. No
/// EBNF an author writes comes near either.
pub(crate) const MAX_GROUP_DEPTH: usize = 520;

/// The diagnostic raised when [`MAX_GROUP_DEPTH`] is exceeded.
pub(crate) const DEPTH_MESSAGE: &str =
    "ebnf: grammar nests too deeply (more than 520 rule levels, about 130 nested groups)";

/// The error code carried by the depth refusal, so the converter can
/// tell it apart from an ordinary engine rejection.
pub(crate) const DEPTH_CODE: &str = "ebnf_group_depth";

/// How deep the IR this front-end builds may NEST.
///
/// [`MAX_GROUP_DEPTH`] bounds the engine's own rule stack while the
/// source is read. This bounds the tree that reading it BUILDS, which
/// is a different resource and the one that actually runs out first: a
/// group nests the IR once per group, and a postfix operator nests it
/// once per operator, so `A ::= "x"` followed by N question marks
/// builds an element nested N deep while costing only N rule levels.
/// Everything downstream of the parse walks that nesting recursively --
/// `Value::to_json`, `integral`, serde's `from_value`, and the default
/// drop of a `Value` -- and a Rust stack that runs out ABORTS the
/// process rather than unwinding.
///
/// MEASURED, not guessed, on the unoptimised profile in one of the 2
/// MiB threads `cargo test` runs a test on. The narrowest of those
/// walks is serde's, and it is reached first:
///
/// | source | element nesting | result |
/// |---|---|---|
/// | `A ::= "x"` and 390 `?` | 391 | parses |
/// | `A ::= "x"` and 400 `?` | 401 | aborts, inside `serde_json::from_value` |
/// | 129 nested groups, one `?` each | 259 | parses |
/// | 129 nested groups, two `?` each | 388 | aborts |
///
/// A group level costs about 1.6 times what a postfix level costs, so
/// the worst case for a given nesting depth is a source of nothing but
/// groups. At the cap that worst case sits about 1.9 times inside the
/// measured abort, which is the margin [`MAX_GROUP_DEPTH`] already
/// carries.
///
/// The number is the SAME boundary [`MAX_GROUP_DEPTH`] draws for
/// groups, written in the units the IR nests in: 129 nested groups
/// around a terminal nest elements 130 deep. A grammar at 128 is still
/// refused BY THE COMPILER, whose `MAX_ELEMENT_DEPTH` is 128 and whose
/// diagnostic names the rule, so this cap only catches what is deeper
/// still -- now for every construct that nests, not only for groups.
pub(crate) const MAX_NEST_DEPTH: usize = 130;

/// The diagnostic raised when [`MAX_NEST_DEPTH`] is exceeded.
pub(crate) const NEST_MESSAGE: &str =
    "ebnf: grammar nests elements more than 130 deep, which is past what this front-end will \
     build. Split the rule into named rules.";

/// The error code carried by the nesting refusal, so the converter can
/// tell it apart from an ordinary engine rejection.
pub(crate) const NEST_CODE: &str = "ebnf_nest_depth";

/// Context key: the nesting depth of the element most recently
/// completed. A terminal is 1, and every group and every postfix
/// operator wrapped around it adds one.
const NEST_KEY: &str = "ebnfNest";

/// Context key: the greatest element depth completed at the CURRENT
/// group level. A group takes one more than this when it closes. Saved
/// and restored by the group's own rule, so the value is stack
/// disciplined without a stack of its own.
const LEVEL_KEY: &str = "ebnfLevelMax";

/// The error code carried out of a named rejection.
///
/// Every construct this dialect refuses is refused from inside the rule
/// action that read the offending token, exactly as the canonical
/// TypeScript throws from inside its `a:` closure. An action here can
/// only answer an `ActionError`, and the engine would wrap that with a
/// position and a code of its own, so the finished diagnostic is put on
/// [`REJECTION`] and this code marks the failure as one to read back
/// from there.
pub(crate) const REJECT_CODE: &str = "ebnf_reject";

thread_local! {
    /// The first named rejection of the parse running on this thread.
    ///
    /// A thread local rather than the parse context's `u` bag because
    /// `Tabnas::parse` hands the context back to nobody: this is the
    /// only channel out. `Tabnas` parses through `&self` and is shared
    /// between threads, so the slot is per thread and the whole
    /// record-and-read pair happens inside one `parse_ebnf_raw` call.
    static REJECTION: RefCell<Option<EbnfParseError>> = const { RefCell::new(None) };
}

/// Record a named rejection and answer the engine error that carries it
/// out of the parse. The FIRST rejection wins: the engine may try
/// further alternatives, and the diagnostic a reader wants is the one
/// for the construct they actually wrote.
fn reject(error: EbnfParseError) -> ActionError {
    let message = error.message.clone();
    REJECTION.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
    });
    ActionError::new(REJECT_CODE, message)
}

fn clear_rejection() {
    REJECTION.with(|slot| *slot.borrow_mut() = None);
}

fn taken_rejection() -> Option<EbnfParseError> {
    REJECTION.with(|slot| slot.borrow_mut().take())
}

/// The EBNF meta-grammar as a tabnas grammar document.
///
/// Every alternative is the one the canonical `ebnfRules` table carries,
/// in the same order. The order is load-bearing twice over: the engine
/// tries alternatives in order, and the `s` patterns decide which token
/// matchers the lexer is offered at each position.
///
/// ```text
/// ebnf ::= prod*
/// prod ::= NAME ('::=' | '=') alts ';'?
/// alts ::= seq ('|' seq)*
/// seq  ::= (','? item)*
/// item ::= atom post          (the canonical table calls this rule `elem`)
/// post ::= ('?' | '*' | '+')*
/// atom ::= NAME | STRING | CHARCLASS | HEX | '(' alts ')'
/// ```
const GRAMMAR_TEXT: &str = r##"
{
  "rule": {
    "ebnf": {
      "open": [
        { "s": "#ZZ", "g": "empty" },
        { "p": "prod" }
      ],
      "close": [
        { "s": "#ZZ" }
      ]
    },

    "prod": {
      "open": [
        { "s": "#TX #DEFOP", "a": "@prod-name", "p": "alts" }
      ],
      "close": [
        { "s": "#SC #TX #DEFOP", "b": 2, "r": "prod" },
        { "s": "#SC" },
        { "s": "#TX #DEFOP", "b": 2, "r": "prod" },
        { "b": 1 }
      ]
    },

    "alts": {
      "open": [
        { "p": "seq" }
      ],
      "close": [
        { "s": "#ALT", "p": "seq" },
        { "b": 1 }
      ]
    },

    "seq": {
      "open": [
        { "s": "#TX #DEFOP", "b": 2, "a": "@fail-empty-alt" },
        { "s": "#ALT", "a": "@fail-empty-alt" },
        { "s": "#RP", "a": "@fail-empty-alt" },
        { "s": "#SC", "a": "@fail-empty-alt" },
        { "s": "#ZZ", "a": "@fail-empty-alt" },
        { "s": "#CA", "a": "@fail-leading-comma" },

        { "s": "#ST", "b": 1, "p": "item" },
        { "s": "#CC", "b": 1, "p": "item" },
        { "s": "#HX", "b": 1, "p": "item" },
        { "s": "#TX", "b": 1, "p": "item" },
        { "s": "#LP", "b": 1, "p": "item" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" },
        { "s": "#CS", "a": "@fail-char-class" },
        { "s": "#STAR", "a": "@fail-dangling-postfix" },
        { "s": "#PLUS", "a": "@fail-dangling-postfix" },

        { "b": 1 }
      ],
      "close": [
        { "s": "#TX #DEFOP", "b": 2, "g": "end" },
        { "s": "#ALT", "b": 1, "g": "end" },
        { "s": "#RP", "b": 1, "g": "end" },
        { "s": "#SC", "b": 1, "g": "end" },
        { "s": "#ZZ", "b": 1, "g": "end" },
        { "s": "#CA", "p": "item" },

        { "s": "#ST", "b": 1, "p": "item" },
        { "s": "#CC", "b": 1, "p": "item" },
        { "s": "#HX", "b": 1, "p": "item" },
        { "s": "#TX", "b": 1, "p": "item" },
        { "s": "#LP", "b": 1, "p": "item" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" },
        { "s": "#CS", "a": "@fail-char-class" },
        { "s": "#STAR", "a": "@fail-dangling-postfix" },
        { "s": "#PLUS", "a": "@fail-dangling-postfix" },

        { "b": 1 }
      ]
    },

    "item": {
      "open": [
        { "p": "atom" }
      ],
      "close": [
        { "c": "@item-first", "a": "@item-take", "p": "post" },
        { "b": 1 }
      ]
    },

    "post": {
      "open": [
        { "s": "#QM", "a": "@post-opt", "p": "post" },
        { "s": "#STAR", "a": "@post-star", "p": "post" },
        { "s": "#PLUS", "a": "@post-plus", "p": "post" },
        { "b": 1, "g": "empty" }
      ],
      "close": [
        { "b": 1 }
      ]
    },

    "atom": {
      "open": [
        { "s": "#ST", "a": "@atom-st" },
        { "s": "#CC", "a": "@atom-cc" },
        { "s": "#HX", "a": "@atom-hx" },
        { "s": "#TX", "a": "@atom-tx" },
        { "s": "#LP", "a": "@atom-lp", "p": "alts" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" }
      ],
      "close": [
        { "s": "#RP", "c": "@atom-group-c", "a": "@atom-group-close" },
        { "c": "@atom-group-c", "a": "@fail-unclosed-group" },
        { "b": 1 }
      ]
    }
  }
}
"##;

/// The EBNF meta-grammar's rule table, as data.
///
/// The Rust spelling of the TypeScript `ebnfRules` export: a fresh
/// document each call, so a caller may take it apart without disturbing
/// the parser this crate builds from the same text.
pub fn ebnf_rules() -> JsonValue {
    let document: JsonValue =
        serde_json::from_str(GRAMMAR_TEXT).expect("the embedded EBNF meta-grammar is valid JSON");
    document
        .get("rule")
        .cloned()
        .expect("the embedded EBNF meta-grammar has a rule map")
}

// ---- node helpers ---------------------------------------------------

/// Replace a rule's node, rebinding the cell rather than writing through
/// it.
///
/// A pushed rule INHERITS its parent's node cell, so writing through the
/// cell would overwrite what the parent is accumulating. This is the
/// Rust spelling of TypeScript's `r.node = …`, which rebinds the
/// property and leaves the parent's own reference alone.
fn set_node(rule: &mut Rule, value: Value) {
    rule.node = Rc::new(RefCell::new(value));
}

/// Append to the array a rule's node holds, through the shared cell, so
/// a parent accumulating into the same array sees it. The TypeScript
/// `r.node.push(…)` on an inherited array.
fn push_node(rule: &Rule, value: Value) {
    if let Some(list) = rule.node.borrow_mut().as_array_mut() {
        list.push(value);
    }
}

// ---- nesting depth --------------------------------------------------
//
// The IR nests once per group and once per postfix operator, and the
// walks over it are recursive, so the depth is tracked AS IT IS BUILT
// and refused at [`MAX_NEST_DEPTH`] before anything deeper exists.
//
// Two registers on the parse context are enough, because the parse is
// depth first and a postfix chain never contains a group:
//
// - [`NEST_KEY`] is the depth of the element that just completed, which
//   at any point an action reads it is the atom the enclosing `item`
//   is about to wrap.
// - [`LEVEL_KEY`] is the greatest depth completed at the current group
//   level, which is what a group takes one more than when it closes.
//   A group rule saves the enclosing level's value in its OWN `u` bag
//   at open and puts it back at close, so the register is stack
//   disciplined without a stack.

/// Read a `usize` register off the parse context, absent meaning zero.
fn register(context: &Context, key: &str) -> usize {
    match context.u.get(key) {
        Some(Value::Number(number)) if 0.0 <= *number => *number as usize,
        _ => 0,
    }
}

/// Write a `usize` register onto the parse context.
fn set_register(context: &mut Context, key: &str, value: usize) {
    context
        .u
        .insert(key.to_string(), Value::Number(value as f64));
}

/// Refuse a source whose IR would nest past [`MAX_NEST_DEPTH`].
fn check_nest(depth: usize) -> Result<(), ActionError> {
    if MAX_NEST_DEPTH < depth {
        return Err(ActionError::new(NEST_CODE, NEST_MESSAGE));
    }
    Ok(())
}

/// One postfix operator, wrapping the atom the enclosing `item` holds
/// one level deeper.
///
/// Checked HERE, as each operator is read, rather than once the chain
/// has closed: a chain is one rule level per operator, so a source of
/// thousands of them would otherwise run the engine's own stack out
/// before any count could be taken.
fn wrap_one_deeper(context: &mut Context) -> Result<(), ActionError> {
    let depth = register(context, NEST_KEY) + 1;
    check_nest(depth)?;
    set_register(context, NEST_KEY, depth);
    Ok(())
}

/// The token a matched open slot holds, cloned so the rule stays
/// borrowable.
fn open_token(rule: &Rule, index: usize) -> Option<Token> {
    rule.o.get(index).cloned()
}

/// The first token matched in a rule's CLOSE phase: the `)` of a group.
/// `None` when the rule closed without matching one, so a span falls
/// back to its opener.
fn close_token(rule: &Rule) -> Option<Token> {
    rule.c0().cloned()
}

/// A matched token's value as a string, resolving a lazy value function
/// the way the canonical `.val` getter does, and falling back to the
/// matched source.
fn token_string(token: &Token, rule: &mut Rule, context: &mut Context) -> String {
    let token = token.clone();
    match token.resolve_val(rule, context) {
        Value::String(text) => text,
        Value::Text(text) => text.string,
        _ => token.src.as_str().to_string(),
    }
}

/// The source span of a token, as the IR's `SrcSpan` shape.
///
/// Every field is copied straight off the token: the compiler stores
/// whatever units the front-end's own engine tokens use, precisely so
/// that no arithmetic, and so no off-by-one, happens at this boundary.
/// This engine counts bytes where the canonical TypeScript counts UTF-16
/// code units, the divergence the engine already records for token
/// positions.
fn span_of(token: Option<&Token>) -> Option<Value> {
    let token = token?;
    Some(object(vec![
        ("s", Some(Value::Number(token.site.si as f64))),
        ("e", Some(Value::Number((token.site.si + token.len) as f64))),
        ("r", Some(Value::Number(token.site.ri as f64))),
        ("c", Some(Value::Number(token.site.ci as f64))),
    ]))
}

/// One named field of an object value, or `None` when there is none.
fn field(value: &Value, key: &str) -> Option<Value> {
    match value {
        Value::Object(entries) => entries.get(key).cloned(),
        _ => None,
    }
}

/// The 1-based line and column of a token, for a diagnostic.
fn loc_of(token: Option<&Token>) -> Loc {
    token.map(|token| (token.site.ri, token.site.ci))
}

/// A production name, as W3C calls a symbol.
///
/// Deliberately narrower than "whatever the text matcher produced": the
/// lexer's bareword token runs to the next delimiter, so without this
/// check a typo like `Foo! ::= …` would become a rule genuinely named
/// `Foo!` and the mistake would only surface much later as an
/// unknown-rule reference. The Rust spelling of `NAME_RE`,
/// `/^[A-Za-z_][A-Za-z0-9_.-]*$/`, written out rather than compiled: the
/// `regex` crate's `$` and JavaScript's agree here, but a character
/// predicate cannot be wrong about it at all.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || '_' == first) {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || '_' == c || '.' == c || '-' == c)
}

/// Validate a symbol name at the point it is read, so the diagnostic can
/// point at the source position.
fn check_name(name: String, loc: Loc) -> Result<String, ActionError> {
    if valid_name(&name) {
        return Ok(name);
    }
    Err(reject(EbnfParseError::at(
        format!(
            "ebnf: '{name}'{} is not a valid symbol name. A name starts with a letter or \
             underscore and continues with letters, digits, '_', '.' or '-'.",
            at(loc)
        ),
        loc,
    )))
}

// ---- named rejections -----------------------------------------------
//
// Every construct this dialect refuses raises an error naming it and its
// position, rather than compiling to the wrong language. The set matches
// ts/README.md's not-supported table exactly, and the wording is the
// canonical wording: `ts/test/ebnf.test.js` asserts that each message
// names the construct or the rule.

/// An alternative with nothing in it. Both dialects require an
/// expression either side of `|`, and this package already refuses an
/// empty literal, so silently reading `A ::= | "x"` as an epsilon
/// alternative made the accepted language wider than the documented one.
fn fail_empty_alternative(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: empty alternative{}. Both dialects require an expression on each side of '|', \
             and this notation has no epsilon terminal; to make a construct optional write 'A?' \
             rather than 'A |'.",
            at(loc)
        ),
        loc,
    ))
}

/// ISO 14977's comma separates concatenated items; it is not a prefix.
fn fail_leading_comma(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: leading comma{}. In ISO 14977 the comma separates concatenated items, so it \
             must appear between two of them \u{2014} 'A , B', not ', A'.",
            at(loc)
        ),
        loc,
    ))
}

/// ISO/IEC 14977 exception (`A - B`) and the same operator in W3C EBNF
/// (`Char - ']'`). The IR has no difference operator, and there is no
/// general way to synthesise one: subtracting an arbitrary language from
/// another is not a regular operation over the `Element` kinds, and
/// faking it with a negated character class only works for the special
/// case where both sides are single characters.
fn fail_subtraction(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: subtraction ('-'){} is not supported. The grammar IR has no difference \
             operator, so 'A - B' cannot be compiled. Where both sides are single characters, \
             write the difference as a negated character class instead \u{2014} '[^abc]' rather \
             than 'Char - [abc]'.",
            at(loc)
        ),
        loc,
    ))
}

/// ISO/IEC 14977 special sequences, `? anything at all ?`, are an escape
/// hatch for prose, deliberately undefined by the standard. There is
/// nothing to compile. This also catches a `?` used as a postfix
/// operator with no element in front of it.
fn fail_special_sequence(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: unexpected '?'{}. A postfix '?' must follow an element, and ISO 14977 special \
             sequences ('? \u{2026} ?') are not supported: their content is undefined by the \
             standard, so there is nothing to compile.",
            at(loc)
        ),
        loc,
    ))
}

/// ISO/IEC 14977 bracket repetition (`{ A }`) and option (`[ A ]`).
/// `[ … ]` is a character class in this dialect, so accepting the ISO
/// reading is impossible; `{ … }` could be accepted in principle, but
/// supporting half of a bracket pair invites grammars that are ISO
/// everywhere except where they silently are not.
fn fail_iso_repetition(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: ISO 14977 bracket repetition '{{ \u{2026} }}'{} is not supported. This \
             front-end reads W3C EBNF, where repetition is postfix: write 'A*' for '{{ A }}' and \
             'A?' for the ISO '[ A ]'. ('[ \u{2026} ]' is a character class here, which is why \
             the ISO reading cannot also be offered.)",
            at(loc)
        ),
        loc,
    ))
}

/// A `[` or `]` that the character-class matcher did not claim. The
/// matcher takes a complete `[…]` in one bite, so a bare bracket means
/// the class never closed on the same line, or that the grammar meant
/// the ISO option bracket.
fn fail_char_class(src: &str, loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: stray '{src}'{}. A character class must open and close on one line, contain no \
             ']' (write a literal ']' as the string \"]\"), and is written '[a-z]', '[^<&]' or \
             '[#x20-#x7E]'. ISO 14977's optional '[ A ]' is not supported \u{2014} write 'A?'.",
            at(loc)
        ),
        loc,
    ))
}

/// `*` or `+` with nothing in front of it. Worth its own message because
/// ABNF, and therefore half the BNF-family muscle memory, puts
/// repetition in front of the element rather than after it.
fn fail_dangling_postfix(src: &str, loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: unexpected '{src}'{}. Repetition in this dialect is postfix \u{2014} 'A*' and \
             'A+', not ABNF's '*A' and '1*A'.",
            at(loc)
        ),
        loc,
    ))
}

fn fail_unclosed_group(loc: Loc) -> ActionError {
    reject(EbnfParseError::at(
        format!(
            "ebnf: unclosed group \u{2014} '(' has no matching ')'{}.",
            at(loc)
        ),
        loc,
    ))
}

// ---- the AST-assembly closures --------------------------------------

/// Register every action, condition and lifecycle hook the meta-grammar
/// names. The reserved `@<rule>-bo` / `@<rule>-bc` names are wired onto
/// their rule's phase by the grammar loader.
fn register_refs(parser: &mut Tabnas) {
    // --- ebnf (top level) ---
    parser.state_action_ref("@ebnf-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });

    // --- prod ---
    parser.action_with_context("@prod-name", |rule, context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let raw = token_string(&token, rule, context);
        let name = check_name(raw, loc_of(Some(&token)))?;
        let span = span_of(Some(&token));
        let bag = rule.u_mut();
        bag.insert("name".into(), Value::String(name));
        bag.insert("nameSp".into(), span.unwrap_or(Value::Undefined));
        Ok(())
    });
    parser.state_action_ref("@prod-bc", |rule, _context| {
        if rule.child_node.is_undefined() {
            return Ok(());
        }
        let alts = rule.child_node.clone();
        let name = match rule.u.get("name") {
            Some(Value::String(name)) => name.clone(),
            _ => String::new(),
        };
        // The production's span is its NAME. A rule's body can run over
        // many lines, and every consumer of this span (an outline entry,
        // go-to-definition, the underline on a whole-rule diagnostic like
        // "purely left-recursive") wants the name, not the paragraph.
        // Element spans already locate everything inside.
        let span = match rule.u.get("nameSp") {
            Some(value) if !value.is_undefined() => Some(value.clone()),
            _ => None,
        };
        push_node(
            rule,
            object(vec![
                ("name", Some(Value::String(name))),
                ("alts", Some(alts)),
                ("sp", span),
            ]),
        );
        Ok(())
    });

    // --- alts ---
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

    // --- seq ---
    parser.state_action_ref("@seq-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });

    // --- item ---
    //
    // The canonical table calls this rule `elem`. That name is not
    // available here: `@elem-bc` is one of the engine's own builtin
    // action references, and a builtin wins the lookup, so a rule named
    // `elem` would silently run the engine's list-element push instead
    // of the closure below. The TypeScript front-end hands the engine
    // closures rather than named references, so the collision cannot
    // arise there. Only the rule NAME differs; the IR is identical.
    //
    // The `c:` guard is what makes the two-child sequence work: a close
    // state is re-entered every time a child returns, so without it the
    // `post` push would repeat forever.
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
    parser.state_action_ref("@item-bc", |rule, context| {
        if !matches!(rule.u.get("got"), Some(Value::Bool(true))) {
            return Ok(());
        }
        // The postfix chain has closed, so the register now holds this
        // element's own depth. A group one level out takes one more than
        // the deepest element at this level.
        let depth = register(context, NEST_KEY);
        if register(context, LEVEL_KEY) < depth {
            set_register(context, LEVEL_KEY, depth);
        }
        let Some(atom) = rule.u.get("atom").cloned().filter(|a| !a.is_undefined()) else {
            return Ok(());
        };
        let ops = match &rule.child_node {
            Value::Array(items) => items.as_ref().clone(),
            _ => Vec::new(),
        };
        let mut element = atom;
        for op in ops {
            let Value::String(kind) = op else { continue };
            element = object(vec![
                ("kind", Some(Value::String(kind))),
                ("inner", Some(element)),
            ]);
        }
        push_node(rule, element);
        Ok(())
    });

    // --- post ---
    //
    // Zero or more postfix operators, innermost first. Tail-recurses
    // through `p:` so a stacked `(A)*?` is read left to right; W3C EBNF
    // never stacks them, but reading a stack is strictly more permissive
    // than failing on one.
    parser.state_action_ref("@post-bo", |rule, _context| {
        set_node(rule, Value::array(Vec::new()));
        Ok(())
    });
    parser.action_with_context("@post-opt", |rule, context| {
        wrap_one_deeper(context)?;
        push_node(rule, Value::String("opt".into()));
        Ok(())
    });
    parser.action_with_context("@post-star", |rule, context| {
        wrap_one_deeper(context)?;
        push_node(rule, Value::String("star".into()));
        Ok(())
    });
    parser.action_with_context("@post-plus", |rule, context| {
        wrap_one_deeper(context)?;
        push_node(rule, Value::String("plus".into()));
        Ok(())
    });
    parser.state_action_ref("@post-bc", |rule, _context| {
        let ops = match &rule.child_node {
            Value::Array(items) => items.as_ref().clone(),
            _ => Vec::new(),
        };
        for op in ops {
            push_node(rule, op);
        }
        Ok(())
    });

    // --- atom ---
    parser.state_action_ref("@atom-bo", |rule, context| {
        set_node(rule, Value::Undefined);
        // Every atom is a terminal until `@atom-lp` says otherwise, and
        // a terminal nests one deep. `@atom-group-close` overwrites this
        // with the group's own depth.
        set_register(context, NEST_KEY, 1);
        let bag = rule.u_mut();
        bag.insert("group".into(), Value::Bool(false));
        Ok(())
    });
    // A quoted terminal. W3C EBNF literals are case-SENSITIVE (unlike
    // RFC 5234's, which are not), so the flag is set explicitly rather
    // than left to the IR default. Single and double quotes are
    // interchangeable: the spec grammars use both, and the choice is
    // only ever about which quote character the literal itself contains.
    parser.action_with_context("@atom-st", |rule, context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let literal = token_string(&token, rule, context);
        let loc = loc_of(Some(&token));
        if literal.is_empty() {
            return Err(reject(EbnfParseError::at(
                format!(
                    "ebnf: empty string literal{} matches nothing. W3C EBNF has no epsilon \
                     terminal; to make a construct optional write 'A?'.",
                    at(loc)
                ),
                loc,
            )));
        }
        let span = span_of(Some(&token));
        set_node(
            rule,
            object(vec![
                ("kind", Some(Value::String("term".into()))),
                ("literal", Some(Value::String(literal))),
                ("caseSensitive", Some(Value::Bool(true))),
                ("sp", span),
            ]),
        );
        Ok(())
    });
    parser.action_with_context("@atom-cc", |rule, _context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let loc = loc_of(Some(&token));
        let span = span_of(Some(&token));
        let element = parse_char_class(token.src.as_str(), span, loc).map_err(reject)?;
        set_node(rule, element);
        Ok(())
    });
    parser.action_with_context("@atom-hx", |rule, _context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let loc = loc_of(Some(&token));
        let span = span_of(Some(&token));
        let element = hex_term(token.src.as_str(), span, loc).map_err(reject)?;
        set_node(rule, element);
        Ok(())
    });
    parser.action_with_context("@atom-tx", |rule, context| {
        let Some(token) = open_token(rule, 0) else {
            return Ok(());
        };
        let raw = token_string(&token, rule, context);
        let name = check_name(raw, loc_of(Some(&token)))?;
        let span = span_of(Some(&token));
        set_node(
            rule,
            object(vec![
                ("kind", Some(Value::String("ref".into()))),
                ("name", Some(Value::String(name))),
                ("sp", span),
            ]),
        );
        Ok(())
    });
    parser.action_with_context("@atom-lp", |rule, context| {
        if MAX_GROUP_DEPTH < rule.d {
            return Err(ActionError::new(DEPTH_CODE, DEPTH_MESSAGE));
        }
        // This group becomes the current nesting level: remember the
        // enclosing level's running maximum on THIS rule, so the close
        // can put it back, and start counting the group's contents from
        // nothing.
        let outer = register(context, LEVEL_KEY);
        set_register(context, LEVEL_KEY, 0);
        let span = open_token(rule, 0).and_then(|token| span_of(Some(&token)));
        let loc = loc_of(rule.o.first());
        let bag = rule.u_mut();
        bag.insert("outerMax".into(), Value::Number(outer as f64));
        bag.insert("group".into(), Value::Bool(true));
        bag.insert("lp".into(), span.unwrap_or(Value::Undefined));
        bag.insert(
            "lpLine".into(),
            loc.map_or(Value::Undefined, |(line, _)| Value::Number(line as f64)),
        );
        bag.insert(
            "lpCol".into(),
            loc.map_or(Value::Undefined, |(_, column)| Value::Number(column as f64)),
        );
        Ok(())
    });
    parser.alt_condition("@atom-group-c", |rule, _context| {
        matches!(rule.u.get("group"), Some(Value::Bool(true)))
    });
    parser.action_with_context("@atom-group-close", |rule, context| {
        // The group nests one deeper than the deepest element it holds.
        // Put the enclosing level's running maximum back before leaving.
        let inner = register(context, LEVEL_KEY);
        let outer = match rule.u.get("outerMax") {
            Some(Value::Number(number)) if 0.0 <= *number => *number as usize,
            _ => 0,
        };
        set_register(context, LEVEL_KEY, outer);
        check_nest(inner + 1)?;
        set_register(context, NEST_KEY, inner + 1);
        let alts = rule.child_node.clone();
        let open = rule
            .u
            .get("lp")
            .cloned()
            .filter(|value| !value.is_undefined());
        // The opening paren through the closing one; the elements inside
        // carry their own, narrower spans.
        let span = group_span(rule, open.as_ref());
        set_node(
            rule,
            object(vec![
                ("kind", Some(Value::String("group".into()))),
                ("alts", Some(alts)),
                ("sp", span),
            ]),
        );
        Ok(())
    });
    // A group that never closed: report it here, where the opening `(`
    // is still the thing being parsed, rather than letting the failure
    // surface as a stray token further up.
    parser.action_with_context("@fail-unclosed-group", |rule, _context| {
        Err(fail_unclosed_group(remembered_loc(rule)))
    });

    // --- named rejections ---
    parser.action_with_context("@fail-empty-alt", |rule, _context| {
        Err(fail_empty_alternative(loc_of(rule.o.first())))
    });
    parser.action_with_context("@fail-leading-comma", |rule, _context| {
        Err(fail_leading_comma(loc_of(rule.o.first())))
    });
    parser.action_with_context("@fail-subtraction", |rule, _context| {
        Err(fail_subtraction(loc_of(rule.o.first())))
    });
    parser.action_with_context("@fail-special-sequence", |rule, _context| {
        Err(fail_special_sequence(loc_of(rule.o.first())))
    });
    parser.action_with_context("@fail-iso-repetition", |rule, _context| {
        Err(fail_iso_repetition(loc_of(rule.o.first())))
    });
    parser.action_with_context("@fail-char-class", |rule, _context| {
        let token = rule.o.first().cloned();
        let source = token
            .as_ref()
            .map_or_else(String::new, |token| token.src.as_str().to_string());
        Err(fail_char_class(&source, loc_of(token.as_ref())))
    });
    parser.action_with_context("@fail-dangling-postfix", |rule, _context| {
        let token = rule.o.first().cloned();
        let source = token
            .as_ref()
            .map_or_else(String::new, |token| token.src.as_str().to_string());
        Err(fail_dangling_postfix(&source, loc_of(token.as_ref())))
    });
}

/// The line and column of the `(` this atom opened with, as
/// `@atom-lp` remembered them.
///
/// The close phase has its own matched tokens, so `rule.o` no longer
/// holds the opener by the time the unclosed-group refusal runs; the
/// canonical `r.o[0]` still does, because a JavaScript rule keeps one
/// list. Reading the remembered pair is what makes the two diagnostics
/// name the same position.
fn remembered_loc(rule: &Rule) -> Loc {
    let line = match rule.u.get("lpLine") {
        Some(Value::Number(line)) => *line as usize,
        _ => return None,
    };
    let column = match rule.u.get("lpCol") {
        Some(Value::Number(column)) => *column as usize,
        _ => return None,
    };
    Some((line, column))
}

/// The span from a remembered opening bracket to the closing one.
fn group_span(rule: &Rule, open: Option<&Value>) -> Option<Value> {
    let close = close_token(rule).and_then(|token| span_of(Some(&token)));
    match (open, close) {
        (Some(open), Some(close)) => Some(object(vec![
            ("s", field(open, "s")),
            ("e", field(&close, "e")),
            ("r", field(open, "r")),
            ("c", field(open, "c")),
        ])),
        (Some(open), None) => Some(open.clone()),
        (None, close) => close,
    }
}

// ---- the parser instance --------------------------------------------

/// The engine options the EBNF meta-grammar needs.
fn ebnf_parser_options() -> Options {
    let mut options = Options::default();

    // `:` is not an operator on its own (`::=` is matched whole), so
    // leave colons free to appear inside names. `[` `]` `{` `}` and `,`
    // keep the engine's own spellings: a well-formed class is claimed by
    // `#CC` before the fixed matcher is reached, and the brackets stay
    // declared so the text matcher treats them as delimiters. Without
    // that, `A[a-z]` would lex as one long bareword.
    options.fixed.tokens.shift_remove("#CL");

    // A W3C symbol name is an ordinary word, and `true`, `false` and
    // `null` are ordinary names: several published grammars define rules
    // with exactly those names. With the engine's default keyword-value
    // lexing they would arrive as `#VL` value tokens instead of `#TX`
    // barewords and no such grammar would compile. This switch affects
    // the parser that READS EBNF, never the grammars it emits, where
    // `VL` remains a built-in token name.
    options.value.lex = false;

    // EBNF has no numeric literals. Digits only ever appear inside names
    // (`Char32`), code points (`#x41`) and character classes, all of
    // which are claimed by a matcher or by the text matcher.
    options.number.lex = false;

    // W3C EBNF quotes are `'` and `"`, interchangeable. Backticks are
    // not EBNF, and no literal spans lines.
    options.string.chars = "'\"".to_string();
    options.string.multi_chars = String::new();

    // W3C EBNF defines NO escape sequences inside a literal: a backslash
    // is the backslash character. The engine has no shared "escaping
    // off" switch, so point the escape character at DEL (%x7F), which no
    // EBNF literal contains. Without this, `"\"` (a one-character
    // literal, and the one every grammar for a language with escapes
    // needs) would swallow its closing quote.
    options.string.escape_char = '\u{7F}';

    // `#` starts a code point, not a comment, and `//` is not an EBNF
    // comment: leaving it on would make a grammar that uses `/` as a
    // terminal harder to read than it needs to be. The engine's `multi`
    // definition is already W3C EBNF's `/* … */`.
    options.comment.definitions.shift_remove("hash");
    options.comment.definitions.shift_remove("slash");

    options.rule.start = "ebnf".to_string();
    options
}

/// ISO/IEC 14977 comments, `(* … *)`.
///
/// These cannot be declared as an ordinary comment definition: the
/// fixed-token matcher runs after the match matcher but before the
/// comment matcher, so `(` is already a `#LP` by the time a comment
/// definition would be offered the position, and `(*` never survives to
/// be recognised. A match-token matcher runs FIRST of all, so the
/// comment is claimed here instead and emitted as an ordinary `#CM`,
/// which the parser skips like any other comment.
///
/// Unlike the canonical TypeScript, the row and column bookkeeping is
/// the engine's: this lexer advances one character at a time over the
/// consumed source and counts embedded newlines itself, so a multi-line
/// comment does not shift the line every later diagnostic reports.
fn iso_comment(rest: &str) -> Option<MatchTokenResult> {
    if !rest.starts_with("(*") {
        return None;
    }
    // Unterminated: leave it to `(` plus `*`, exactly as the canonical
    // matcher does, so the failure is reported against the group.
    let end = rest[2..].find("*)")? + 2 + 2;
    let text = &rest[..end];
    Some(MatchTokenResult::new(text, Value::String(text.to_string())))
}

/// Build the EBNF parser instance: a bare engine carrying only the
/// meta-grammar above.
fn build_ebnf_parser() -> Result<Tabnas, String> {
    let mut parser = Tabnas::with_options(ebnf_parser_options());

    // Fixed tokens the notation adds. `#OS` `#CS` `#OB` `#CB` `#CA` are
    // already the engine's, spelled the same way.
    let _semicolon = parser.token_with_source("#SC", ";");
    let _alternation = parser.token_with_source("#ALT", "|");
    let _star = parser.token_with_source("#STAR", "*");
    let _plus = parser.token_with_source("#PLUS", "+");
    let _question = parser.token_with_source("#QM", "?");
    let _open_paren = parser.token_with_source("#LP", "(");
    let _close_paren = parser.token_with_source("#RP", ")");

    // Token identities for the matchers, minted before the grammar names
    // them.
    let define = parser.token("#DEF");
    let define_iso = parser.token("#DEFE");
    let char_class = parser.token("#CC");
    let hex = parser.token("#HX");
    let subtract = parser.token("#SUB");
    let comment = parser.token("#CM");

    // Every match-token matcher in this grammar is EAGER: it fires
    // wherever its pattern matches, rather than only where the current
    // rule's token column already expects it. That is safe here, and it
    // is what keeps the rule table honest, because each pattern starts
    // with a character that has exactly one meaning in EBNF: `[` only
    // opens a character class, `#` only opens a code point (the `#` line
    // comment is off), `::=` and `=` only define a production, and `-`
    // only subtracts, since a hyphen INSIDE a name is consumed by the
    // text matcher as part of that name and never reaches a position of
    // its own.
    let patterns: [(&str, tabnas::Tin, &str); 5] = [
        // `::=` (W3C) and `=` (ISO). Fixed tokens would do the same job,
        // but as matchers the two spellings share one `#DEFOP` set and
        // the rule table needs only one lookahead pattern.
        ("#DEF", define, "^::="),
        ("#DEFE", define_iso, "^="),
        // A complete character class, bounded to one line so an unclosed
        // `[` fails at the bracket instead of swallowing the rest of the
        // grammar.
        ("#CC", char_class, r"^\[\^?[^\]\n]*\]"),
        // A standalone code point: `#x41`, `#xD7FF`. The `x` is
        // lowercase, which is what W3C specifies; `#X41` is not among
        // the four ISO spellings this package documents accepting, and
        // case-folding it would make the accepted language quietly wider
        // than the supported table says. The hex digits themselves may
        // be either case. Spelled out rather than written `\d`, because
        // the `regex` crate reads that class as Unicode-aware where the
        // JavaScript pattern it is ported from is ASCII.
        ("#HX", hex, "^#x[0-9a-fA-F]+"),
        // The subtraction operator. Only reachable when `-` starts a
        // token; inside a name the text matcher has already eaten it.
        ("#SUB", subtract, "^-"),
    ];
    for (name, tin, pattern) in patterns {
        let regex = Regex::new(pattern).map_err(|error| format!("ebnf: {name}: {error}"))?;
        insert_match(&mut parser, name, tin, MatchTokenMatcher::Regex(regex));
    }
    insert_match(
        &mut parser,
        "#CM",
        comment,
        MatchTokenMatcher::Callback(Arc::new(iso_comment)),
    );

    // The two definition operators, so `NAME ::=` and `NAME =`
    // production headers share one lookahead pattern.
    parser
        .options
        .token_set
        .insert("DEFOP".to_string(), vec![define, define_iso]);

    // Drop the default rules: they would compete with the meta-grammar
    // for the starting token set.
    for name in parser.rule_names() {
        parser.remove_rule(&name);
    }

    register_refs(&mut parser);
    parser
        .grammar_json(GRAMMAR_TEXT)
        .map_err(|error| error.to_string())?;
    Ok(parser)
}

/// Install one match-token matcher, eagerly.
fn insert_match(parser: &mut Tabnas, name: &str, tin: tabnas::Tin, matcher: MatchTokenMatcher) {
    parser.options.match_tokens.insert(
        name.to_string(),
        MatchToken {
            name: name.to_string(),
            tin,
            matcher,
            eager: true,
        },
    );
}

/// The cached EBNF parser instance, built once.
///
/// `Tabnas` parses through `&self` and is `Send + Sync`, so one instance
/// serves every caller and every thread; per-parse state lives on the
/// rules and the context.
fn ebnf_parser() -> Result<&'static Tabnas, String> {
    static PARSER: OnceLock<Result<Tabnas, String>> = OnceLock::new();
    PARSER
        .get_or_init(build_ebnf_parser)
        .as_ref()
        .map_err(String::clone)
}

/// How a raw EBNF parse failed.
pub(crate) enum RawError {
    /// A construct this front-end refuses by name.
    Rejected(EbnfParseError),
    /// The engine rejected the source.
    Engine(Box<TabnasError>),
    /// The parser itself could not be built, or the source nests past
    /// [`MAX_GROUP_DEPTH`] or [`MAX_NEST_DEPTH`].
    Message(String),
}

/// Run the meta-grammar over `src` and return the raw production list.
pub(crate) fn parse_ebnf_raw(src: &str) -> Result<Vec<Value>, RawError> {
    let parser = ebnf_parser().map_err(RawError::Message)?;
    clear_rejection();
    let parsed = parser.parse(src);
    let rejection = taken_rejection();
    match parsed {
        Ok(value) => Ok(match value {
            Value::Array(items) => items.as_ref().clone(),
            Value::Undefined | Value::Null => Vec::new(),
            other => vec![other],
        }),
        Err(error) if REJECT_CODE == error.code => Err(match rejection {
            Some(rejection) => RawError::Rejected(rejection),
            // Unreachable in practice: the code is only ever set beside
            // a recorded diagnostic. Answering the engine's own text
            // keeps the failure a failure rather than a panic.
            None => RawError::Message(error.to_string()),
        }),
        Err(error) if DEPTH_CODE == error.code => Err(RawError::Message(DEPTH_MESSAGE.to_string())),
        Err(error) if NEST_CODE == error.code => Err(RawError::Message(NEST_MESSAGE.to_string())),
        Err(error) => Err(RawError::Engine(Box::new(error))),
    }
}
