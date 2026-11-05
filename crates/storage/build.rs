//! Embeds every `migrations/NNNN_MM_slug.sql` file, sorted by name, so an issue adds a migration
//! by adding its file and never edits a shared list.

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    let migrations_dir = Path::new(&manifest_dir).join("migrations");
    println!("cargo:rerun-if-changed={}", migrations_dir.display());

    let mut names: Vec<String> = fs::read_dir(&migrations_dir)
        .expect("crates/storage/migrations exists")
        .map(|entry| {
            let entry = entry.expect("migration directory entry is readable");
            println!("cargo:rerun-if-changed={}", entry.path().display());
            entry
                .file_name()
                .into_string()
                .expect("migration file names are UTF-8")
        })
        .collect();
    names.sort();

    let mut generated = String::from("&[\n");
    for name in &names {
        assert!(
            is_migration_name(name),
            "migrations/{name} must be named NNNN_MM_slug.sql (issue number, sequence, lowercase slug)"
        );
        let path = migrations_dir.join(name);
        writeln!(
            generated,
            "    crate::migrate::Migration {{ name: {name:?}, sql: include_str!({path:?}) }},",
            path = path.display().to_string()
        )
        .expect("writing to a String cannot fail");
    }
    generated.push(']');

    let out =
        Path::new(&env::var("OUT_DIR").expect("OUT_DIR is set by Cargo")).join("migrations.rs");
    fs::write(out, generated).expect("OUT_DIR is writable");
}

fn is_migration_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sql") else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() > 8
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'_'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'_'
        && bytes[8..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}
