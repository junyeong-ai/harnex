//! The module doc contract, as a gate rather than as prose.
//!
//! `.claude/rules/module-doc.md` asks every `mod.rs` for a `//!` block naming
//! what the module is, how it works, and what it refuses to do. Only the last
//! of the three has a closed form, and the rule calls that one non-optional:
//! it is where a module's boundary is written down, and it is what a reader
//! reaches for before adding something the module deliberately excludes.
//!
//! So that is what is checked. A module whose purpose sentence is thin reads
//! badly; a module with no refusal section has no recorded boundary at all,
//! and nothing else in this repository would notice.

use std::path::{Path, PathBuf};

/// The rule owns the contract and its scope; this is its `paths:` glob.
const RULE: &str = ".claude/rules/module-doc.md";

/// The one section with a closed form, spelled as every module spells it.
const REFUSAL: &str = "## What this module refuses to do";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `mod.rs` under the crate the rule scopes to. `lib.rs` is exempt and
/// is not one of these: it declares modules and documents no behaviour.
fn module_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.file_name().is_some_and(|name| name == "mod.rs") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(&root().join("crates/harness-core/src"), &mut found);
    found.sort();
    found
}

/// The leading `//!` block, which is the only part of a file that documents
/// the module rather than an item inside it.
fn module_doc(path: &Path) -> String {
    let body = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    body.lines()
        .take_while(|line| line.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rel(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// A walk that finds nothing passes every case below without reading a file,
/// so the population is asserted before it is judged.
#[test]
fn the_rule_governs_modules_that_exist() {
    let files = module_files();
    assert!(
        files.len() > 1,
        "no modules found to check — the walk is wrong, not the crate"
    );
}

#[test]
fn every_module_documents_itself() {
    let undocumented: Vec<String> = module_files()
        .into_iter()
        .filter(|path| module_doc(path).is_empty())
        .map(|path| rel(&path))
        .collect();
    assert!(
        undocumented.is_empty(),
        "{undocumented:?} open with no `//!` block; {RULE} says what one carries"
    );
}

#[test]
fn every_module_records_what_it_refuses() {
    let unbounded: Vec<String> = module_files()
        .into_iter()
        .filter(|path| !module_doc(path).contains(REFUSAL))
        .map(|path| rel(&path))
        .collect();
    assert!(
        unbounded.is_empty(),
        "{unbounded:?} carry no `{REFUSAL}` section — a module with no recorded \
         boundary, which is the one part of {RULE} that is not optional"
    );
}

/// The heading this gate matches is a fact the rule and every module share, so
/// the rule is the place it is spelled and this is the check that it still is.
#[test]
fn the_rule_spells_the_section_this_gate_matches() {
    let rule = std::fs::read_to_string(root().join(RULE)).expect("the rule is readable");
    let heading = REFUSAL
        .trim_start_matches("# ")
        .trim_start_matches('#')
        .trim();
    assert!(
        rule.to_lowercase().contains(&heading.to_lowercase()),
        "{RULE} no longer names `{heading}`, so this gate is matching a heading \
         nothing asks for"
    );
}
