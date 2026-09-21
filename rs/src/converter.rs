// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! The EBNF front-end: EBNF text in, the notation-neutral grammar IR
//! that `tabnas-bnf` compiles out.
//!
//! ```text
//! EBNF text ──parse_ebnf──▶ Grammar ──bnf::emit_grammar_spec──▶ GrammarSpec
//! ```
//!
//! Everything downstream of that IR (desugaring, left-recursion
//! elimination, tail repeats, probe dispatch, literal lifting, token
//! allocation, first-set analysis, chain emission) lives in
//! `tabnas-bnf` and is shared with the ABNF and GBNF front-ends. What
//! stays here is what is genuinely EBNF: the meta-grammar in
//! [`crate::parser_ebnf`], the character-class and code-point decoders
//! in [`crate::classes`], the named rejections for constructs the IR
//! cannot express, and the two soundness checks below.
//!
//! WHICH EBNF. "EBNF" names a family, not a language. The primary
//! dialect here is W3C EBNF, the notation the XML, XPath and XQuery
//! specifications publish their grammars in, plus the four ISO/IEC
//! 14977 spellings that cannot collide with the W3C reading (`=`, `,`,
//! `;` and `(* … *)`). ISO's `{ A }` repetition and `[ A ]` option are
//! refused by name, because `[ … ]` is a character class here.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value as JsonValue;
use tabnas::Value;
use tabnas_bnf::{Element, Grammar, Kind, Production, Sequence};

use crate::parser_ebnf::{parse_ebnf_raw, RawError};

/// A 1-based line and column, where the offending token carried one.
///
/// The canonical `tokenLoc` answers `{line, column}` read straight off
/// the token, and every `fail*` helper puts both in the message and on
/// the error.
pub(crate) type Loc = Option<(usize, usize)>;

/// The ` at line L, column C` a diagnostic carries when the token that
/// caused it named a position, and nothing when it did not.
pub(crate) fn at(loc: Loc) -> String {
    match loc {
        Some((line, column)) => format!(" at line {line}, column {column}"),
        None => String::new(),
    }
}

/// A failure to read EBNF source: a syntax error, or a construct this
/// front-end deliberately refuses.
///
/// `line` and `column` come from the offending token where one is
/// available, and are the same numbers the message already carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbnfParseError {
    /// The rendered diagnostic, always prefixed `ebnf: `.
    pub message: String,
    /// The 1-based line, when the underlying failure named one.
    pub line: Option<usize>,
    /// The 1-based column, when the underlying failure named one.
    pub column: Option<usize>,
    /// The engine's own error code, when the failure came from the
    /// engine rather than from this front-end.
    pub code: Option<String>,
}

impl EbnfParseError {
    /// A diagnostic with no position.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
            code: None,
        }
    }

    /// A diagnostic reported at a token's position.
    pub(crate) fn at(message: impl Into<String>, loc: Loc) -> Self {
        Self {
            message: message.into(),
            line: loc.map(|(line, _)| line),
            column: loc.map(|(_, column)| column),
            code: None,
        }
    }
}

impl fmt::Display for EbnfParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EbnfParseError {}

// ---- parse ----------------------------------------------------------

/// Parse EBNF source into the grammar IR.
///
/// The order of the checks is part of the contract, and the Go and
/// TypeScript suites pin it: the parse itself first (every named
/// rejection is raised from inside the rule action that read the
/// offending token), then the empty-source refusal, then duplicate
/// productions, then the nullable-alternatives check.
pub fn parse_ebnf(src: &str) -> Result<Grammar, EbnfParseError> {
    let raw = match parse_ebnf_raw(src) {
        Ok(productions) => productions,
        Err(RawError::Rejected(error)) => return Err(error),
        Err(RawError::Message(message)) => return Err(EbnfParseError::new(message)),
        Err(RawError::Engine(error)) => {
            let line = error.row;
            let column = error.col;
            let location = if 0 != line && 0 != column {
                format!(" at line {line}, column {column}")
            } else {
                String::new()
            };
            let rendered = error.to_string();
            let raw = rendered.lines().next().unwrap_or_default();
            return Err(EbnfParseError {
                message: format!("ebnf: parse error{location}: {raw}"),
                line: (0 != line).then_some(line),
                column: (0 != column).then_some(column),
                code: Some(error.code.clone()),
            });
        }
    };

    if raw.is_empty() {
        return Err(EbnfParseError::new("ebnf: no productions found"));
    }

    let mut productions = Vec::with_capacity(raw.len());
    for value in &raw {
        productions.push(production_from_value(value)?);
    }

    check_duplicates(&productions)?;
    check_nullable_alts(&productions)?;

    Ok(Grammar::new(productions))
}

// ---- duplicate productions ------------------------------------------

/// EBNF has no incremental-alternatives operator (ABNF's `=/`), so a
/// repeated symbol is a mistake, and a silent one: the compiler would
/// take whichever definition it saw last.
fn check_duplicates(prods: &[Production]) -> Result<(), EbnfParseError> {
    let mut seen: HashSet<&str> = HashSet::new();
    for production in prods {
        if !seen.insert(production.name.as_str()) {
            return Err(EbnfParseError::new(format!(
                "ebnf: rule '{}' is defined more than once. EBNF has no \
                 incremental-alternatives operator; combine the definitions into one production \
                 with '|' between the alternatives.",
                production.name
            )));
        }
    }
    Ok(())
}

// ---- nullable alternatives ------------------------------------------

