//! Drift guard for the shipped `examples/*.toml` configurations.
//!
//! The examples are the documented entry point (`README.md` § Oracle
//! quickstart) and a second expression of the `Config` schema. Constitution
//! IX forbids a hand-maintained fact in two places without a guard: a field
//! renamed in `config` leaves the examples parsing against the old name, and
//! the failure surfaces to a first-time operator as `CONFIG_INVALID` on the
//! very first command.
//!
//! Discovery is a directory read rather than a literal list, so a new example
//! is covered the moment it lands.

use std::path::PathBuf;

use harness_core::config::Config;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_files() -> Vec<PathBuf> {
    let dir = examples_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files
}

#[test]
fn examples_directory_is_not_empty() {
    assert!(
        !example_files().is_empty(),
        "examples/ holds no configurations — the quickstart in README.md has no starting point"
    );
}

#[test]
fn every_example_loads_and_validates() {
    for path in example_files() {
        if let Err(e) = Config::load_from(&path) {
            panic!(
                "examples/{} does not load: {e}\n\
                 the shipped examples are the documented quickstart — a schema change \
                 must update them in the same commit",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
}
