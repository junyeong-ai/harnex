//! # harness — what the project's own harness did, and what it cost
//!
//! These are the cause candidates for everything the prompt and rework figures
//! surface. A rule that loads on every turn is spending context; a Stop hook
//! that holds the agent is spending wall-clock; a permission rule that fires is
//! the harness acting on the work. None of it means anything on its own, which
//! is why this sits behind the operator-side facts rather than in front of them.
//!
//! Denials answer two questions and are grouped twice, because the two are not
//! the same. By `(kind, tool)` is who refused. By the refused call's own input
//! is what the operator keeps trying and cannot run — the friction the harness
//! puts in their way, which is the deletion candidate a cost figure is not.
//!
//! Neither grouping reaches the rule that matched. The runtime writes its
//! reason into the message text — which rule, or which part of a compound
//! command needed approval — and reading that would be a pattern match against
//! a literal that reports zero the day it is reworded. `tool_use_id` resolves
//! the call structurally instead, on 100% of the denials measured, and the
//! matching rule is simply not recorded anywhere. That limit is reported
//! rather than worked around.
//!
//! A rule load is the opposite case. The load records the patterns the runtime
//! matched, and those patterns are `.gitignore` lines, a grammar git owns, so
//! an edit after the load is held against what was recorded by the grammar it
//! was recorded in (`path_scope`). What the load does not record is the read
//! that triggered it, and nothing here reconstructs that.
//!
//! ## What this module refuses to do
//!
//! - Never attribute a denial to a permission rule. The record does not carry
//!   one, and re-deriving the match would reimplement the runtime's own
//!   matching semantics — the drift `spec` exists to prevent, with a deletion
//!   decision downstream of it.
//! - Never report an edit as made without a rule. The runtime withholds a load
//!   for a rule file the context has already read directly, so an edit with no
//!   load before it may still have had the rule in view.
//! - Never judge a cost. A hook that spends time and prevents nothing is
//!   reported as exactly that; whether it should go is a reading.
//! - Never carry every citation. A group's count is made verifiable by its
//!   first and last occurrence; the transcripts hold the rest.
//! - Never present a cost as a verdict. `durationMs` is per hook and exact;
//!   nothing in the record attributes what a hook produced to that hook, so
//!   [`HookCost::stops_with_prevention`] is reported with its limit rather than
//!   used as a predicate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::record::{Citation, Record, RuleLoad};
use crate::path_scope::PathScope;

/// A call that met a refusal more than once.
///
/// Once is not a pattern, the same threshold `prompt` applies to a paragraph,
/// and it is what separates a wall the operator keeps walking into from the
/// ordinary friction of a broad rule. Measured over the local corpus the
/// distinction matters: 2,848 refusals fall across 2,718 distinct calls, so
/// listing every one would present diffuse friction as a list of habits.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockedCall {
    pub tool: Option<String>,
    /// The refused call's input. Operator-written, so present only when the
    /// caller asked for text; the grouping is on it either way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    pub attempts: usize,
    pub span: Span,
}

/// A harness element and how often it was actually invoked.
///
/// The other half of the question — which elements exist and were never
/// invoked — needs the project's own tree, so it belongs to a project-scoped
/// window and not here.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssetInvocation {
    pub kind: String,
    pub name: String,
    pub calls: usize,
    pub span: Span,
}

