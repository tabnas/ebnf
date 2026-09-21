// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! The two terminal decoders that are genuinely W3C EBNF: a character
//! class (`[a-z]`, `[^<&]`, `[#x20-#x7E]`) and the standalone code point
//! (`#x41`). Both lower onto the notation-neutral IR that `tabnas-bnf`
//! compiles, and both refuse what Unicode cannot represent.
//!
//! The Rust port of `parseCharClass`, `hexTerm` and `codePoint` in
//! `ts/src/converter.ts`.

use indexmap::IndexMap;
use tabnas::Value;

use crate::converter::{at, EbnfParseError, Loc};

/// The largest code point Unicode has.
const MAX_CODE_POINT: u32 = 0x10_FFFF;

/// The replacement character, written where a Rust `String` cannot hold
/// what the source named.
///
/// `#xD800` names half of a UTF-16 pair. The canonical TypeScript
/// answers a lone surrogate, which is a legal JavaScript string and no
/// legal Rust one; `char::from_u32` refuses it, so the element carries
/// U+FFFD instead. Recorded in `DIVERGENCE.md` and pinned by a test,
/// exactly as the ABNF port records the same boundary.
const REPLACEMENT: char = '\u{FFFD}';

/// Build an object value from named fields, dropping the absent ones.
pub(crate) fn object(fields: Vec<(&str, Option<Value>)>) -> Value {
    let mut map = IndexMap::new();
    for (key, value) in fields {
        if let Some(value) = value {
            map.insert(key.to_string(), value);
        }
    }
    Value::object(map)
}

/// Decode a hex code-point body, refusing anything Unicode cannot
/// represent.
///
/// `hex` is the digits alone, `src` the whole construct as written,
/// because that is what the diagnostic quotes. A body too long for a
/// `u32` is refused by the same ceiling rather than by an overflow: the
/// canonical `parseInt` answers a finite number far above the maximum
/// and fails the same comparison.
pub(crate) fn code_point(hex: &str, src: &str, loc: Loc) -> Result<u32, EbnfParseError> {
    let value = u32::from_str_radix(hex, 16).unwrap_or(u32::MAX);
    if MAX_CODE_POINT < value {
        return Err(EbnfParseError::at(
            format!(
                "ebnf: '{src}'{} is not a Unicode code point (the maximum is #x10FFFF).",
                at(loc)
            ),
            loc,
        ));
    }
    Ok(value)
}

/// The character a code point names, or the replacement character when
/// it names half a surrogate pair. See [`REPLACEMENT`].
pub(crate) fn char_of(value: u32) -> char {
    char::from_u32(value).unwrap_or(REPLACEMENT)
}

/// A standalone `#xNN` code point: `Char ::= #x9 | #xA | #xD`.
///
/// The literal is the single character it denotes; the span covers
/// `#x41` as written, which is what a diagnostic underlines.
pub(crate) fn hex_term(src: &str, span: Option<Value>, loc: Loc) -> Result<Value, EbnfParseError> {
    let value = code_point(&src[2..], src, loc)?;
    Ok(object(vec![
        ("kind", Some(Value::String("term".into()))),
        ("literal", Some(Value::String(char_of(value).to_string()))),
        ("caseSensitive", Some(Value::Bool(true))),
        ("sp", span),
    ]))
}

/// One member of a class, before ranges are folded. A hyphen is kept as
/// a MARKER rather than as its code point, so the fold can tell `a-z`
/// (a range) from `[-a]` (a literal hyphen and an `a`).
enum Item {
    Hyphen,
    Char(u32),
}

/// One member or range of the folded class.
struct Part {
    lo: u32,
    hi: u32,
}

