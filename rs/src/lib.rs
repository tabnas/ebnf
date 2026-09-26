// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

//! An EBNF grammar front-end for the
//! [`tabnas`](https://github.com/tabnas/parser) parsing engine.
//!
//! The grammar it installs is whatever EBNF text it is handed at run
//! time: hand it the grammar a specification publishes and the engine
//! parses that language.
//!
//! ```text
//! EBNF text ──parse_ebnf──▶ Grammar ──emit_grammar_spec──▶ GrammarSpec
//! ```
//!
//! [`parse_ebnf`] is the front-end and is what this crate adds;
//! everything downstream of the IR lives in
//! [`tabnas_bnf`](https://github.com/tabnas/bnf) and is shared with the
//! ABNF and GBNF front-ends.
//!
//! ```
//! let mut parser = tabnas::Tabnas::new();
//! tabnas_ebnf::ebnf(&mut parser, "Greet ::= \"hi\" | \"hello\"", None).unwrap();
//! let tree = parser.parse("hello").unwrap();
//! assert_eq!(tree.to_json()["rule"], "Greet");
//! ```
//!
//! WHICH EBNF. "EBNF" names a family, not a language. The primary
//! dialect here is W3C EBNF, the notation the XML, XPath and XQuery
//! specifications publish their grammars in, plus the four ISO/IEC
//! 14977 spellings that cannot collide with the W3C reading: `=`, `,`,
//! `;` and `(* … *)`. This is a best-effort front-end, and the README
//! itemises what is and is not supported.
//!
//! This is the Rust port of the canonical TypeScript implementation in
//! `ts/src`; the TypeScript version is authoritative and this crate
//! tracks it.

mod classes;
mod converter;
mod parser_ebnf;

/// The README's Rust examples run as doctests, so a stale one fails the
/// gate rather than misleading the reader. Its `toml`, `text` and `ebnf`
/// fences are skipped; rustdoc runs only the `rust` ones.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

use std::fmt;

use tabnas::{Plugin, PluginError, Tabnas, Value};

pub use converter::{parse_ebnf, EbnfParseError};
pub use parser_ebnf::ebnf_rules;

pub use tabnas_bnf::{
    eliminate_left_recursion, AltSpec, ConvertOptions as EbnfConvertOptions, Element,
    Element as EbnfElement, EmitError, Grammar, Grammar as EbnfGrammar, GrammarSpec, Kind,
    NodeKind, Production, Production as EbnfProduction, RuleSpec, Sequence,
    Sequence as EbnfSequence, SrcSpan,
};

/// This crate's version. It MUST equal `ts/package.json` "version": the
/// release orchestrator rewrites both, and `tests/version_test.rs` fails
/// the build if they drift. Mirrors `VERSION` in `ts/src/ebnf.ts` and
/// `const VERSION` in `go/ebnf.go`.
pub const VERSION: &str = "0.1.10";

/// The group tag stamped on every emitted alt, and the prefix of every
/// diagnostic this crate raises through the shared compiler.
pub const TAG: &str = "ebnf";

/// The EBNF parsed cleanly, but the shared compiler could not compile
/// the resulting IR: an unknown rule reference, a purely left-recursive
/// rule, an ambiguous first set.
///
/// The compiler's message is kept verbatim, because it names the
/// offending rule; only its package prefix is restamped, so a user who
/// wrote EBNF never sees the name of a package they did not import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbnfCompileError {
    /// The rendered diagnostic, prefixed `ebnf: `.
    pub message: String,
    /// The range of the offending grammar text, when the compiler
    /// carried one.
    pub sp: Option<SrcSpan>,
}

impl fmt::Display for EbnfCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EbnfCompileError {}

/// Restamp the shared compiler's package prefix with this front-end's,
/// leaving the rest of the message (which names the offending rule)
/// exactly as the compiler wrote it.
///
/// Both spellings are matched because the compiler's own prefix has
/// moved: `abnf:` while it lived inside `@tabnas/abnf`, `bnf:` since the
/// extraction.
fn restamp(message: &str) -> String {
    for prefix in ["bnf: ", "abnf: "] {
        if let Some(rest) = message.strip_prefix(prefix) {
            return format!("ebnf: {rest}");
        }
    }
    message.to_string()
}