/// A key that is the same for any two equal inputs.
///
/// `schemars` enables `serde_json/preserve_order`, so a `Value`'s map is
/// insertion-ordered and `to_string` reflects the order the keys arrived in
/// rather than the value. Two identical calls whose arguments were emitted in
/// a different order would key apart and each fall below the repeat threshold,
/// so the ordering is imposed here instead of assumed there.
fn canonical(value: &serde_json::Value) -> String {
    fn write(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push(':');
                    write(&map[k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(v, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

/// The span of one group, enough to check its count by hand.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Span {
    pub first: Citation,
    pub last: Citation,
}

/// Tool calls the runtime refused, by who refused and what was called.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DenialGroup {
    /// The runtime's own denial kind — `permission-rule` when the harness
    /// refused, `user-rejected` when the operator did.
    pub kind: String,
    /// `None` when the call that was denied is not in the same transcript.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub denials: usize,
    pub span: Span,
}

/// A project memory file, and what loading it cost.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RuleLoadGroup {
    pub path: PathBuf,
    /// Whether the loads counted here entered a subagent's window rather than
    /// the main thread's. The same file appears under both where both read it.
    /// One row holds every subagent that read the file, since a subagent's
    /// window has no identity the transcript carries.
    pub sidechain: bool,
    pub loads: usize,
    /// Characters entering context across every load. A row on the subagent
    /// side adds up windows that never saw each other, so read it as what the
    /// file cost the run and not as what any one context held.
    pub chars: usize,
    /// Of these loads, the ones after which the same context edited a file the
    /// rule's `paths:` match, before a compaction dropped the rule.
    ///
    /// A floor: an edit made through a shell leaves no record, and neither a
    /// context forked from this one nor an edit after the window is followed.
    /// It says an edit in scope came after the load, not that the rule shaped
    /// it. `None` where no load in this row recorded a scope.
    pub followed_by_scoped_edit: Option<usize>,
    pub span: Span,
}

/// A load whose context has not yet been compacted, and whether an edit its
/// scope covers has followed it there.
struct OpenLoad {
    load: RuleLoad,
    scope: Option<PathScope>,
    edited: bool,
}

/// A hook command, and what running it cost.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HookCost {
    pub command: String,
    pub runs: usize,
    pub total_ms: u64,
    /// Stops this hook ran in that the hooks held the agent through.
    ///
    /// Zero means this hook never held the agent. It does not mean the hook
    /// bought nothing: a Stop wrapper that reports rather than blocks exits 0
    /// by design, and the fields that would show what a hook produced —
    /// `hasOutput`, `hookAdditionalContext` — belong to the Stop event rather
    /// than to a hook inside it. Every Stop in the measured corpus ran three to
    /// six hooks (single-hook stops: 0 of 7,195), so none of them resolves to
    /// one hook even in principle. Cost is attributable here; value is not, and
    /// a removal gated on this zero would mark every hook there is.
    pub stops_with_prevention: usize,
    pub span: Span,
}

/// What the harness did across the window.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HarnessFacts {
    /// Most denials first.
    pub denials: Vec<DenialGroup>,
    /// The same refusals grouped by the call that met them, most attempts
    /// first. Where the harness and the operator's habits disagree.
    pub blocked: Vec<BlockedCall>,
    /// Harness elements that were invoked, most calls first.
    pub invocations: Vec<AssetInvocation>,
    /// Most characters first — the cost of having the rule, not its rank.
    pub rule_loads: Vec<RuleLoadGroup>,
    /// Most milliseconds first.
    pub hooks: Vec<HookCost>,
    pub stops: usize,
    pub hook_errors: usize,
    /// Stops these hooks held the agent through. A hook spending wall-clock
    /// with this at zero bought nothing it was installed to buy.
    pub prevented_continuations: usize,
}

#[derive(Default)]
struct Group {
    count: usize,
    weight: u64,
    flagged: usize,
    first: Option<Citation>,
    last: Option<Citation>,
}

impl Group {
    fn observe(&mut self, citation: &Citation, weight: u64) {
        self.observe_flagged(citation, weight, false);
    }

    fn observe_flagged(&mut self, citation: &Citation, weight: u64, flagged: bool) {
        self.count += 1;
        self.weight += weight;
        self.flagged += usize::from(flagged);
        // By time, not by arrival: a group spans every session in the window
        // and those are read in path order, which for a UUID-named transcript
        // has nothing to do with when it was written.
        if self
            .first
            .as_ref()
            .is_none_or(|f| citation.timestamp < f.timestamp)
        {
            self.first = Some(citation.clone());
        }
        if self
            .last
            .as_ref()
            .is_none_or(|l| citation.timestamp > l.timestamp)
        {
            self.last = Some(citation.clone());
        }
    }

