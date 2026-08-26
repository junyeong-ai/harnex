//! Drift guard for the `retire` removal contract.
//!
//! `plugins/harnex/reference/retire.md` gates each removal verb on a field of
//! the session envelope. Renaming one of those fields would leave the shipped
//! prose describing a predicate no envelope carries, and the skill would read
//! an absent key as a candidate that does not exist — a removal silently
//! unavailable, or worse, a predicate the model fills in for itself.
//!
//! The names below are load-bearing in both directions: each must exist in the
//! session schema, and each must still be cited by the document that depends on
//! it. Constitution IX — no hand-maintained fact in two places without a guard.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::export::{SchemaTarget, schema_for};

/// Envelope fields the removal predicates are written against.
const GATING_FIELDS: &[&str] = &["harness", "hooks", "stops_with_prevention", "rule_loads"];

fn retire_doc() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/reference/retire.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("retire.md unreadable at {}: {e}", path.display()))
}

/// Every property name anywhere in the session schema.
fn schema_property_names() -> BTreeSet<String> {
    fn walk(value: &serde_json::Value, out: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Object(props)) = map.get("properties") {
                    out.extend(props.keys().cloned());
                }
                for v in map.values() {
                    walk(v, out);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    walk(v, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(&schema_for(SchemaTarget::Session), &mut out);
    out
}

#[test]
fn every_gating_field_exists_in_the_session_schema() {
    let names = schema_property_names();
    for field in GATING_FIELDS {
        assert!(
            names.contains(*field),
            "retire.md gates a verb on `{field}`, which the session schema does not carry"
        );
    }
}

#[test]
fn every_gating_field_is_still_cited_by_the_retire_contract() {
    let doc = retire_doc();
    for field in GATING_FIELDS {
        assert!(
            doc.contains(field),
            "`{field}` is guarded here but retire.md no longer cites it; \
             either the contract changed or this guard is now watching nothing"
        );
    }
}

#[test]
fn the_retire_contract_names_every_verb_the_skill_menu_offers() {
    let doc = retire_doc();
    let skill = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/SKILL.md"),
    )
    .expect("SKILL.md");

    for verb in ["drop-hook", "drop-rule"] {
        assert!(doc.contains(verb), "retire.md is missing the `{verb}` verb");
        assert!(
            skill.contains(verb),
            "the retire menu in SKILL.md is missing `{verb}`"
        );
    }
}
