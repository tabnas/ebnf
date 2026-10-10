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
pub const VERSION: &str = "0.1.15";

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

/// One optional alchemy translation source and its explicit entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationPart {
    /// The definition a host calls after linking the source.
    pub entry: &'static str,
    /// The source text, or `None` for an entry supplied by alchemy.
    pub source: Option<&'static str>,
}

/// The package-local structural translation interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranslationParts {
    /// The complete `tabnas.plugin.json` text.
    pub manifest: &'static str,
    /// An optional lift from the grammar's events to its first read shape.
    pub lift: Option<TranslationPart>,
    /// An optional embedding of a plain tree in the format's schema, with its reverse.
    pub embed: Option<TranslationPart>,
    /// An optional render from the write shape to text.
    pub render: Option<TranslationPart>,
}

const TRANSLATION: TranslationParts = TranslationParts {
    manifest: include_str!("../translate/manifest.json"),
    lift: None,
    embed: None,
    render: Some(TranslationPart {
        entry: "ebnf-render",
        source: Some(include_str!("../translate/render.alc")),
    }),
};

/// Return the translation parts of EBNF documents: the manifest, and the
/// render that writes a grammar spec, the GrammarSpec the compiler emits
/// for a grammar, back as W3C EBNF. There is no lift and no embed: a host
/// reads a document by compiling it (`tabnas_bnf::compile_spec` over
/// [`ebnf_convert`] with `builtins` on, `recognition` false and `strict`
/// true, the text the TypeScript compiler writes byte for byte), and the
/// tree is the spec's own shape, the schema `grammar-spec`, which ABNF and
/// GBNF share.
///
/// ```
/// let parts = tabnas_ebnf::translate().expect("ebnf carries translation parts");
/// assert_eq!(parts.render.map(|part| part.entry), Some("ebnf-render"));
/// assert_eq!(parts.embed, None);
/// ```
#[must_use]
pub const fn translate() -> Option<TranslationParts> {
    Some(TRANSLATION)
}

/// The plugin's manifest, `tabnas.plugin.json`, as the repository carries
/// it. Its `translate` object is what a host that translates reads: the
/// shape a document is read as and written from (`tree`), the schema of
/// that tree (`grammar-spec`), the root the render needs (`object`), the
/// file that holds the render, and the sentences that say what a written
/// grammar does not keep. The crate embeds its own copy,
/// `translate/manifest.json`, since a packaged crate holds nothing outside
/// `rs/`; `tests/translate_test.rs` holds the copy to the file.
///
/// ```
/// assert!(tabnas_ebnf::manifest_text().contains("\"grammar-spec\""));
/// ```
pub fn manifest_text() -> &'static str {
    TRANSLATION.manifest
}

/// The render, `alchemy/render.alc`, the file the manifest's
/// `translate.render` names: a library of alchemy definitions, with no
/// `export`, whose entry point `ebnf-render` writes a grammar spec's
/// events as one W3C EBNF document that compiles back to the same spec. A
/// host links it with its own program. The crate embeds its own copy,
/// `translate/render.alc`, held to the file as the manifest's is.
///
/// ```
/// assert!(tabnas_ebnf::render_text().contains("def ebnf-render [input]"));
/// ```
pub fn render_text() -> &'static str {
    match TRANSLATION.render {
        Some(part) => part.source.unwrap_or_default(),
        None => "",
    }
}
