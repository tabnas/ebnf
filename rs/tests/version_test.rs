// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// The Rust crate's version is one of the release sites that must agree.
// A bump that updates ts/package.json and forgets these fails here
// rather than shipping a crate whose version disagrees with the package
// it is a port of. Mirrors `ts/test/version.test.js` and
// `go/version_test.go`.

use std::fs;
use std::path::Path;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
}

#[test]
fn version_looks_like_a_semver() {
    let parts: Vec<&str> = tabnas_ebnf::VERSION.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "VERSION is not x.y.z: {}",
        tabnas_ebnf::VERSION
    );
    for part in parts {
        assert!(
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit()),
            "VERSION segment is not numeric: {}",
            tabnas_ebnf::VERSION
        );
    }
}

#[test]
fn version_matches_cargo_toml() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("the manifest is readable");
    let declared = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("the manifest declares a version");
    assert_eq!(
        declared,
        tabnas_ebnf::VERSION,
        "Cargo.toml disagrees with VERSION"
    );
}

#[test]
fn version_matches_package_json() {
    let package = fs::read_to_string(repo_root().join("ts").join("package.json"))
        .expect("ts/package.json is readable");
    let declared: serde_json::Value =
        serde_json::from_str(&package).expect("ts/package.json is JSON");
    assert_eq!(
        declared["version"]
            .as_str()
            .expect("package.json declares a version"),
        tabnas_ebnf::VERSION,
        "ts/package.json disagrees with VERSION"
    );
}

#[test]
fn version_matches_the_go_port() {
    let source =
        fs::read_to_string(repo_root().join("go").join("ebnf.go")).expect("go/ebnf.go is readable");
    let declared = source
        .lines()
        .find_map(|line| line.strip_prefix("const VERSION = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("go/ebnf.go declares a VERSION");
    assert_eq!(
        declared,
        tabnas_ebnf::VERSION,
        "go/ebnf.go disagrees with VERSION"
    );
}

#[test]
fn version_matches_the_typescript_source() {
    let source = fs::read_to_string(repo_root().join("ts").join("src").join("ebnf.ts"))
        .expect("ts/src/ebnf.ts is readable");
    let declared = source
        .lines()
        .find_map(|line| line.strip_prefix("const VERSION = '"))
        .and_then(|rest| rest.split('\'').next())
        .expect("ts/src/ebnf.ts declares a VERSION");
    assert_eq!(
        declared,
        tabnas_ebnf::VERSION,
        "ts/src/ebnf.ts disagrees with VERSION"
    );
}