impl From<EmitError> for EbnfCompileError {
    fn from(error: EmitError) -> Self {
        Self {
            message: restamp(&error.to_string()),
            sp: error.sp,
        }
    }
}

/// Anything that can go wrong turning EBNF source into a grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum EbnfError {
    /// The EBNF source itself could not be read.
    Parse(EbnfParseError),
    /// The shared compiler refused the grammar.
    Compile(EbnfCompileError),
    /// The engine refused the emitted grammar.
    Install(String),
}

impl fmt::Display for EbnfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Compile(error) => error.fmt(formatter),
            Self::Install(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EbnfError {}

impl From<EbnfParseError> for EbnfError {
    fn from(error: EbnfParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<EbnfCompileError> for EbnfError {
    fn from(error: EbnfCompileError) -> Self {
        Self::Compile(error)
    }
}

impl From<EmitError> for EbnfError {
    fn from(error: EmitError) -> Self {
        Self::Compile(error.into())
    }
}

/// Emit a spec from an already-parsed EBNF grammar.
///
/// Wraps the shared emitter to keep this crate's `ebnf` tag default,
/// which consumers use to group and inspect the emitted alts. An
/// explicit tag still wins.
pub fn emit_grammar_spec(
    grammar: &Grammar,
    opts: Option<&EbnfConvertOptions>,
) -> Result<GrammarSpec, EbnfCompileError> {
    let mut options = opts.cloned().unwrap_or_default();
    if options.tag.is_none() {
        options.tag = Some(TAG.to_string());
    }
    tabnas_bnf::emit_grammar_spec(grammar, &options).map_err(EbnfCompileError::from)
}

/// Convert EBNF source into a tabnas grammar spec, without installing
/// it. The Rust spelling of `tn.ebnf.toSpec(src)`.
pub fn ebnf_convert(
    src: &str,
    opts: Option<&EbnfConvertOptions>,
) -> Result<GrammarSpec, EbnfError> {
    let grammar = parse_ebnf(src)?;
    Ok(emit_grammar_spec(&grammar, opts)?)
}

/// `ebnf_convert` under the name the canonical package uses for the bare
/// conversion entry point.
pub fn to_spec(src: &str, opts: Option<&EbnfConvertOptions>) -> Result<GrammarSpec, EbnfError> {
    ebnf_convert(src, opts)
}

/// Convert EBNF source and install the resulting grammar on `parser`.
///
/// The Rust spelling of the callable `tn.ebnf(src, opts)` the canonical
/// plugin decorates an instance with. Use a fresh instance per grammar:
/// installing applies lexer settings as well as rules, and those are
/// instance-wide.
pub fn ebnf(
    parser: &mut Tabnas,
    src: &str,
    opts: Option<&EbnfConvertOptions>,
) -> Result<GrammarSpec, EbnfError> {
    let spec = ebnf_convert(src, opts)?;
    spec.install(parser)
        .map_err(|error| EbnfError::Install(error.to_string()))?;
    Ok(spec)
}

/// The plugin descriptor, for [`Tabnas::use_plugin`].
///
/// A grammar this crate installs is whatever EBNF the caller hands over,
/// so the plugin has nothing of its own to install. Pass
/// `{"src": "<ebnf text>"}` in the option bag to have it convert and
/// install that source; otherwise call [`ebnf`] directly, which is the
/// typed way in.
pub fn plugin() -> Plugin {
    Plugin::new("Ebnf", |parser, options| {
        let source = match options {
            Value::Object(entries) => match entries.get("src") {
                Some(Value::String(src)) => Some(src.clone()),
                _ => None,
            },
            _ => None,
        };
        let Some(source) = source else {
            return Ok(());
        };
        ebnf(parser, &source, None)
            .map(|_| ())
            .map_err(|error| PluginError(error.to_string()))
    })
}
