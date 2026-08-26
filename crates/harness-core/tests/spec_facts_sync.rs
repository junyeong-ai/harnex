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

use harness_core::policy::rule;
use harness_core::sentinel;
use harness_core::validate::{
    KNOWN_HOOK_EVENTS, KNOWN_PROJECT_SCOPE_NOOP_KEYS, KNOWN_SESSION_START_SOURCES, KNOWN_SKILL_KEYS,
};

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
fn spec_facts_session_start_sources_match_known_sources() {
    let regions = sentinel::extract_regions(&spec_facts_content());
    let block = regions
        .get("spec-facts-session-start-sources")
        .expect("missing managed region 'spec-facts-session-start-sources' in spec-facts.md");
    let parsed = parse_identifier_csv(block);
    let canonical: BTreeSet<String> = KNOWN_SESSION_START_SOURCES
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        parsed, canonical,
        "spec-facts.md session-start-sources block drifted from \
         KNOWN_SESSION_START_SOURCES — update the sentinel block to match Rust SSoT"
    );
}

/// The scaffolded `SessionStart` hook must match every documented source.
///
/// Three of the five are context-loss boundaries, and a hook that injects
/// branch, dirty-file count and recent commits is worth most precisely there.
/// A matcher of `startup|resume` is well-formed, so no validator flags it —
/// only this test holds the template to the reason it exists.
#[test]
fn the_scaffolded_session_start_hook_matches_every_source() {
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/harnex/templates/common/hooks.json"),
    )
    .expect("read common/hooks.json");
    let fragment: serde_json::Value = serde_json::from_str(&raw).expect("hooks.json parses");
    let matcher = fragment["SessionStart"][0]["matcher"]
        .as_str()
        .expect("SessionStart entry declares a matcher");
    let declared: BTreeSet<&str> = matcher.split('|').collect();
    let canonical: BTreeSet<&str> = KNOWN_SESSION_START_SOURCES.iter().copied().collect();
    assert_eq!(
        declared, canonical,
        "the scaffolded SessionStart matcher '{matcher}' does not cover every source; \
         the session-state injection is silent for the ones it omits"
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

/// The stamp is a commitment to a vocabulary, not a decoration beside it.
/// Editing a closed set without re-reading the page it came from leaves the
/// date claiming a verification that never happened — so the digest is held
/// to the live constants and the build fails until both move together.
#[test]
fn spec_stamps_match_live_vocabularies() {
    for surface in harness_core::spec::SpecSurface::ALL {
        assert_eq!(
            surface.digest,
            surface.live_digest(),
            "the '{}' vocabulary changed but its stamp did not. Re-read {} , then set \
             `measured` to today and `digest` to 0x{:016x}",
            surface.name,
            surface.doc,
            surface.live_digest()
        );
    }
}

/// Tokenize a `Tool:field` block, preserving the colon that pairs them
/// (`Bash:command, Read:file_path` → {"Bash:command", "Read:file_path"}).
/// The pairing is the fact — a field read against the wrong tool is the
/// drift this block exists to catch.
fn parse_pair_csv(block: &str) -> BTreeSet<String> {
    block
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|t| t.trim_matches(|c: char| c == '.' || c.is_whitespace()))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn spec_facts_covered_by_edit_rules_match_the_rule_grammar() {
    assert_region_matches(
        "spec-facts-covered-by-edit-rules",
        rule::COVERED_BY_EDIT_RULES,
        parse_identifier_csv,
    );
}

#[test]
fn spec_facts_covered_by_read_rules_match_the_rule_grammar() {
    assert_region_matches(
        "spec-facts-covered-by-read-rules",
        rule::COVERED_BY_READ_RULES,
        parse_identifier_csv,
    );
}

#[test]
fn spec_facts_primary_content_fields_match_the_rule_grammar() {
    assert_region_matches(
        "spec-facts-primary-content-fields",
        rule::PRIMARY_CONTENT_FIELDS,
        parse_pair_csv,
    );
}

fn assert_region_matches(region: &str, canonical: &[&str], tokenize: fn(&str) -> BTreeSet<String>) {
    let content = spec_facts_content();
    let regions = sentinel::extract_regions(&content);
    let block = regions
        .get(region)
        .unwrap_or_else(|| panic!("missing managed region '{region}' in spec-facts.md"));
    assert_eq!(
        tokenize(block),
        canonical
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
        "spec-facts.md '{region}' block drifted from policy::rule — update the \
         sentinel block to match Rust SSoT"
    );
}
