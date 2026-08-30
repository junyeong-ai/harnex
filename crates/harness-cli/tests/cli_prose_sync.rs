//! Drift guard for the CLI invocations the shipped prose and templates spell.
//!
//! Every `` `harnex <sub>` `` citation and every scripted `harnex <sub>`
//! invocation across the plugin tree, README and CLAUDE.md must name a live
//! subcommand — and nothing may invoke the binary by its pre-rename name.
//! The commit-msg hook template shipped spelling `harness validate` for
//! months of releases; its `command -v` guard read the absent old name as
//! consent to skip, so every scaffolded repo ran a commit-msg hook that
//! validated nothing. A stale citation is the same defect one notch softer:
//! `harnex spec` survived in template prose after the surface became an
//! envelope warning.

use std::path::{Path, PathBuf};
use std::process::Command;

fn subcommands() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    let mut subs = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if line.starts_with("Options:") {
            break;
        }
        if in_commands && let Some(word) = line.split_whitespace().next() {
            subs.push(word.to_string());
        }
    }
    assert!(subs.len() > 5, "subcommand parse failed: {help}");
    subs
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn word_after(text: &str, from: usize) -> &str {
    let rest = &text[from..];
    let end = rest
        .find(|c: char| !c.is_ascii_lowercase() && c != '-')
        .unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn every_spelled_invocation_names_a_live_subcommand() {
    let subs = subcommands();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![root.join("README.md"), root.join("CLAUDE.md")];
    walk(&root.join("plugins/harnex"), &mut files);

    for file in files {
        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
        for anchor in ["`harnex ", "$(harnex ", "&& harnex "] {
            for (at, _) in text.match_indices(anchor) {
                let word = word_after(&text, at + anchor.len());
                if !word.starts_with(|c: char| c.is_ascii_lowercase()) || word == "harness" {
                    continue; // `harnex --flag`, `harnex harness.toml` prose
                }
                assert!(
                    subs.iter().any(|s| s == word),
                    "{} cites `harnex {word}`, which is not a subcommand of the built binary",
                    file.display()
                );
            }
        }
        for (at, _) in text.match_indices("harness ") {
            let word = word_after(&text, at + "harness ".len());
            assert!(
                !subs.iter().any(|s| s == word),
                "{} invokes `harness {word}` — the binary is `harnex`",
                file.display()
            );
        }
        assert!(
            !text.contains("command -v harness "),
            "{} probes for a binary named `harness` — the binary is `harnex`",
            file.display()
        );
    }
}
