//! Every shell file harnex ships parses under every bash it runs on.
//!
//! macOS keeps `/bin/bash` at 3.2, and a Homebrew or Linux bash on `PATH` is
//! 5.x. A construct the newer one accepts and 3.2 refuses — a `case` pattern
//! inside a process substitution — passes a check run with the bash on `PATH`
//! alone and fails every commit where 3.2 answers.

mod common;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn is_bash_script(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| {
        text.lines()
            .next()
            .is_some_and(|line| line.starts_with("#!") && line.contains("bash"))
    })
}

#[test]
fn every_shipped_bash_script_parses_under_every_bash() {
    let root = repo_root();
    let scripts: Vec<PathBuf> = ["plugins/harnex/templates", "hooks", "scripts"]
        .into_iter()
        .flat_map(|dir| common::tracked(&root, dir))
        .filter(|path| is_bash_script(path))
        .collect();
    assert!(
        scripts.len() > 10,
        "the shipped scripts were not found: {scripts:?}"
    );
    let refused: Vec<String> = scripts
        .iter()
        .filter_map(|path| {
            common::parses_under_every_bash(path)
                .err()
                .map(|why| format!("{}: {why}", path.display()))
        })
        .collect();
    assert!(refused.is_empty(), "{refused:#?}");
}