/// Whether one element can match nothing, given the rules already known
/// to derive the empty string.
fn el_derives_empty(element: &Element, nullable: &HashSet<String>) -> bool {
    match &element.kind {
        Kind::Opt { .. } | Kind::Star { .. } => true,
        Kind::Plus { inner } => el_derives_empty(inner, nullable),
        // This dialect has no bounded repetition: `{ A }` is refused and
        // `*` is the spelling, so `rep` never reaches here from EBNF
        // source. Kept because the shared IR type carries it.
        Kind::Rep { min, .. } => 0 == *min,
        Kind::Group { alts } => alts.iter().any(|alt| alt_derives_empty(alt, nullable)),
        Kind::Ref { name, .. } => nullable.contains(name),
        // Term, regex, token and prose all consume at least one
        // character; an empty literal is refused before the IR.
        _ => false,
    }
}

fn alt_derives_empty(alt: &Sequence, nullable: &HashSet<String>) -> bool {
    alt.iter()
        .all(|element| el_derives_empty(element, nullable))
}

/// Which rules derive the empty string.
///
/// Least fixed point: a rule is nullable if any alternative is, and that
/// can only become true as more rules are found nullable. One pass is
/// not enough, because a rule's nullability can depend on a rule defined
/// later.
fn nullable_rules(prods: &[Production]) -> HashSet<String> {
    let mut nullable: HashSet<String> = HashSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for production in prods {
            if nullable.contains(&production.name) {
                continue;
            }
            if production
                .alts
                .iter()
                .any(|alt| alt_derives_empty(alt, &nullable))
            {
                nullable.insert(production.name.clone());
                changed = true;
            }
        }
    }
    nullable
}

/// Refuse a production with two or more alternatives that can each match
/// nothing.
///
/// This is the one ambiguity the front-end can rule out soundly and
/// cheaply: if two alternatives both derive the empty string then the
/// GRAMMAR is ambiguous, because no amount of lookahead distinguishes
/// them and there is nothing to look at. The shared compiler emits a
/// dispatch for such a rule anyway and the result mis-parses.
///
/// This is NOT a general ambiguity check. The engine is deterministic
/// with bounded, grammar-declared lookahead plus a probe for one
/// optional-prefix shape, and a grammar that exceeds that fails either
/// in `tabnas-bnf`, with a named error, or at parse time on the inputs
/// that need the extra lookahead.
fn check_nullable_alts(prods: &[Production]) -> Result<(), EbnfParseError> {
    let nullable = nullable_rules(prods);
    let count = |alts: &[Sequence]| -> usize {
        alts.iter()
            .filter(|alt| alt_derives_empty(alt, &nullable))
            .count()
    };
    let complain = |subject: String, n: usize| -> EbnfParseError {
        EbnfParseError::new(format!(
            "ebnf: {subject} has {n} alternatives that each match nothing, so the grammar is \
             ambiguous: an empty input has more than one derivation and no lookahead can choose \
             between them. Make at most one alternative optional \u{2014} '(A | B)?' rather than \
             'A? | B?'."
        ))
    };

    // A group is a choice like any other, so the same ambiguity exists
    // inside one: `A ::= ("x"? | "y"?) "y"` has two epsilon-deriving
    // branches and mis-parses exactly as the production-level shape
    // does. Checking only the production's alternatives let every
    // grouped spelling through.
    fn check_groups(
        element: &Element,
        rule: &str,
        count: &dyn Fn(&[Sequence]) -> usize,
        complain: &dyn Fn(String, usize) -> EbnfParseError,
    ) -> Result<(), EbnfParseError> {
        match &element.kind {
            Kind::Group { alts } => {
                let n = count(alts);
                if 1 < n {
                    return Err(complain(format!("a group in rule '{rule}'"), n));
                }
                for alt in alts {
                    for inner in alt {
                        check_groups(inner, rule, count, complain)?;
                    }
                }
                Ok(())
            }
            Kind::Opt { inner }
            | Kind::Star { inner, .. }
            | Kind::Plus { inner }
            | Kind::Rep { inner, .. } => check_groups(inner, rule, count, complain),
            _ => Ok(()),
        }
    }

    for production in prods {
        let n = count(&production.alts);
        if 1 < n {
            return Err(complain(format!("rule '{}'", production.name), n));
        }
        for alt in &production.alts {
            for element in alt {
                check_groups(element, &production.name, &count, &complain)?;
            }
        }
    }
    Ok(())
}

// ---- value helpers --------------------------------------------------

/// Read one parsed production into the typed IR.
///
/// The parse AST is built as engine values in exactly the shape the IR
/// serializes to, so this is a deserialization rather than a
/// translation, and the two representations cannot drift apart.
fn production_from_value(value: &Value) -> Result<Production, EbnfParseError> {
    let mut json: JsonValue = value.to_json();
    integral(&mut json);
    let name = match value {
        Value::Object(entries) => match entries.get("name") {
            Some(Value::String(name)) => name.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    serde_json::from_value(json).map_err(|error| {
        EbnfParseError::new(format!(
            "ebnf: rule '{name}' is malformed \u{2014} {error}."
        ))
    })
}

/// Rewrite every whole number in a JSON tree as an integer.
///
/// The engine's number is a double, so a span offset arrives as `8.0`,
/// which serde will not read into a `usize`. Nothing in the IR this
/// front-end builds is a fractional number, so the conversion is total
/// rather than a heuristic.
fn integral(value: &mut JsonValue) {
    match value {
        JsonValue::Number(number) => {
            if let Some(float) = number.as_f64() {
                if 0.0 == float.fract() && float.is_finite() && 0.0 <= float {
                    *value = JsonValue::Number((float as u64).into());
                }
            }
        }
        JsonValue::Array(items) => items.iter_mut().for_each(integral),
        JsonValue::Object(entries) => entries.values_mut().for_each(integral),
        _ => {}
    }
}
