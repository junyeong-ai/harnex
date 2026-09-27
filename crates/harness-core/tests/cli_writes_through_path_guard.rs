//! Constitution III over the surface that would otherwise route around it.
//!
//! Every file mutation goes through `path_guard`, which rejects traversal and
//! refuses to write through a symlink. A handler reaching for `std::fs`
//! directly gets neither, and nothing else in this repository would fail:
//! the handler's own test asserts the file it wrote, which a direct write
//! satisfies exactly as well.
//!
//! An allowlist and not a list of what is forbidden. `std::fs` grows, and a
//! mutating function added to it after this file was written would pass a
//! denylist by being unknown to it. Here it fails until someone says which it
//! is, which is the answer being asked for.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The crate whose handlers the constitution names.
const HANDLERS: &str = "crates/harness-cli/src";

/// `std::fs` calls that read. Everything else this crate reaches for has to
/// be argued for, whether it mutates or is merely new.
const READS: [&str; 9] = [
    "read",
    "read_to_string",
    "read_dir",
    "read_link",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "exists",
    "try_exists",
];

/// Openers that hand back a writable handle, so the write itself never spells
/// `fs::` and the scan above would not see it.
const OPENERS: [&str; 3] = ["File::create", "File::create_new", "OpenOptions"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, found: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join(HANDLERS), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let rel = path
                .strip_prefix(root())
                .unwrap_or(&path)
                .display()
                .to_string();
            (rel, body)
        })
        .collect()
}

/// Every `fs::<name>` this crate spells, with the file it is spelled in.
fn fs_calls(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = body;
    while let Some(at) = rest.find("fs::") {
        let tail = &rest[at + "fs::".len()..];
        let end = tail
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(tail.len());
        if end > 0 {
            found.insert(tail[..end].to_string());
        }
        rest = &tail[end.max(1)..];
    }
    found
}

#[test]
fn the_scan_reads_the_handlers_it_claims_to() {
    let files = sources();
    assert!(
        files.len() > 1 && files.iter().any(|(_, body)| body.contains("fs::")),
        "no handler source read, or none reaches for `fs` at all — a pass here \
         would say nothing about {HANDLERS}"
    );
}

#[test]
fn no_handler_reaches_past_path_guard_to_the_filesystem() {
    let reads: BTreeSet<&str> = READS.into_iter().collect();
    let offenders: Vec<String> = sources()
        .into_iter()
        .flat_map(|(rel, body)| {
            fs_calls(&body)
                .into_iter()
                .filter(|call| !reads.contains(call.as_str()))
                .map(move |call| format!("{rel}: fs::{call}"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{offenders:?} — a mutation here skips `path_guard`'s traversal and \
         symlink refusals (constitution III). Route it through `write_atomic` \
         or `append_line`; if it only reads, add it to READS."
    );
}

#[test]
fn no_handler_opens_a_file_for_writing() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .flat_map(|(rel, body)| {
            OPENERS
                .into_iter()
                .filter(move |opener| body.contains(opener))
                .map(move |opener| format!("{rel}: {opener}"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{offenders:?} hand back a writable handle, so the write never spells \
         `fs::` and the case above cannot see it (constitution III)"
    );
}
