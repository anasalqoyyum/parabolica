//! Provider crates ship no shared credentials: every token comes from the user's own sign-in
//! through `AuthFlow` and lives in the OS credential store.
//!
//! This scans the source of every provider crate and the desktop app for well-known credential
//! formats and for long opaque string literals that look like keys or tokens.

use std::fs;
use std::path::{Path, PathBuf};

/// Prefixes of Google API keys, OAuth access tokens and OAuth client secrets, and the header
/// forms Discogs and bearer tokens use.
const KNOWN_FORMATS: [&str; 5] = ["AIza", "ya29.", "GOCSPX-", "Discogs token=", "Bearer ey"];

/// An opaque literal at least this long, made only of key characters, looks like a credential.
const OPAQUE_LITERAL_LEN: usize = 32;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// String literals of each line, ignoring comment lines so an apostrophe or quote in prose
/// cannot shift the pairing.
fn string_literals(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| line.split('"').skip(1).step_by(2))
        .collect()
}

fn looks_opaque(literal: &str) -> bool {
    literal.len() >= OPAQUE_LITERAL_LEN
        && literal
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '='))
        && literal.chars().any(|c| c.is_ascii_digit())
        && literal.chars().any(|c| c.is_ascii_alphabetic())
        && !literal.contains("://")
}

#[test]
fn provider_and_app_sources_contain_no_embedded_credentials() {
    let root = workspace_root();
    let mut files = Vec::new();
    for dir in [
        "crates/providers/youtube/src",
        "crates/providers/discogs/src",
        "crates/providers/scrobbling/src",
        "crates/providers/contracts/src",
        "crates/platform/src",
        "apps/desktop/src",
    ] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(
        files.len() > 10,
        "found {} files under {}",
        files.len(),
        root.display()
    );

    let mut findings = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap();
        for format in KNOWN_FORMATS {
            if source.contains(format) {
                findings.push(format!("{}: contains {format:?}", file.display()));
            }
        }
        for literal in string_literals(&source) {
            if looks_opaque(literal) {
                findings.push(format!("{}: opaque literal {literal:?}", file.display()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "credentials must come from the user's sign-in, not the source:\n{}",
        findings.join("\n")
    );
}

#[test]
fn the_scan_detects_credentials() {
    let source = r#"const KEY: &str = "AbCdEf0123456789AbCdEf0123456789xy";"#;
    assert!(string_literals(source).into_iter().any(looks_opaque));
    assert!(!looks_opaque(
        "https://api.discogs.com/database/search?q=0123456789"
    ));
    assert!(!looks_opaque("Signed in. You can close this tab."));
}
