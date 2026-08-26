//! Drift guard for the envelope names the plugin's shipped prose depends on.
//!
//! Each document below is written against fields of a harness envelope or of
//! `harness.toml`. Renaming one would leave the prose describing a key nothing
//! carries, and a model reads an absent key as an answer rather than as an
//! error — a removal verb silently unavailable, or a judgement made against a
//! field the agent filled in for itself.
//!
//! Both directions, because each catches a different mistake: a name the
//! schema dropped, and a document that stopped citing a name this guard still
//! claims to watch. Constitution IX — no hand-maintained fact in two places
//! without a guard.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::export::{SchemaTarget, schema_for};

/// Plugin documents and the names each is written against.
const CONTRACTS: &[(&str, &[&str])] = &[
    (
        "reference/retire.md",
        &["harness", "hooks", "stops_with_prevention", "rule_loads"],
    ),
    (
        "agents/session-judge.md",
        &[
            "citation",
            "chars",
            "turns",
            "agent_turns",
            "questions",
            "edits",
            "files",
            "commits",
            "interrupts",
            "denials",
            "steered_away",
            "tokens",
            "models",
            "tools",
        ],
    ),
    (
        "commands/measure.md",
        &[
            "repeated_blocks",
            "restated_blocks",
            "interventions",
            "post_commit_reedits",
            "compactions",
            "cumulative_dropped_tokens",
            "invocations",
            "blocked",
            "min_support",
            "submission_sample",
            "repository",
            "by_fate",
            "reverted_by",
            "tools",
            "sessions",
        ],
    ),
];

fn plugin_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} unreadable at {}: {e}", path.display()))
}

/// Every property name anywhere in the schemas the plugin's prose reads.
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
    for target in [SchemaTarget::Session, SchemaTarget::Config] {
        walk(&schema_for(target), &mut out);
    }
    out
}

#[test]
fn every_name_the_prose_depends_on_exists_in_a_schema() {
    let names = schema_property_names();
    for (doc, fields) in CONTRACTS {
        for field in *fields {
            assert!(
                names.contains(*field),
                "{doc} is written against `{field}`, which no schema carries"
            );
        }
    }
}

#[test]
fn every_name_this_guard_watches_is_still_cited() {
    for (doc, fields) in CONTRACTS {
        let body = plugin_file(doc);
        for field in *fields {
            assert!(
                body.contains(field),
                "`{field}` is guarded here but {doc} no longer cites it; \
                 either the contract changed or this guard is watching nothing"
            );
        }
    }
}

#[test]
fn the_retire_contract_names_every_verb_the_skill_menu_offers() {
    let doc = plugin_file("reference/retire.md");
    let skill = plugin_file("SKILL.md");

    for verb in ["drop-hook", "drop-rule"] {
        assert!(doc.contains(verb), "retire.md is missing the `{verb}` verb");
        assert!(
            skill.contains(verb),
            "the retire menu in SKILL.md is missing `{verb}`"
        );
    }
}

#[test]
fn the_command_dispatches_the_agent_the_plugin_ships() {
    let command = plugin_file("commands/measure.md");
    let agent = plugin_file("agents/session-judge.md");
    let name = agent
        .lines()
        .find_map(|l| l.strip_prefix("name: "))
        .expect("the agent declares a name");

    assert!(
        command.contains(name),
        "measure.md dispatches no agent called `{name}`; a renamed agent leaves \
         the command naming one that does not exist"
    );
}
