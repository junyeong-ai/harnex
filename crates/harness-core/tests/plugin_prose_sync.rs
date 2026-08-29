//! Drift guard for the envelope names the plugin's shipped prose depends on.
//!
//! Each document below is written against fields of a harness envelope or of
//! `harness.toml`. Renaming one would leave the prose describing a key nothing
//! carries, and a model reads an absent key as an answer rather than as an
//! error — a removal verb silently unavailable, or a judgement made against a
//! field the agent filled in for itself.
//!
//! A field is named with the type that carries it, because the names collide:
//! `sessions` belongs to both `Coverage` and `RepeatedBlock`, `chars` to three
//! types, and nine of the entries below share a name with another type. Asking
//! only whether some schema somewhere has the name cannot see either of them
//! being renamed.
//!
//! Both directions, because each catches a different mistake: a name the
//! schema dropped, and a document that stopped citing a name this guard still
//! claims to watch. Constitution IX — no hand-maintained fact in two places
//! without a guard.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use harness_core::export::{SchemaTarget, schema_for};

/// Plugin documents, how many schema names each one cites, and the fields it is
/// written against as `Type.field`.
///
/// The count is the denominator this guard would otherwise not have. A field
/// list is an allow-list, so a document that gains a citation gains it
/// unwatched, and the suite still says every test passed — the shape of failure
/// this repository refuses everywhere else, arriving here as a guard that
/// reports coverage it never measured. Declaring the count makes a new citation
/// break the build at the moment it is written, and both sides are counted by
/// the same function, so there is nothing to match approximately.
const CONTRACTS: &[(&str, usize, &[&str])] = &[
    (
        "reference/retire.md",
        9,
        &[
            "SessionFacts.harness",
            "HarnessFacts.hooks",
            "HarnessFacts.rule_loads",
            "HookCost.stops_with_prevention",
        ],
    ),
    (
        // The agent judges one submission, and reads it field by field.
        "agents/session-judge.md",
        26,
        &[
            "Submission.citation",
            "Submission.chars",
            "Submission.turns",
            "Submission.agent_turns",
            "Submission.questions",
            "Submission.edits",
            "Submission.written",
            "Submission.commits",
            "Submission.committed",
            "Submission.interrupts",
            "Submission.denials",
            "Submission.steered_away",
            "Submission.tokens",
            "Submission.models",
            "Submission.tools",
        ],
    ),
    (
        "commands/measure.md",
        57,
        &[
            "PromptFacts.across_sessions",
            "PromptFacts.within_sessions",
            "Repetition.chars",
            "Repetition.blocks",
            "SessionFacts.interventions",
            "SessionFacts.compactions",
            "SessionFacts.recovery",
            "RecoveryFacts.after_compaction",
            "RecoveryFacts.elsewhere",
            "Compaction.instruction_chars",
            "Compaction.instruction",
            "SessionFacts.repository",
            "SessionFacts.tools",
            "ReworkFacts.post_commit_reedits",
            "Compaction.cumulative_dropped_tokens",
            "HarnessFacts.invocations",
            "HarnessFacts.blocked",
            "RepositoryFacts.by_fate",
            "CommitOutcome.reverted_by",
            "Coverage.sessions",
            "Coverage.files_in_window",
            "Coverage.records_duplicated",
            "SubmissionWindow.coverage",
            "SubmissionWindow.submissions",
            "RepositoryFacts.authored_in_span",
            "Coverage.files_discovered",
            "Submission.written",
            "Submission.committed",
            "BaselineDiff.support_floor",
            "BaselineDiff.harness_change",
            "Coverage.record_types_unconsumed",
            "MetricDelta.change",
            "SessionConfig.min_support",
            "SessionConfig.submission_sample",
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

/// Every named type in the schemas the plugin's prose reads, with the
/// properties it carries.
fn schema_types() -> BTreeMap<String, BTreeSet<String>> {
    fn take(types: &mut BTreeMap<String, BTreeSet<String>>, name: &str, body: &serde_json::Value) {
        if let Some(serde_json::Value::Object(properties)) = body.get("properties") {
            types.insert(name.to_string(), properties.keys().cloned().collect());
        }
    }

    let mut types = BTreeMap::new();
    for target in [
        SchemaTarget::Session,
        SchemaTarget::SessionSubmissions,
        SchemaTarget::SessionBaseline,
        SchemaTarget::Config,
    ] {
        let schema = schema_for(target);
        if let Some(title) = schema.get("title").and_then(serde_json::Value::as_str) {
            take(&mut types, title, &schema);
        }
        if let Some(serde_json::Value::Object(defs)) = schema.get("$defs") {
            for (name, body) in defs {
                take(&mut types, name, body);
            }
        }
    }
    types
}

fn split(qualified: &str) -> (&str, &str) {
    qualified
        .split_once('.')
        .unwrap_or_else(|| panic!("`{qualified}` names no type; write it as `Type.field`"))
}

/// Every identifier a document cites in a code span.
///
/// A citation is a code span, not a word. `tools`, `commits` and `harness` are
/// ordinary English in these documents, so searching the prose for them passes
/// whatever the prose says — including prose that stopped citing the field.
fn cited_identifiers(doc: &str, body: &str) -> BTreeSet<String> {
    let spans: Vec<&str> = body.split('`').collect();
    assert!(
        spans.len() % 2 == 1,
        "{doc} leaves a ` unclosed, so this guard cannot tell a citation from prose"
    );
    spans
        .iter()
        .skip(1)
        .step_by(2)
        .flat_map(|span| span.split(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every property name any schema carries, whichever type carries it.
///
/// A citation in prose is a bare name, so this is the set a citation can be
/// tested against without resolving which type it belongs to — enough to say
/// whether the guard is watching a document, and not enough to say what it is
/// watching, which is what the field list is for.
fn schema_property_names() -> BTreeSet<String> {
    schema_types().into_values().flatten().collect()
}

#[test]
fn each_document_declares_how_many_schema_names_it_cites() {
    let names = schema_property_names();
    for (doc, declared, fields) in CONTRACTS {
        let cited = cited_identifiers(doc, &plugin_file(doc));
        let schema_names: BTreeSet<&String> = cited.intersection(&names).collect();
        let watched: BTreeSet<String> = fields.iter().map(|f| split(f).1.to_string()).collect();
        let unwatched: Vec<&&String> = schema_names
            .iter()
            .filter(|name| !watched.contains(**name))
            .collect();
        assert_eq!(
            schema_names.len(),
            *declared,
            "{doc} cites {} schema names and this guard is declared against {declared}; \
             it watches {} of them, and these it does not: {unwatched:?}. Register the \
             new citation above, or move the count deliberately",
            schema_names.len(),
            watched.len()
        );
    }
}

#[test]
fn every_field_the_prose_depends_on_is_carried_by_the_type_named_with_it() {
    let types = schema_types();
    for (doc, _, fields) in CONTRACTS {
        for qualified in *fields {
            let (owner, field) = split(qualified);
            let properties = types.get(owner).unwrap_or_else(|| {
                panic!("{doc} is written against `{qualified}`, and no schema carries `{owner}`")
            });
            assert!(
                properties.contains(field),
                "{doc} is written against `{qualified}`, and `{owner}` has no `{field}`"
            );
        }
    }
}

#[test]
fn every_field_this_guard_watches_is_still_cited_in_a_code_span() {
    for (doc, _, fields) in CONTRACTS {
        let cited = cited_identifiers(doc, &plugin_file(doc));
        for qualified in *fields {
            let (_, field) = split(qualified);
            assert!(
                cited.contains(field),
                "`{field}` is guarded here but {doc} cites it in no code span; \
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

    // Plugin agents are namespaced `plugin:agent`, and the bare name does not
    // resolve — verified against an installed copy, where `session-judge` is
    // absent and `harnex:session-judge` is what exists.
    let qualified = format!("harnex:{name}");
    assert!(
        command.contains(&qualified),
        "measure.md must dispatch `{qualified}`; the bare `{name}` is not a type \
         Claude Code resolves for a plugin agent"
    );
}