/// Decode a W3C character class into an IR `regex` element.
///
/// ```text
/// [a-z]        => [a-z]
/// [^<&]        => [^<&]
/// [#x20-#x7E]  => the printable ASCII range
/// [#x9#xA#xD]  => tab, newline, or carriage return
/// ```
///
/// Every member is emitted as an explicit escape rather than as its raw
/// character, so nothing inside the class can be re-read as regular
/// expression syntax: a class containing `]`, `^`, `\` or `-` needs no
/// special casing on the way out.
///
/// There are NO escape sequences. W3C EBNF defines none, so a backslash
/// is the backslash character, exactly as it is in a quoted literal.
/// That is also why `]` cannot appear inside a class: the matcher ends
/// the class at the first one, and a literal `]` is written as the
/// string `"]"`.
pub(crate) fn parse_char_class(
    src: &str,
    span: Option<Value>,
    loc: Loc,
) -> Result<Value, EbnfParseError> {
    let source: Vec<char> = src.chars().collect();
    let negated = matches!(source.get(1), Some('^'));
    let body: Vec<char> =
        source[if negated { 2 } else { 1 }..source.len().saturating_sub(1)].to_vec();

    if body.is_empty() {
        return Err(EbnfParseError::at(
            format!(
                "ebnf: empty character class '{src}'{} matches nothing.",
                at(loc)
            ),
            loc,
        ));
    }

    // Members, in source order, read by CODE POINT rather than by UTF-16
    // unit, so an astral character written literally in the class
    // survives as one member.
    let mut items: Vec<Item> = Vec::new();
    let mut index = 0;
    while index < body.len() {
        let current = body[index];

        // `#xNN`, the W3C spelling of a code point inside a class.
        // Lowercase `x` only, matching the standalone `#HX` matcher and
        // the documented dialect: `#X41` is not an accepted spelling.
        if '#' == current && matches!(body.get(index + 1), Some('x')) {
            let mut end = index + 2;
            while end < body.len() && body[end].is_ascii_hexdigit() {
                end += 1;
            }
            if end == index + 2 {
                return Err(EbnfParseError::at(
                    format!(
                        "ebnf: '#x' with no hex digits in character class '{src}'{}.",
                        at(loc)
                    ),
                    loc,
                ));
            }
            let digits: String = body[index + 2..end].iter().collect();
            let whole: String = body[index..end].iter().collect();
            items.push(Item::Char(code_point(&digits, &whole, loc)?));
            index = end;
            continue;
        }

        if '-' == current {
            items.push(Item::Hyphen);
            index += 1;
            continue;
        }

        items.push(Item::Char(current as u32));
        index += 1;
    }

    // Fold `lo - hi` triples into ranges; any other `-` is a literal
    // hyphen, which the W3C grammars rely on for classes like `[-+]`.
    let mut parts: Vec<Part> = Vec::new();
    let mut k = 0;
    while k < items.len() {
        let Item::Char(lo) = items[k] else {
            parts.push(Part { lo: 0x2D, hi: 0x2D });
            k += 1;
            continue;
        };
        if k + 2 < items.len() {
            if let (Item::Hyphen, Item::Char(hi)) = (&items[k + 1], &items[k + 2]) {
                let hi = *hi;
                if hi < lo {
                    return Err(EbnfParseError::at(
                        format!(
                            "ebnf: reversed range in character class '{src}'{} \u{2014} the low \
                             end must not be greater than the high end.",
                            at(loc)
                        ),
                        loc,
                    ));
                }
                parts.push(Part { lo, hi });
                k += 3;
                continue;
            }
        }
        parts.push(Part { lo, hi: lo });
        k += 1;
    }

    // Above the BMP a `\uXXXX` escape is not enough; `\u{…}` is, and in
    // JavaScript it needs the `u` flag, which changes how the whole
    // pattern is read. The class switches over together so the two
    // spellings never mix.
    let astral = parts
        .iter()
        .any(|part| 0xFFFF < part.lo || 0xFFFF < part.hi);
    let escape = |value: u32| -> String {
        if astral {
            format!("\\u{{{value:x}}}")
        } else {
            format!("\\u{value:04x}")
        }
    };

    let mut pattern = String::from("[");
    if negated {
        pattern.push('^');
    }
    for part in &parts {
        pattern.push_str(&escape(part.lo));
        if part.lo != part.hi {
            pattern.push('-');
            pattern.push_str(&escape(part.hi));
        }
    }
    pattern.push(']');

    // Unicode mode follows what the matcher can MATCH, not what was
    // written. A negated class matches the complement of its members,
    // and that complement always contains every astral code point, so
    // `[^<&]` needs `u` just as much as a class with an astral member
    // does. Without it the canonical matcher consumes one UTF-16
    // surrogate rather than one character.
    let flags = if negated || astral { "u" } else { "" };

    Ok(object(vec![
        ("kind", Some(Value::String("regex".into()))),
        ("pattern", Some(Value::String(pattern))),
        ("flags", Some(Value::String(flags.into()))),
        ("sp", span),
    ]))
}
