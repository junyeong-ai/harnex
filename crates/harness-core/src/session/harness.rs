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
//! ## What this module refuses to do
//!
//! - Never attribute a denial to a permission rule. The record does not carry
//!   one, and re-deriving the match would reimplement the runtime's own
//!   matching semantics — the drift `spec` exists to prevent, with a deletion
//!   decision downstream of it.
//! - Never judge a cost. A hook that spends time and prevents nothing is
//!   reported as exactly that; whether it should go is a reading.
//! - Never carry every citation. A group's count is made verifiable by its
//!   first and last occurrence; the transcripts hold the rest.
//! - Never present a cost as a verdict. `durationMs` is per hook and exact;
//!   nothing in the record attributes what a hook produced to that hook, so
//!   [`HookCost::stops_with_prevention`] is reported with its limit rather than
//!   used as a predicate.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::record::{Citation, Record};

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
    pub loads: usize,
    /// Characters entering context across every load.
    pub chars: usize,
    pub span: Span,
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
        if self.first.is_none() {
            self.first = Some(citation.clone());
        }
        self.last = Some(citation.clone());
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
    rules: HashMap<PathBuf, Group>,
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
                        // serde_json orders object keys, so an input serialises
                        // the same way every time it is equal — the grouping
                        // needs no per-tool notion of which argument matters.
                        let key = (denial.tool.clone(), input.to_string());
                        self.blocked
                            .entry(key)
                            .or_insert_with(|| (input.clone(), Group::default()))
                            .1
                            .observe(&turn.citation, 0);
                    }
                }
            }
            Record::RuleLoad(load) => {
                self.rules
                    .entry(load.path.clone())
                    .or_default()
                    .observe(&load.citation, load.chars as u64);
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
            Record::Compaction(_) => {}
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

    pub fn finish(self, with_text: bool) -> HarnessFacts {
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
        blocked.sort_by(|a, b| {
            b.attempts
                .cmp(&a.attempts)
                .then(a.span.first.timestamp.cmp(&b.span.first.timestamp))
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
            .map(|(path, g)| RuleLoadGroup {
                path,
                loads: g.count,
                chars: g.weight as usize,
                span: g.span(),
            })
            .collect();
        rule_loads.sort_by(|a, b| b.chars.cmp(&a.chars).then(a.path.cmp(&b.path)));

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
mod tests {
    use super::*;
    use crate::session::record::{Authorship, Denial, HookRun, RuleLoad, StopSummary, UserTurn};

    fn cite(uuid: &str, seconds: i64) -> Citation {
        Citation {
            session: "s1".into(),
            file: PathBuf::from("/corpus/s.jsonl"),
            uuid: uuid.into(),
            timestamp: jiff::Timestamp::from_second(seconds).unwrap(),
        }
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
        })
    }

    fn loaded(uuid: &str, seconds: i64, path: &str, chars: usize) -> Record {
        Record::RuleLoad(RuleLoad {
            citation: cite(uuid, seconds),
            path: PathBuf::from(path),
            chars,
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
