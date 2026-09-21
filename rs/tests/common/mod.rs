// Shared test helpers. Cargo compiles this module into EVERY integration
// test binary, so an item only one binary uses is dead code in the
// others; the allow keeps that from being a warning rather than hiding
// anything real.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde_json::{Map, Value as JsonValue};
use tabnas::{Options, RewindOptions, Tabnas, Value};
use tabnas_ebnf::{ebnf_convert, EbnfConvertOptions, EbnfError, GrammarSpec};

/// The repository root, one level above this crate.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// One of the `.ebnf` fixtures the TypeScript suite reads.
pub fn fixture(name: &str) -> String {
    let path = repo_root()
        .join("ts")
        .join("test")
        .join("grammar")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Convert EBNF source with the crate's defaults.
pub fn convert(src: &str) -> Result<GrammarSpec, EbnfError> {
    ebnf_convert(src, None)
}

/// Convert with explicit options.
pub fn convert_with(src: &str, opts: &EbnfConvertOptions) -> Result<GrammarSpec, EbnfError> {
    ebnf_convert(src, Some(opts))
}

/// An engine carrying an already-converted spec.
///
/// The rewind history is widened well past the default: a compiled EBNF
/// grammar backs up over whole alternatives, and the realistic fixtures
/// include inputs that need more than the 64 tokens an engine retains by
/// default.
pub fn install(spec: &GrammarSpec) -> Result<Tabnas, EbnfError> {
    let options = Options {
        rewind: RewindOptions {
            history: Some(4096),
        },
        ..Options::default()
    };
    let mut parser = Tabnas::with_options(options);
    spec.install(&mut parser)
        .map_err(|error| EbnfError::Install(error.to_string()))?;
    Ok(parser)
}

/// An engine carrying only the grammar `src` compiles to. Fresh per
/// grammar, never shared: installing applies lexer settings as well as
/// rules, and those are instance-wide.
pub fn engine_for(src: &str) -> Result<Tabnas, EbnfError> {
    let spec = convert(src)?;
    install(&spec)
}

/// Compile `src`, install it, and parse `input`.
pub fn parse_with(src: &str, input: &str) -> Result<Value, String> {
    let parser = engine_for(src).map_err(|error| error.to_string())?;
    parser.parse(input).map_err(|error| error.to_string())
}

/// Whether the grammar accepts the empty string.
pub fn accepts_empty(src: &str) -> bool {
    engine_for(src).expect("compiles").parse("").is_ok()
}

/// The `{rule, src, kids}` tree as JSON.
pub fn tree(value: &Value) -> JsonValue {
    value.to_json()
}

/// The rule names of a spec, sorted.
pub fn rule_names(spec: &GrammarSpec) -> Vec<String> {
    let mut names: Vec<String> = spec.rule.keys().cloned().collect();
    names.sort();
    names
}

/// Drop the OFFSET half of every source span, keeping the row and
/// column.
///
/// A span's units are runtime-native and deliberately unconverted:
/// TypeScript's `s` and `e` count UTF-16 code units, this port's count
/// bytes, which is the divergence `DIVERGENCE.md` records. Row and
/// column agree in both, so a source carrying a non-ASCII character is
/// compared on everything except the two fields that cannot agree.
pub fn strip_offsets(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(strip_offsets).collect()),
        JsonValue::Object(entries) => {
            let span = entries.contains_key("s") && entries.contains_key("e");
            let mut out = Map::new();
            for (key, item) in entries {
                if span && ("s" == key || "e" == key) {
                    continue;
                }
                out.insert(key.clone(), strip_offsets(item));
            }
            JsonValue::Object(out)
        }
        other => other.clone(),
    }
}

/// Normalize an IR value for comparison with the TypeScript oracle.
///
/// TypeScript's IR is a plain object literal with only the keys a
/// front-end set; this port deserializes into a typed struct that also
/// carries the compiler's own fields. Two of them serialize even at
/// their defaults, so they are dropped here rather than written into
/// every oracle entry: `nodeKind`, which is always `user` for a
/// production this front-end builds, and any key whose value is null.
/// Nothing that a front-end sets is removed, so a real difference still
/// shows.
pub fn normalize_ir(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(normalize_ir).collect()),
        JsonValue::Object(entries) => {
            let mut out = Map::new();
            for (key, item) in entries {
                if "nodeKind" == key && JsonValue::String("user".to_string()) == *item {
                    continue;
                }
                if item.is_null() {
                    continue;
                }
                out.insert(key.clone(), normalize_ir(item));
            }
            JsonValue::Object(out)
        }
        other => other.clone(),
    }
}
