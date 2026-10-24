//! Drift guard for the shipped `examples/*.toml` configurations.
//!
//! The examples are the documented entry point (`README.md` § Oracle
//! quickstart) and a second expression of the `Config` schema. Constitution
//! IX forbids a hand-maintained fact in two places without a guard: a field
//! renamed in `config` leaves the examples parsing against the old name, and
//! the failure surfaces to a first-time operator as `CONFIG_INVALID` on the
//! very first command.
//!
//! Discovery asks git rather than a literal list, so a new example is covered
//! the moment it is added, and a file this machine keeps there untracked is
//! not one.

mod common;

use std::path::PathBuf;

use harness_core::config::Config;

fn example_files() -> Vec<PathBuf> {
    common::tracked(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        "examples",
    )
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
