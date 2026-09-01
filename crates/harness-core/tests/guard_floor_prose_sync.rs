//! Drift guard for the enforcement-floor facts restated in prose.
//!
//! Constitution IX: a fact with more than one representation has one owner,
//! and the rest are verified from it by a test that fails on drift. The
//! runtime owns the built-in protected set (`BUILT_IN_PROTECTED`) and the
//! break-glass grant key (`FLOOR_EDIT_GRANT_KEY`); the plugin templates and
//! the reference doc restate them for the reader. This test fails if a
//! restatement stops matching the owner — rename the const and every file
//! below must move with it.

use std::path::PathBuf;

use harness_core::guard::floor::BUILT_IN_PROTECTED;
use harness_core::guard::floor::grant::FLOOR_EDIT_GRANT_KEY;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Files that enumerate the built-in protected set verbatim (not the ones
/// that merely describe it as "the two settings files"). Each must name
/// every entry.
const BUILT_IN_RESTATEMENTS: [&str; 2] = [
    "plugins/harnex/templates/patterns/enforcement-floor/enforcement-floor.md",
    "plugins/harnex/templates/common/harness.toml",
];

/// Files that name the break-glass grant key verbatim (the CLI message reads
/// it from the const and is not a restatement).
const GRANT_KEY_RESTATEMENTS: [&str; 3] = [
    "plugins/harnex/templates/patterns/enforcement-floor/enforcement-floor.md",
    "plugins/harnex/templates/common/harness.toml",
    ".claude/rules/guard.md",
];

#[test]
fn every_built_in_protected_path_is_named_in_the_prose_that_restates_it() {
    for rel in BUILT_IN_RESTATEMENTS {
        let content = read(rel);
        for entry in BUILT_IN_PROTECTED {
            assert!(
                content.contains(entry),
                "{rel} no longer names the built-in protected path `{entry}` — \
                 BUILT_IN_PROTECTED changed and the prose drifted"
            );
        }
    }
}

#[test]
fn the_grant_key_is_named_in_the_prose_that_restates_it() {
    for rel in GRANT_KEY_RESTATEMENTS {
        let content = read(rel);
        assert!(
            content.contains(FLOOR_EDIT_GRANT_KEY),
            "{rel} no longer names the grant key `{FLOOR_EDIT_GRANT_KEY}` — \
             FLOOR_EDIT_GRANT_KEY changed and the prose drifted"
        );
    }
}
