// The translation parts: what the manifest says and what the crate
// embeds are the same files, and the tree a host translates is the
// grammar spec the compiler writes.
//
// A packaged crate holds nothing outside `rs/`, so the crate embeds its
// own copies, `rs/translate/manifest.json` of `tabnas.plugin.json` and
// `rs/translate/render.alc` of the render the manifest names, as
// `manifest_text()` and `render_text()`. The copies are the only texts a
// host sees, so they must be the files: this holds the embedded manifest
// to the repository's, and the render the manifest names, read from the
// repository, to the embedded one, as it would an embed the manifest
// named. Change the file at the root and run `npm run embed` in `ts/`,
// which copies it into `rs/translate/`; this fails until both are the
// same.

mod common;

use std::fs;

use serde_json::Value;
use tabnas_bnf::{compile_spec, CompileOptions, ConvertOptions};
use tabnas_ebnf::ebnf_convert;

fn translate() -> Value {
    let manifest: Value =
        serde_json::from_str(tabnas_ebnf::manifest_text()).expect("the manifest is JSON");
    manifest
        .get("translate")
        .cloned()
        .expect("the manifest carries a translate object")
}

#[test]
fn the_manifest_the_crate_embeds_is_the_repositorys() {
    let on_disk = fs::read_to_string(common::repo_root().join("tabnas.plugin.json"))
        .expect("the repository has its manifest");
    assert_eq!(
        on_disk,
        tabnas_ebnf::manifest_text(),
        "rs/translate/manifest.json is not tabnas.plugin.json: run npm run embed in ts"
    );
}

#[test]
fn the_render_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let path = translate["render"]
        .as_str()
        .expect("translate.render names a file");
    let on_disk = fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.render names {path}, which cannot be read: {e}"));
    assert_eq!(
        on_disk,
        tabnas_ebnf::render_text(),
        "translate.render names {path}, and rs/translate/render.alc, which render_text() \
         embeds, is another text: run npm run embed in ts"
    );
}

/// An embed takes a plain tree into a format's own schema. A grammar spec
/// is no plain tree's shape, so the manifest names no embed and the crate
/// carries none; a manifest that named one would be held to its file
/// here, as the render is above.
#[test]
fn the_embed_the_manifest_names_is_the_one_the_crate_embeds() {
    let translate = translate();
    let parts = tabnas_ebnf::translate().expect("ebnf carries translation parts");
    let Some(path) = translate.get("embed").and_then(Value::as_str) else {
        assert_eq!(
            parts.embed, None,
            "the manifest names no embed, and the crate carries one"
        );
        return;
    };
    let on_disk = fs::read_to_string(common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("translate.embed names {path}, which cannot be read: {e}"));
    let embed = parts
        .embed
        .unwrap_or_else(|| panic!("translate.embed names {path}, and the crate carries no embed"));
    assert_eq!(embed.entry, "ebnf-embed");
    assert_eq!(
        embed.source,
        Some(on_disk.as_str()),
        "translate.embed names {path}, and the crate embeds another text: run npm run embed in ts"
    );
}

#[test]
fn the_structural_interface_names_the_render_entry() {
    let parts = tabnas_ebnf::translate().expect("ebnf carries translation parts");
    assert_eq!(parts.manifest, tabnas_ebnf::manifest_text());
    assert_eq!(parts.lift, None);
    let render = parts.render.expect("ebnf carries a render");
    assert_eq!(render.entry, "ebnf-render");
    assert_eq!(render.source, Some(tabnas_ebnf::render_text()));
}

/// An EBNF document is read as a tree and written from one, the grammar
/// spec's own shape, which the manifest names as its schema; reading a
/// document is compiling it, which is the host's to do, so there is no
/// lift, and the render needs the spec, an object, at the root.
#[test]
fn ebnf_reads_and_writes_its_grammar_spec_with_no_lift() {
    let manifest: Value =
        serde_json::from_str(tabnas_ebnf::manifest_text()).expect("the manifest is JSON");
    assert_eq!(manifest["languageId"], "ebnf");
    let translate = translate();
    assert_eq!(translate["reads"], "tree");
    assert_eq!(translate["writes"], "tree");
    assert_eq!(translate["root"], "object");
    assert_eq!(translate["schema"], "grammar-spec");
    assert_eq!(translate.get("lift"), None);
}

/// The host prints the loss lines verbatim, so each is a sentence.
#[test]
fn the_loss_is_a_list_of_sentences() {
    let translate = translate();
    let loss = translate["loss"]
        .as_array()
        .expect("translate.loss is a list");
    assert!(!loss.is_empty());
    for line in loss {
        let line = line.as_str().expect("each loss line is a string");
        assert!(
            line.starts_with(char::is_uppercase) && line.ends_with('.'),
            "{line:?} is not a sentence"
        );
    }
}

/// A host links the render with its own program and other formats'
/// parts, so every definition is named for EBNF, the entry point is
/// `ebnf-render`, and the file defines no `export` of its own.
#[test]
fn the_render_is_a_library_named_for_ebnf() {
    let names: Vec<&str> = tabnas_ebnf::render_text()
        .lines()
        .filter_map(|line| line.strip_prefix("def "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    assert!(names.contains(&"ebnf-render"), "{names:?}");
    for name in &names {
        assert!(name.starts_with("ebnf-"), "{name} is not named for ebnf");
    }
}

/// What a host reads an EBNF document as: the pure-data grammar spec
/// `compile_spec` writes over [`ebnf_convert`] with `builtins` on,
/// `recognition` false and `strict` true, valid JSON whose root is an
/// object holding the rules and the options.
#[test]
fn a_host_reads_a_document_as_the_strict_pure_data_grammar_spec() {
    let spec = ebnf_convert(
        "Doc ::= Greet\nGreet ::= \"hi\" | \"hello\"",
        Some(&ConvertOptions {
            builtins: true,
            ..ConvertOptions::default()
        }),
    )
    .expect("the grammar converts");
    let text = compile_spec(
        &spec,
        CompileOptions {
            recognition: false,
            strict: true,
            indent: None,
        },
    )
    .expect("the grammar compiles");
    let tree: Value = serde_json::from_str(&text).expect("the grammar spec is JSON");
    let rules: Vec<&String> = tree["rule"]
        .as_object()
        .expect("the spec holds its rules")
        .keys()
        .collect();
    assert_eq!(rules, ["Doc", "Greet", "__start__"]);
    assert_eq!(tree["options"]["rule"]["start"], "__start__");
    assert_eq!(tree["meta"]["provenance"]["__start__"], "Doc");
}
