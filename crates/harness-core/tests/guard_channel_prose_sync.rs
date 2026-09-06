//! Drift guard for the hook channel a guard speaks instead of the envelope.
//!
//! Constitution IX: the runtime owns the pair (`OPERATOR_CHANNEL_KEY`,
//! `SUPPRESS_OUTPUT_KEY`), and the two rule files that record the exception
//! restate it for the reader. `envelope.md` carries the exception itself, so
//! it names the whole body; `guard.md` explains which reader each outcome is
//! addressed to, so it names the channel. Rename a key and both must move
//! with it.

use std::path::PathBuf;

use harness_core::guard::{OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY};

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Files that name the body a non-blocking guard writes, key by key.
const BODY_RESTATEMENTS: [&str; 1] = [".claude/rules/envelope.md"];

/// Files that name the channel the operator is addressed on.
const CHANNEL_RESTATEMENTS: [&str; 2] = [".claude/rules/envelope.md", ".claude/rules/guard.md"];

#[test]
fn every_key_of_the_body_is_named_where_the_exception_is_recorded() {
    for rel in BODY_RESTATEMENTS {
        let content = read(rel);
        for key in [OPERATOR_CHANNEL_KEY, SUPPRESS_OUTPUT_KEY] {
            assert!(
                content.contains(key),
                "{rel} records the envelope exception without naming `{key}`"
            );
        }
    }
}

#[test]
fn the_operators_channel_is_named_wherever_the_outcome_is_explained() {
    for rel in CHANNEL_RESTATEMENTS {
        let content = read(rel);
        assert!(
            content.contains(OPERATOR_CHANNEL_KEY),
            "{rel} explains which reader a guard's outcome reaches without naming \
             `{OPERATOR_CHANNEL_KEY}`"
        );
    }
}