    fn span(&self) -> Span {
        let first = self.first.clone().expect("a group holds an observation");
        let last = self.last.clone().unwrap_or_else(|| first.clone());
        Span { first, last }
    }
}

/// Accumulates harness activity across every transcript in a run.
#[derive(Default)]
pub struct HarnessAnalyzer {
    denials: HashMap<(String, Option<String>), Group>,
    blocked: HashMap<(Option<String>, String), (serde_json::Value, Group)>,
    invocations: HashMap<(String, String), Group>,
    /// Each row's loads, flagged where a scoped edit followed, and whether any
    /// of them recorded a scope.
    rules: HashMap<(PathBuf, bool), (Group, bool)>,
    /// Loads still in context, by the transcript that holds that context. A
    /// subagent writes its own transcript and starts without its parent's
    /// loads, and a compaction drops every load in the transcript it is in.
    open: HashMap<PathBuf, Vec<OpenLoad>>,
    hooks: HashMap<String, Group>,
    stops: usize,
    hook_errors: usize,
    prevented_continuations: usize,
}

impl HarnessAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, record: &Record) {
        match record {
            Record::User(turn) => {
                if let Some(denial) = &turn.denial {
                    self.denials
                        .entry((denial.kind.clone(), denial.tool.clone()))
                        .or_default()
                        .observe(&turn.citation, 0);
                    if let Some(input) = &denial.input {
                        let key = (denial.tool.clone(), canonical(input));
                        self.blocked
                            .entry(key)
                            .or_insert_with(|| (input.clone(), Group::default()))
                            .1
                            .observe(&turn.citation, 0);
                    }
                }
                if let (Some(edited), Some(open)) =
                    (&turn.edited_file, self.open.get_mut(&turn.citation.file))
                {
                    for load in open.iter_mut() {
                        if load.scope.as_ref().is_some_and(|s| s.contains(edited)) {
                            load.edited = true;
                        }
                    }
                }
            }
            Record::RuleLoad(load) => {
                self.open
                    .entry(load.citation.file.clone())
                    .or_default()
                    .push(OpenLoad {
                        scope: load
                            .scope
                            .as_ref()
                            .map(|s| PathScope::new(&s.root, &s.patterns)),
                        load: load.clone(),
                        edited: false,
                    });
            }
            Record::StopSummary(stop) => {
                self.stops += 1;
                self.hook_errors += stop.errors;
                if stop.prevented_continuation {
                    self.prevented_continuations += 1;
                }
                for hook in &stop.hooks {
                    self.hooks
                        .entry(hook.command.clone())
                        .or_default()
                        .observe_flagged(
                            &stop.citation,
                            hook.duration_ms,
                            stop.prevented_continuation,
                        );
                }
            }
            Record::Compaction(boundary) => self.close(&boundary.citation.file),
            Record::Assistant(turn) => {
                for asset in turn.actions.iter().filter_map(|a| a.asset.as_ref()) {
                    self.invocations
                        .entry((asset.kind.clone(), asset.name.clone()))
                        .or_default()
                        .observe(&turn.citation, 0);
                }
            }
        }
    }

    fn close(&mut self, transcript: &Path) {
        for open in self.open.remove(transcript).unwrap_or_default() {
            let (group, scoped) = self
                .rules
                .entry((open.load.path, open.load.sidechain))
                .or_default();
            group.observe_flagged(&open.load.citation, open.load.chars as u64, open.edited);
            *scoped |= open.scope.is_some();
        }
    }

    pub fn finish(mut self, with_text: bool) -> HarnessFacts {
        let transcripts: Vec<PathBuf> = self.open.keys().cloned().collect();
        for transcript in &transcripts {
            self.close(transcript);
        }

        let mut blocked: Vec<BlockedCall> = self
            .blocked
            .into_iter()
            .filter(|(_, (_, g))| g.count > 1)
            .map(|((tool, _), (input, g))| BlockedCall {
                tool,
                input: with_text.then_some(input),
                attempts: g.count,
                span: g.span(),
            })
            .collect();
        // Every sort here ends on a key that cannot tie. A comparison that
        // stops at a timestamp leaves the order to hash iteration, and the
        // same corpus would then serialise two different ways.
        blocked.sort_by(|a, b| {
            b.attempts
                .cmp(&a.attempts)
                .then(a.span.first.timestamp.cmp(&b.span.first.timestamp))
                .then(a.tool.cmp(&b.tool))
                .then(a.span.first.uuid.cmp(&b.span.first.uuid))
        });

        let mut invocations: Vec<AssetInvocation> = self
            .invocations
            .into_iter()
            .map(|((kind, name), g)| AssetInvocation {
                kind,
                name,
                calls: g.count,
                span: g.span(),
            })
            .collect();
        invocations.sort_by(|a, b| {
            b.calls
                .cmp(&a.calls)
                .then(a.kind.cmp(&b.kind))
                .then(a.name.cmp(&b.name))
        });

        let mut denials: Vec<DenialGroup> = self
            .denials
            .into_iter()
            .map(|((kind, tool), g)| DenialGroup {
                kind,
                tool,
                denials: g.count,
                span: g.span(),
            })
            .collect();
        denials.sort_by(|a, b| {
            b.denials
                .cmp(&a.denials)
                .then(a.kind.cmp(&b.kind))
                .then(a.tool.cmp(&b.tool))
        });

        let mut rule_loads: Vec<RuleLoadGroup> = self
            .rules
            .into_iter()
            .map(|((path, sidechain), (g, scoped))| RuleLoadGroup {
                path,
                sidechain,
                loads: g.count,
                chars: g.weight as usize,
                followed_by_scoped_edit: scoped.then_some(g.flagged),
                span: g.span(),
            })
            .collect();
        rule_loads.sort_by(|a, b| {
            b.chars
                .cmp(&a.chars)
                .then(a.path.cmp(&b.path))
                .then(a.sidechain.cmp(&b.sidechain))
        });

        let mut hooks: Vec<HookCost> = self
            .hooks
            .into_iter()
            .map(|(command, g)| HookCost {
                command,
                runs: g.count,
                total_ms: g.weight,
                stops_with_prevention: g.flagged,
                span: g.span(),
            })
            .collect();
        hooks.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.command.cmp(&b.command)));

        HarnessFacts {
            denials,
            blocked,
            invocations,
            rule_loads,
            hooks,
            stops: self.stops,
            hook_errors: self.hook_errors,
            prevented_continuations: self.prevented_continuations,
        }
    }
}

