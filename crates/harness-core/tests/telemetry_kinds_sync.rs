//! Drift guard for the telemetry-kinds pattern's facts.
//!
//! Constitution IX: a fact with more than one representation has one owner and
//! the rest are verified from it. The auto-emit Kind name lives in the oracle
//! (`HARNESS_INVOCATION_KIND`); the scaffold's `harness.toml` and the pattern's
//! prose restate it. The tool set the emit reads is the session module's
//! measured authority (`ASSET_TOOL_KEYS`), reused through `asset_of`; the
//! pattern's matcher prose must name exactly those tools. This test fails if a
//! restatement drifts from its owner.

use std::path::PathBuf;

use harness_core::guard::HARNESS_INVOCATION_KIND;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The tools whose invocations the emit records — the exact set `asset_of`
/// resolves an element for (`ASSET_TOOL_KEYS`). The emit reuses `asset_of`, so
/// this is not a second copy of the mapping; it is the matcher the wiring prose
/// must keep aligned with it, checked here.
const ELEMENT_TOOLS: [&str; 3] = ["Skill", "Task", "Agent"];

#[test]
fn the_scaffold_declares_the_kind_the_emit_appends_to() {
    let toml = read("plugins/harnex/templates/common/harness.toml");
    assert!(
        toml.contains(&format!("name = \"{HARNESS_INVOCATION_KIND}\"")),
        "the scaffold harness.toml no longer declares the `{HARNESS_INVOCATION_KIND}` Kind \
         the emit appends to — HARNESS_INVOCATION_KIND changed and the config drifted"
    );
}

#[test]
fn the_pattern_prose_names_the_kind_and_the_element_tool_matcher() {
    // The wiring prose names the Kind literally (the rule states the contract
    // in imperatives and names no specific Kind, per Article VIII).
    for rel in [
        "plugins/harnex/reference/patterns.md",
        "plugins/harnex/templates/patterns/manifest.toml",
    ] {
        let content = read(rel);
        assert!(
            content.contains(HARNESS_INVOCATION_KIND),
            "{rel} no longer names the `{HARNESS_INVOCATION_KIND}` Kind"
        );
    }
    // The matcher prose lives where the skill reads it to wire the settings
    // entries; it must name exactly the element tools the emit resolves.
    let matcher = "Skill|Task|Agent";
    for &tool in &ELEMENT_TOOLS {
        assert!(
            matcher.contains(tool),
            "the element-tool matcher dropped `{tool}`, which asset_of still resolves"
        );
    }
    for rel in [
        "plugins/harnex/reference/patterns.md",
        "plugins/harnex/templates/patterns/manifest.toml",
    ] {
        assert!(
            read(rel).contains(matcher),
            "{rel} no longer wires the emit to the `{matcher}` matcher"
        );
    }
}
