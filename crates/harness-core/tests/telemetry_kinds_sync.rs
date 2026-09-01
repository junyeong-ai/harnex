//! Drift guard for the telemetry-kinds pattern's facts.
//!
//! Constitution IX: a fact with more than one representation has one owner and
//! the rest are verified from it. The auto-emit Kind name and payload fields
//! live in the oracle (`HARNESS_INVOCATION_KIND`, `SURFACE_FIELD`,
//! `OUTCOME_FIELD`); the scaffold's `harness.toml` and the pattern's prose
//! restate them. The tool set the emit reads is the session module's measured
//! authority (`ASSET_TOOL_KEYS`), reused through `asset_of`; the matcher prose
//! must name exactly those tools. This test fails if a restatement drifts —
//! and, unlike a substring check, it validates the emit's own payloads against
//! the scaffold schema and matches the matcher exactly.

use std::path::PathBuf;

use harness_core::config::{Config, ConsumerDetectorDecl, KindDecl};
use harness_core::guard::{HARNESS_INVOCATION_KIND, OUTCOME_FIELD, SURFACE_FIELD};
use harness_core::lifecycle::{RetirementSweeper, SilenceState};
use harness_core::session::ASSET_TOOL_KEYS;
use harness_core::telemetry::{JsonlStorage, KindSchema, TelemetryAppender, TelemetryQuery};

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read(rel: &str) -> String {
    let path = repo_path(rel);
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

/// The scaffold's `harness_invocation` payload schema, as the oracle would use
/// it to validate an append.
fn scaffold_kind_schema() -> KindSchema {
    let config = Config::load_from(&repo_path("plugins/harnex/templates/common/harness.toml"))
        .expect("scaffold harness.toml loads");
    let decl = config
        .telemetry
        .expect("scaffold declares [telemetry]")
        .kinds
        .into_iter()
        .find(|k| k.name == HARNESS_INVOCATION_KIND)
        .unwrap_or_else(|| panic!("scaffold declares no `{HARNESS_INVOCATION_KIND}` Kind"));
    KindSchema::from_value(&decl.payload_schema).expect("payload_schema is a valid closed schema")
}

#[test]
fn the_instruction_the_pattern_ships_actually_measures_a_surface() {
    // Executes the wiring the docs instruct, against the real scaffold rather
    // than a fixture — so what is pinned is that the instruction WORKS, not
    // that a file mentions it. Four owners meet here: the config key exists
    // under this path (a rename stops this compiling), the scaffold's Kind
    // name matches the emit's constant (validate fails on a template rename),
    // the scaffold schema accepts the emit's payload shape (the append fails
    // on a renamed field or a tightened schema), and the sweep still reads a
    // record per kind (the verdicts fail if it stops).
    let mut config = Config::load_from(&repo_path("plugins/harnex/templates/common/harness.toml"))
        .expect("scaffold harness.toml loads");
    config.kinds.push(KindDecl {
        name: "skill".into(),
        glob: ".claude/skills/*".into(),
        foundation: false,
        invocation_kind: Some(HARNESS_INVOCATION_KIND.into()),
    });
    let lifecycle = config
        .lifecycle
        .as_mut()
        .expect("scaffold declares [lifecycle]");
    lifecycle.consumer_detectors.push(ConsumerDetectorDecl {
        kind: "skill".into(),
        strategy: "grep".into(),
        pattern: "{slug}".into(),
        exclude_globs: vec![],
    });
    config
        .validate()
        .expect("the scaffold must accept the wiring the pattern instructs");

    let tmp = tempfile::tempdir().unwrap();
    for slug in ["review-lenses", "unused-skill"] {
        std::fs::create_dir_all(tmp.path().join(".claude/skills").join(slug)).unwrap();
    }
    let ledger = tmp.path().join("tele");
    {
        let mut appender = TelemetryAppender::new(
            config.telemetry.as_ref().unwrap(),
            JsonlStorage::new(ledger.clone(), 10),
        )
        .unwrap();
        // Shaped exactly as `guard::telemetry::emit` writes it.
        let mut payload = serde_json::Map::new();
        payload.insert(SURFACE_FIELD.into(), "review-lenses".into());
        payload.insert(OUTCOME_FIELD.into(), "ok".into());
        appender
            .append(HARNESS_INVOCATION_KIND, serde_json::Value::Object(payload))
            .expect("the scaffold schema must accept the payload the emit writes");
    }

    let query = TelemetryQuery::new(JsonlStorage::new(ledger, 10));
    let outcome = RetirementSweeper::new(&config, tmp.path(), &query)
        .unwrap()
        .run()
        .unwrap();
    let silence = |slug: &str| {
        outcome
            .verdicts
            .iter()
            .find(|v| v.slug == slug)
            .unwrap_or_else(|| panic!("the sweep classified no `{slug}`"))
            .silence
    };
    assert_eq!(silence("review-lenses"), SilenceState::Active);
    assert_eq!(silence("unused-skill"), SilenceState::Silent);
}

#[test]
fn the_shipped_prose_spells_the_key_and_the_kind_it_instructs() {
    // The one residue the executable guard above cannot pin: how the config
    // key is SPELLED in prose, since the test writes the Rust field rather
    // than the doc string. A rename that updated the code and not the docs
    // would leave the instruction inert, so the spelling is checked here and
    // nothing else is claimed for a token search.
    for rel in [
        "plugins/harnex/reference/patterns.md",
        "plugins/harnex/templates/patterns/telemetry-kinds/telemetry-kinds.md",
    ] {
        let content = read(rel);
        assert!(
            content.contains("invocation_kind"),
            "{rel} must tell the installer to wire `invocation_kind`, or the \
             emit records into a ledger retirement never reads"
        );
        assert!(
            content.contains(HARNESS_INVOCATION_KIND),
            "{rel} no longer names the `{HARNESS_INVOCATION_KIND}` Kind"
        );
    }
}

#[test]
fn the_emits_own_payloads_validate_against_the_scaffold_schema() {
    // The append the emit makes for each outcome must pass the scaffold's
    // closed schema. A renamed field, a changed type, or a narrowed `outcome`
    // enum would make production emission a silent no-op while the emit's own
    // unit fixture kept passing — so validate the real shape here, against the
    // real schema, not a substring of the TOML.
    let schema = scaffold_kind_schema();
    for outcome in ["ok", "failed"] {
        let mut payload = serde_json::Map::new();
        payload.insert(SURFACE_FIELD.into(), "review-lenses".into());
        payload.insert(OUTCOME_FIELD.into(), outcome.into());
        let payload = serde_json::Value::Object(payload);
        schema.validate(&payload).unwrap_or_else(|e| {
            panic!(
                "the emit's `{outcome}` payload no longer validates against the scaffold \
                 harness_invocation schema ({e}) — production emission would silently skip"
            )
        });
    }
}

#[test]
fn the_pattern_prose_names_the_kind_and_the_exact_element_matcher() {
    // The matcher the skill wires the settings entries from, in the exact
    // backtick-delimited form both files carry it — so a wrong alternation
    // (a dropped tool, an appended one, a wrong separator) fails, where a bare
    // substring check would pass on a prefix.
    let matcher = format!("`{}`", element_tools().join("|"));
    for rel in [
        "plugins/harnex/reference/patterns.md",
        "plugins/harnex/templates/patterns/manifest.toml",
    ] {
        let content = read(rel);
        assert!(
            content.contains(HARNESS_INVOCATION_KIND),
            "{rel} no longer names the `{HARNESS_INVOCATION_KIND}` Kind"
        );
        assert!(
            content.contains(&matcher),
            "{rel} does not wire the exact element matcher {matcher} — it must equal the \
             `|`-joined ASSET_TOOL_KEYS tools, delimited, so a dropped / appended tool or a \
             wrong separator fails here"
        );
    }
}