#[cfg(test)]
mod canonical_tests {
    use super::canonical;

    #[test]
    fn key_order_does_not_change_the_key() {
        let a = serde_json::json!({"command": "ls", "description": "list", "timeout": 5});
        let b: serde_json::Value =
            serde_json::from_str(r#"{"timeout":5,"description":"list","command":"ls"}"#).unwrap();
        assert_eq!(a, b, "serde_json compares maps by content");
        assert_ne!(
            a.to_string(),
            b.to_string(),
            "and serialises them by insertion order, which is why this exists"
        );
        assert_eq!(canonical(&a), canonical(&b));
    }

    #[test]
    fn nesting_is_ordered_too() {
        let a = serde_json::json!({"x": {"b": 1, "a": [ {"q": 1, "p": 2} ]}});
        let b: serde_json::Value =
            serde_json::from_str(r#"{"x":{"a":[{"p":2,"q":1}],"b":1}}"#).unwrap();
        assert_eq!(canonical(&a), canonical(&b));
    }

    #[test]
    fn different_values_keep_different_keys() {
        let a = serde_json::json!({"command": "ls"});
        let b = serde_json::json!({"command": "ls -l"});
        assert_ne!(canonical(&a), canonical(&b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::record::{
        Authorship, Compaction, Denial, HookRun, RuleLoad, RuleScope, StopSummary, UserTurn,
    };

    const MAIN: &str = "/corpus/s.jsonl";

    fn cite(uuid: &str, seconds: i64) -> Citation {
        cite_in(MAIN, uuid, seconds)
    }

    fn cite_in(transcript: &str, uuid: &str, seconds: i64) -> Citation {
        Citation {
            session: "s1".into(),
            file: PathBuf::from(transcript),
            uuid: uuid.into(),
            timestamp: jiff::Timestamp::from_second(seconds).unwrap(),
        }
    }

    fn scoped_load(transcript: &str, uuid: &str, seconds: i64, patterns: &[&str]) -> Record {
        Record::RuleLoad(RuleLoad {
            citation: cite_in(transcript, uuid, seconds),
            path: PathBuf::from("/repo/.claude/rules/style.md"),
            chars: 100,
            sidechain: false,
            scope: Some(RuleScope {
                root: PathBuf::from("/repo"),
                patterns: patterns.iter().map(|p| p.to_string()).collect(),
            }),
        })
    }

    fn edit(transcript: &str, uuid: &str, seconds: i64, path: &str) -> Record {
        Record::User(UserTurn {
            citation: cite_in(transcript, uuid, seconds),
            authorship: Authorship::Unclaimed,
            text: None,
            queued: false,
            follows_agent_output: true,
            interrupted: false,
            commit: None,
            edited_file: Some(PathBuf::from(path)),
            denial: None,
            failed_tool: None,
        })
    }

    fn compacted(transcript: &str, uuid: &str, seconds: i64) -> Record {
        Record::Compaction(Compaction {
            citation: cite_in(transcript, uuid, seconds),
            trigger: "auto".into(),
            pre_tokens: 0,
            post_tokens: 0,
            cumulative_dropped_tokens: 0,
            duration_ms: 0,
            resumed_tokens: None,
            instruction_chars: None,
            instruction: None,
            sidechain: false,
        })
    }

    fn denied(uuid: &str, seconds: i64, kind: &str, tool: Option<&str>) -> Record {
        Record::User(UserTurn {
            citation: cite(uuid, seconds),
            authorship: Authorship::Unclaimed,
            text: None,
            queued: false,
            follows_agent_output: false,
            interrupted: false,
            commit: None,
            edited_file: None,
            denial: Some(Denial {
                kind: kind.into(),
                tool: tool.map(str::to_string),
                input: None,
            }),
            failed_tool: None,
        })
    }

    fn loaded(uuid: &str, seconds: i64, path: &str, chars: usize) -> Record {
        loaded_into(uuid, seconds, path, chars, false)
    }

    fn loaded_into(uuid: &str, seconds: i64, path: &str, chars: usize, sidechain: bool) -> Record {
        Record::RuleLoad(RuleLoad {
            citation: cite(uuid, seconds),
            path: PathBuf::from(path),
            chars,
            sidechain,
            scope: None,
        })
    }

    fn stop(uuid: &str, seconds: i64, hooks: &[(&str, u64)], prevented: bool) -> Record {
        Record::StopSummary(StopSummary {
            citation: cite(uuid, seconds),
            hooks: hooks
                .iter()
                .map(|(c, ms)| HookRun {
                    command: (*c).into(),
                    duration_ms: *ms,
                })
                .collect(),
            errors: 0,
            prevented_continuation: prevented,
        })
    }

    fn run(records: &[Record]) -> HarnessFacts {
        let mut a = HarnessAnalyzer::new();
        for r in records {
            a.observe(r);
        }
        a.finish(false)
    }

    #[test]
    fn denials_separate_the_harness_refusing_from_the_operator_refusing() {
        let facts = run(&[
            denied("d1", 100, "permission-rule", Some("Bash")),
            denied("d2", 200, "permission-rule", Some("Bash")),
            denied("d3", 300, "user-rejected", Some("Bash")),
        ]);

        assert_eq!(facts.denials.len(), 2);
        assert_eq!(facts.denials[0].kind, "permission-rule");
        assert_eq!(facts.denials[0].denials, 2);
        assert_eq!(facts.denials[0].span.first.uuid, "d1");
        assert_eq!(facts.denials[0].span.last.uuid, "d2");
        assert_eq!(facts.denials[1].kind, "user-rejected");
    }

    #[test]
    fn a_denial_kind_this_binary_has_not_seen_gets_its_own_row() {
        let facts = run(&[denied("d1", 100, "shipped-tomorrow", Some("Bash"))]);
        assert_eq!(facts.denials[0].kind, "shipped-tomorrow");
    }

    #[test]
    fn rule_loads_rank_by_the_context_they_spend_not_by_how_often() {
        let facts = run(&[
            loaded("r1", 100, "/repo/.claude/rules/small.md", 100),
            loaded("r2", 200, "/repo/.claude/rules/small.md", 100),
            loaded("r3", 300, "/repo/.claude/rules/huge.md", 90_000),
        ]);

        assert_eq!(facts.rule_loads[0].path.file_name().unwrap(), "huge.md");
        assert_eq!(facts.rule_loads[0].chars, 90_000);
        assert_eq!(facts.rule_loads[1].loads, 2);
        assert_eq!(facts.rule_loads[1].chars, 200);
    }

    #[test]
    fn one_file_read_by_both_threads_is_two_rows_because_they_are_two_windows() {
        let path = "/repo/.claude/rules/style.md";
        let facts = run(&[
            loaded("r1", 100, path, 100),
            loaded_into("r2", 200, path, 900, true),
            loaded_into("r3", 300, path, 900, true),
        ]);

        assert_eq!(facts.rule_loads.len(), 2);
        assert!(facts.rule_loads[0].sidechain);
        assert_eq!(facts.rule_loads[0].loads, 2);
        assert_eq!(facts.rule_loads[0].chars, 1_800);
        assert!(!facts.rule_loads[1].sidechain);
        assert_eq!(facts.rule_loads[1].loads, 1);
        assert_eq!(facts.rule_loads[1].chars, 100);
    }

    #[test]
    fn two_windows_that_spent_the_same_on_one_file_still_rank_in_a_fixed_order() {
        // One file loaded once into each window is the same size in both, so
        // path and chars tie and the accumulator is a HashMap. Without the
        // window in the ordering these two rows would swap between runs.
        let path = "/repo/.claude/rules/deps.md";
        let facts = run(&[
            loaded_into("r1", 100, path, 12_076, true),
            loaded("r2", 200, path, 12_076),
        ]);

        assert_eq!(facts.rule_loads.len(), 2);
        assert!(!facts.rule_loads[0].sidechain);
        assert!(facts.rule_loads[1].sidechain);
    }

    #[test]
    fn a_load_is_followed_only_by_an_edit_its_own_paths_match() {
        let load = scoped_load(MAIN, "r1", 100, &["src"]);
        let elsewhere = run(&[load.clone(), edit(MAIN, "e1", 200, "/repo/docs/a.md")]);
        assert_eq!(elsewhere.rule_loads[0].followed_by_scoped_edit, Some(0));

        let within = run(&[load, edit(MAIN, "e1", 200, "/repo/src/a/b.rs")]);
        assert_eq!(within.rule_loads[0].followed_by_scoped_edit, Some(1));
    }

    #[test]
    fn an_edit_before_a_load_or_past_the_compaction_that_dropped_it_does_not_follow_it() {
        let facts = run(&[
            edit(MAIN, "e1", 100, "/repo/src/a.rs"),
            scoped_load(MAIN, "r1", 200, &["src"]),
            compacted(MAIN, "c1", 300),
            edit(MAIN, "e2", 400, "/repo/src/a.rs"),
            scoped_load(MAIN, "r2", 500, &["src"]),
            edit(MAIN, "e3", 600, "/repo/src/a.rs"),
        ]);
        assert_eq!(facts.rule_loads[0].loads, 2);
        assert_eq!(facts.rule_loads[0].followed_by_scoped_edit, Some(1));
    }

    #[test]
    fn an_edit_in_another_transcript_is_another_context() {
        let facts = run(&[
            scoped_load(MAIN, "r1", 100, &["src"]),
            edit(
                "/corpus/s/subagents/agent-a.jsonl",
                "e1",
                200,
                "/repo/src/a.rs",
            ),
            compacted("/corpus/s/subagents/agent-a.jsonl", "c1", 300),
            edit(MAIN, "e2", 400, "/repo/src/a.rs"),
        ]);
        assert_eq!(
            facts.rule_loads[0].followed_by_scoped_edit,
            Some(1),
            "neither the subagent's edit nor its compaction reaches the main thread's load"
        );
    }

    #[test]
    fn a_row_with_no_scoped_load_did_not_ask_the_question() {
        let facts = run(&[
            loaded("r1", 100, "/repo/crates/core/CLAUDE.md", 100),
            edit(MAIN, "e1", 200, "/repo/crates/core/src/a.rs"),
        ]);
        assert_eq!(facts.rule_loads[0].followed_by_scoped_edit, None);
    }

    #[test]
    fn a_hook_that_spends_time_and_prevents_nothing_is_visible_as_both() {
        let facts = run(&[
            stop(
                "s1",
                100,
                &[("afplay chime &", 2543), ("gate.sh", 160)],
                false,
            ),
            stop(
                "s2",
                200,
                &[("afplay chime &", 2400), ("gate.sh", 150)],
                false,
            ),
        ]);

        assert_eq!(facts.stops, 2);
        assert_eq!(facts.prevented_continuations, 0);
        assert_eq!(facts.hooks[0].command, "afplay chime &");
        assert_eq!(facts.hooks[0].runs, 2);
        assert_eq!(facts.hooks[0].total_ms, 4943);
    }

    #[test]
    fn a_hook_that_held_the_agent_is_counted_as_having_done_so() {
        let facts = run(&[stop("s1", 100, &[("gate.sh", 160)], true)]);
        assert_eq!(facts.prevented_continuations, 1);
        assert_eq!(facts.hooks[0].stops_with_prevention, 1);
    }

    #[test]
    fn a_prevention_charges_every_hook_that_ran_in_that_stop() {
        let facts = run(&[
            stop("s1", 100, &[("noisy.sh", 2000), ("gate.sh", 160)], false),
            stop("s2", 200, &[("gate.sh", 160)], true),
        ]);

        let noisy = facts
            .hooks
            .iter()
            .find(|h| h.command == "noisy.sh")
            .expect("noisy hook");
        let gate = facts
            .hooks
            .iter()
            .find(|h| h.command == "gate.sh")
            .expect("gate hook");

        assert_eq!(
            noisy.stops_with_prevention, 0,
            "a hook absent from the stop that held cannot have held it"
        );
        assert_eq!(
            gate.stops_with_prevention, 1,
            "a hook present in the stop that held is not cleared"
        );
    }

    #[test]
    fn an_unresolved_denial_tool_stays_absent_rather_than_guessed() {
        let facts = run(&[denied("d1", 100, "permission-rule", None)]);
        assert!(facts.denials[0].tool.is_none());
    }
}
