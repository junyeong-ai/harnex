//! Drift guard for the module map restated in `CLAUDE.md`.
//!
//! Constitution IX: a fact with more than one representation has one owner,
//! and the rest are verified from it by a test that fails on drift. `lib.rs`
//! owns the module set by declaring it; the project map restates it so an
//! agent reading search results knows which file answers a question before
//! opening one. Nothing else may restate it — a third copy is what this
//! guard was added after finding, drifted six entries deep.
//!
//! Both directions, because each catches a different mistake: a module that
//! reached no map, and a map row for a module that was renamed away.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The map an agent reads to find the module that answers a question.
const MODULE_MAP: &str = "CLAUDE.md";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every module `lib.rs` declares, private ones included: a module the map
/// skips because it is not `pub` is still a file an agent has to place.
fn declared() -> BTreeSet<String> {
    read("crates/harness-core/src/lib.rs")
        .lines()
        .filter_map(|line| {
            let rest = line
                .strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("mod "))?;
            rest.strip_suffix(';').map(str::to_string)
        })
        .collect()
}

/// Every `harness-core::…` path the map spells.
fn mapped() -> BTreeSet<String> {
    let body = read(MODULE_MAP);
    let mut found = BTreeSet::new();
    let mut rest = body.as_str();
    while let Some(at) = rest.find("harness-core::") {
        let tail = &rest[at + "harness-core::".len()..];
        let end = tail
            .find(|c: char| !c.is_ascii_lowercase() && c != '_')
            .unwrap_or(tail.len());
        if end > 0 {
            found.insert(tail[..end].to_string());
        }
        rest = &tail[end.max(1)..];
    }
    found
}

#[test]
fn every_module_is_on_the_map() {
    let missing: Vec<String> = declared().difference(&mapped()).cloned().collect();
    assert!(
        missing.is_empty(),
        "{MODULE_MAP} has no row for {missing:?} — a module an agent cannot place \
         from the map. Add one row per module, spelled `harness-core::<name>`."
    );
}

#[test]
fn the_map_names_no_module_that_is_gone() {
    let stale: Vec<String> = mapped().difference(&declared()).cloned().collect();
    assert!(
        stale.is_empty(),
        "{MODULE_MAP} names {stale:?}, which `lib.rs` does not declare"
    );
}

/// `lib.rs` declares the modules and says what the crate refuses; the map says
/// what each module is for. A list of modules written into `lib.rs` as well is
/// the third copy, and rustdoc already generates one from the declarations.
#[test]
fn lib_rs_does_not_restate_the_module_list() {
    let lib = read("crates/harness-core/src/lib.rs");
    let doc: String = lib
        .lines()
        .take_while(|line| line.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n");
    let restated: Vec<String> = declared()
        .into_iter()
        .filter(|m| doc.contains(&format!("[`{m}`]")) || doc.contains(&format!("- `{m}`")))
        .collect();
    assert!(
        restated.is_empty(),
        "the `lib.rs` crate doc lists {restated:?} — a copy of the module set \
         that no reader needs and this guard does not cover. The map is {MODULE_MAP}."
    );
}
