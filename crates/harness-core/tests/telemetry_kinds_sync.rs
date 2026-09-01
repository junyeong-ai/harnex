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
use harness_core::session::ASSET_TOOL_KEYS;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The distinct tools whose invocations the emit records, read from the
/// authority itself — never a second copy, so a tool added to `ASSET_TOOL_KEYS`
/// fails this guard until the matcher prose names it too.
fn element_tools() -> Vec<&'static str> {
    let mut tools: Vec<&str> = ASSET_TOOL_KEYS.iter().map(|(tool, _, _)| *tool).collect();
    tools.dedup();
    tools
}

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
fn the_pattern_prose_names_the_kind_and_every_element_tool_the_authority_defines() {
    // The wiring prose names the Kind literally (the rule states the contract
    // in imperatives and names no specific Kind, per Article VIII).
    let wiring = [
        "plugins/harnex/reference/patterns.md",
        "plugins/harnex/templates/patterns/manifest.toml",
    ];
    for rel in wiring {
        let content = read(rel);
        assert!(
            content.contains(HARNESS_INVOCATION_KIND),
            "{rel} no longer names the `{HARNESS_INVOCATION_KIND}` Kind"
        );
        // The matcher the skill wires the settings entries from must name every
        // element tool `asset_of` resolves — bound to the authority, not a
        // second list, so adding a tool there fails here until the prose moves.
        for tool in element_tools() {
            assert!(
                content.contains(tool),
                "{rel} wires a matcher that omits `{tool}`, which ASSET_TOOL_KEYS resolves \
                 an element for — the emit would record it while the wiring never fires it"
            );
        }
    }
}
