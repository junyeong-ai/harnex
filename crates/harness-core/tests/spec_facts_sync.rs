//! Drift guard between Rust closed-set vocabularies and the plugin's
//! `spec-facts.md` reference doc.
//!
//! `spec-facts.md` is the LLM-facing perishable spec knowledge — every fact
//! must be re-verifiable against the live Claude Code docs. Constitution
//! IX forbids hand-maintaining the same fact twice; the canonical sets live
//! in Rust (`KNOWN_HOOK_EVENTS`, …) and `spec-facts.md` carries a
//! sentinel-marked mirror block that this test validates.
//!
//! Sentinel parsing routes through `harness_core::sentinel::extract_regions`
//! — the same util the managed-region auditor uses. One parser, one
//! semantics; drift impossible.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::sentinel;
use harness_core::validate::{KNOWN_HOOK_EVENTS, KNOWN_PROJECT_SCOPE_NOOP_KEYS, KNOWN_SKILL_KEYS};

fn spec_facts_content() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/reference/spec-facts.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Tokenize the body of a sentinel region into a set of bare identifiers
/// (`Foo, Bar Baz` → {"Foo", "Bar", "Baz"}), stripping commas and
/// surrounding non-alphanumeric noise. The block is prose-friendly so the
/// LLM can read it; the tokenizer extracts the identifiers regardless of
/// line-wrap or punctuation.
fn parse_identifier_csv(block: &str) -> BTreeSet<String> {
    block
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Tokenize hyphenated identifiers (`allowed-tools, disallowed-tools` →
/// {"allowed-tools", "disallowed-tools"}). Splits only on comma/whitespace
/// but preserves internal hyphens and underscores.
fn parse_hyphenated_csv(block: &str) -> BTreeSet<String> {
    block
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim_matches(|c: char| c == '.' || c.is_whitespace()))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn spec_facts_hook_events_match_known_events() {
    let regions = sentinel::extract_regions(&spec_facts_content());
    let block = regions
        .get("spec-facts-hook-events")
        .expect("missing managed region 'spec-facts-hook-events' in spec-facts.md");
    let parsed = parse_identifier_csv(block);
    let canonical: BTreeSet<String> = KNOWN_HOOK_EVENTS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        parsed, canonical,
        "spec-facts.md hook-events block drifted from KNOWN_HOOK_EVENTS — \
         update the sentinel block to match Rust SSoT"
    );
}

#[test]
fn spec_facts_noop_keys_match_known_keys() {
    let regions = sentinel::extract_regions(&spec_facts_content());
    let block = regions
        .get("spec-facts-project-scope-noop-keys")
        .expect("missing managed region 'spec-facts-project-scope-noop-keys' in spec-facts.md");
    let parsed = parse_identifier_csv(block);
    let canonical: BTreeSet<String> = KNOWN_PROJECT_SCOPE_NOOP_KEYS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parsed, canonical,
        "spec-facts.md noop-keys block drifted from KNOWN_PROJECT_SCOPE_NOOP_KEYS — \
         update the sentinel block to match Rust SSoT"
    );
}

#[test]
fn spec_facts_skill_keys_match_known_keys() {
    let regions = sentinel::extract_regions(&spec_facts_content());
    let block = regions
        .get("spec-facts-skill-keys")
        .expect("missing managed region 'spec-facts-skill-keys' in spec-facts.md");
    let parsed = parse_hyphenated_csv(block);
    let canonical: BTreeSet<String> = KNOWN_SKILL_KEYS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        parsed, canonical,
        "spec-facts.md skill-keys block drifted from KNOWN_SKILL_KEYS — \
         update the sentinel block to match Rust SSoT"
    );
}
