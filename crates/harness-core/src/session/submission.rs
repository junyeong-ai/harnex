//! # submission — the instruction as a unit, and what happened while it stood
//!
//! A turn is not an instruction. The operator sends three messages before the
//! agent replies and has said one thing; the agent works for forty turns and
//! the operator has still said one thing. [`SubmissionIndex`] owns that
//! boundary for every analyser that needs it, so the rule exists once.
//!
//! What a submission carries beyond its text is what happened while it was the
//! standing instruction: how many turns the agent took, whether the operator
//! stopped it, whether a tool call was refused, and whether the next
//! instruction arrived without waiting. Those are observed consequences rather
//! than readings of the wording, which is what makes them worth handing to
//! something that does read the wording.
//!
//! ## What this module refuses to do
//!
//! - Never score a prompt. Nothing here says a submission was vague, long or
//!   well-formed. It says what followed it.
//! - Never sample by chance. [`systematic_sample`] takes every k-th of a
//!   time-ordered list, so a window always yields the same subset and the
//!   subset spans the window rather than its beginning.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::record::Authorship;
use super::record::{Citation, Record, TokenUse, ToolUse, UserTurn};

/// The tool the runtime records when the agent stops and asks the operator
/// rather than choosing for them.
///
/// An exact floor on ambiguity, not a count of it: the tool resolves 257 times
/// over the local corpus while a question asked in prose carries no marker at
/// all. Zero here means the agent never used the tool, never that it never
/// asked.
const CLARIFYING_QUESTION_TOOL: &str = "AskUserQuestion";

/// The submission boundary, and the only place it is decided.
#[derive(Default)]
pub struct SubmissionIndex {
    open: HashMap<String, u64>,
    count: u64,
}

impl SubmissionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// The submission this turn belongs to.
    ///
    /// `None` for a turn no person is behind — a tool result, an injected
    /// record, a subagent's turn. A prompt the operator chose rather than typed
    /// opens one: they sent it, and the work under it is theirs.
    pub fn assign(&mut self, turn: &UserTurn) -> Option<u64> {
        if !turn.authorship.claims_a_person() || turn.text.is_none() {
            return None;
        }
        // A queued turn continues what is already open only while the agent has
        // said nothing back; once it has, the operator is answering it. A
        // queued turn with nothing open — a resumed session whose earlier turns
        // are in another file — opens one rather than being dropped.
        let session = &turn.citation.session;
        if let Some(open) = self
            .open
            .get(session)
            .filter(|_| turn.queued && !turn.follows_agent_output)
        {
            return Some(*open);
        }
        self.count += 1;
        self.open.insert(session.clone(), self.count);
        Some(self.count)
    }

    pub fn count(&self) -> usize {
        self.count as usize
    }
}

/// One instruction, and what followed it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Submission {
    /// The turn that opened it.
    pub citation: Citation,
    /// Operator turns folded into this instruction.
    pub turns: usize,
    pub chars: usize,
    /// Agent turns taken while this was the standing instruction.
    pub agent_turns: usize,
    /// What those turns spent.
    pub tokens: TokenUse,
    /// Wall-clock from this instruction to the last record made under it.
    ///
    /// Taken from the records' own timestamps rather than from the runtime's
    /// `turn_duration`, which measures the interval between two stops and so
    /// counts the operator's idle time as the agent's: measured over the local
    /// corpus its longest value is 245 hours, matching the gap since the
    /// previous stop to within a percent. This ends at the agent's last output,
    /// so time spent waiting for the next instruction is nobody's work.
    pub elapsed_ms: u64,
    /// Tool calls made under it, by tool. How the work was actually done,
    /// which is the half of a costly instruction its wording does not show.
    ///
    /// Summing these across a window is a floor on the window's own counts,
    /// not a second way to compute them: a result that arrives with no
    /// instruction open belongs to no instruction and is counted only where
    /// the window counts it. Measured over three projects the failures that
    /// land that way are 3 of 98, 12 of 3,856, and none of 549 — the same
    /// shortfall `denials`, `edits` and `commits` carry, for the same reason.
    pub tools: BTreeMap<String, ToolUse>,
    /// Models that answered it. More than one means a comparison of token
    /// counts against another instruction is comparing model mixes too.
    pub models: Vec<String>,
    /// Times the agent stopped to ask rather than choose — a floor, see
    /// [`CLARIFYING_QUESTION_TOOL`].
    pub questions: usize,
    /// File edits the agent made under it, through a tool the runtime records.
    pub edits: usize,
    /// Distinct files those edits touched, in path order. A floor on where the
    /// work went and not an account of it: a change written by a shell command
    /// is an edit the runtime never saw, so an empty list is silence rather
    /// than a run that changed nothing.
    pub written: Vec<PathBuf>,
    /// Commits reported under it, as the transcript abbreviated them. Sparse
    /// by design: the agent commits when asked, so most instructions end
    /// without one and an absent commit is not a failed instruction. The shas
    /// are what joins an instruction to what became of its work.
    pub commits: Vec<String>,
    /// Paths those commits changed, in path order, absolute as `written` is
    /// so the two can be held against each other — what was written and did
    /// not ship is the difference. Where the work landed, however it was made.
    /// Present only where the window was scoped to a project and that project
    /// is a git work tree.
    pub committed: Vec<PathBuf>,
    /// Interruptions the runtime marked while it stood — a floor, for the
    /// reason [`super::InterventionKind`] gives.
    pub interrupts: usize,
    /// Tool calls stopped while it stood. Grouped by cause in
    /// `harness.denials`; here it is friction against this instruction.
    pub denials: usize,
    /// Whether the next instruction arrived before this one was answered.
    pub steered_away: bool,
    /// Present only when the caller asked for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Instructions, and the window they were read from.
///
/// The coverage rides along so a saved result says what it covers. A list on
/// its own cannot be held beside another list: neither says which window,
/// which project, or which runtime it came from.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubmissionWindow {
    pub coverage: super::record::Coverage,
    pub submissions: Vec<Submission>,
}

#[derive(Default)]
pub struct SubmissionAnalyzer {
    out: Vec<Submission>,
    /// Session to the index in `out` of the instruction still standing in it,
    /// and the id that instruction was assigned.
    active: HashMap<String, (u64, usize)>,
    /// Distinct files each instruction touched, deduplicated here so the
    /// record carries each path once however many times it was edited.
    touched: HashMap<usize, BTreeSet<PathBuf>>,
    models: HashMap<usize, BTreeSet<String>>,
}

impl SubmissionAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one record, with the submission its turn was assigned if it is the
    /// operator's own text.
    pub fn observe(&mut self, record: &Record, submission: Option<u64>) {
        let session = &record.citation().session;
        match record {
            Record::User(turn) => match submission {
                Some(id) => self.observe_instruction(turn, id),
                None => self.observe_event(turn),
            },
            Record::Assistant(turn) => {
                if let Some((_, at)) = self.active.get(session).copied() {
                    let held = &mut self.out[at];
                    held.agent_turns += 1;
                    held.tokens.add(turn.tokens);
                    held.questions += turn
                        .actions
                        .iter()
                        .filter(|a| a.tool == CLARIFYING_QUESTION_TOOL)
                        .count();
                    for action in &turn.actions {
                        held.tools.entry(action.tool.clone()).or_default().calls += 1;
                    }
                    if let Some(model) = &turn.model {
                        self.models.entry(at).or_default().insert(model.clone());
                    }
                    // The agent's own output is the only record that says it was
                    // still working. Anything else arriving under an open
                    // instruction — a system note, the operator's next message
                    // before it opens one of its own — can land after the run
                    // ended, and would charge the wait to the work.
                    held.elapsed_ms = turn
                        .citation
                        .timestamp
                        .duration_since(held.citation.timestamp)
                        .as_millis()
                        .max(0) as u64;
                }
            }
            Record::RuleLoad(_) | Record::StopSummary(_) | Record::Compaction(_) => {}
        }
    }

    fn observe_instruction(&mut self, turn: &UserTurn, id: u64) {
        let session = turn.citation.session.clone();
        if let Some((open, at)) = self.active.get(&session).copied() {
            if open == id {
                let held = &mut self.out[at];
                held.turns += 1;
                held.chars += turn.text.as_deref().map_or(0, |t| t.chars().count());
                if let Some(text) = &mut held.text {
                    text.push_str("\n\n");
                    text.push_str(turn.text.as_deref().unwrap_or_default());
                }
                return;
            }
            if turn.queued && turn.follows_agent_output {
                self.out[at].steered_away = true;
            }
        }
        let text = turn.text.clone().unwrap_or_default();
        self.active.insert(session, (id, self.out.len()));
        self.out.push(Submission {
            citation: turn.citation.clone(),
            turns: 1,
            chars: text.chars().count(),
            agent_turns: 0,
            tokens: TokenUse::default(),
            elapsed_ms: 0,
            tools: BTreeMap::new(),
            models: Vec::new(),
            questions: 0,
            edits: 0,
            written: Vec::new(),
            commits: Vec::new(),
            committed: Vec::new(),
            interrupts: 0,
            denials: 0,
            steered_away: false,
            text: Some(text),
        });
    }

    fn observe_event(&mut self, turn: &UserTurn) {
        let Some((_, at)) = self.active.get(&turn.citation.session).copied() else {
            return;
        };
        if turn.interrupted {
            self.out[at].interrupts += 1;
        }
        if turn.denial.is_some() {
            self.out[at].denials += 1;
        }
        if let Some(sha) = &turn.commit {
            self.out[at].commits.push(sha.clone());
        }
        if let Some(file) = &turn.edited_file {
            self.out[at].edits += 1;
            self.touched.entry(at).or_default().insert(file.clone());
        }
        if let Some(tool) = &turn.failed_tool {
            self.out[at].tools.entry(tool.clone()).or_default().failed += 1;
        }
    }

    pub fn finish(mut self, with_text: bool) -> Vec<Submission> {
        for (at, files) in &self.touched {
            self.out[*at].written = files.iter().cloned().collect();
        }
        for (at, models) in &self.models {
            self.out[*at].models = models.iter().cloned().collect();
        }
        self.out.sort_by_key(|s| s.citation.timestamp);
        if !with_text {
            for s in &mut self.out {
                s.text = None;
            }
        }
        self.out
    }
}

/// Every k-th of a time-ordered list, at most `max` of them.
///
/// The cost lever for anything that reads submissions one at a time. Taking
/// the first `max` would report the beginning of a window as the window, and
/// taking a random subset would answer the same question differently on each
/// run; a fixed stride does neither.
pub fn systematic_sample(submissions: &[Submission], max: usize) -> Vec<Submission> {
    if max == 0 || submissions.len() <= max {
        return submissions.to_vec();
    }
    let stride = submissions.len().div_ceil(max);
    submissions.iter().step_by(stride).cloned().collect()
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::session::record::Denial;
    use std::path::PathBuf;

    fn turn(session: &str, uuid: &str, seconds: i64) -> UserTurn {
        UserTurn {
            citation: Citation {
                session: session.into(),
                file: PathBuf::from("/corpus/s.jsonl"),
                uuid: uuid.into(),
                timestamp: jiff::Timestamp::from_second(seconds).unwrap(),
            },
            authorship: Authorship::Authored,
            text: Some("resolve the root cause".into()),
            queued: false,
            follows_agent_output: false,
            interrupted: false,
            commit: None,
            edited_file: None,
            denial: None,
            failed_tool: None,
        }
    }

    #[test]
    fn messages_sent_before_any_reply_are_one_instruction() {
        let mut index = SubmissionIndex::new();
        let first = index.assign(&turn("s1", "u1", 100));
        let second = index.assign(&UserTurn {
            queued: true,
            ..turn("s1", "u2", 103)
        });

        assert_eq!(first, second);
        assert_eq!(index.count(), 1);
    }

    #[test]
    fn a_message_sent_after_the_agent_spoke_is_a_new_instruction() {
        let mut index = SubmissionIndex::new();
        index.assign(&turn("s1", "u1", 100));
        index.assign(&UserTurn {
            queued: true,
            follows_agent_output: true,
            ..turn("s1", "u2", 103)
        });

        assert_eq!(index.count(), 2);
    }

    #[test]
    fn a_queued_turn_does_not_continue_another_session() {
        let mut index = SubmissionIndex::new();
        index.assign(&turn("s1", "u1", 100));
        index.assign(&UserTurn {
            queued: true,
            ..turn("s2", "u2", 103)
        });

        assert_eq!(index.count(), 2);
    }

    #[test]
    fn a_turn_the_runtime_did_not_attribute_to_typing_is_not_an_instruction() {
        let mut index = SubmissionIndex::new();
        let unclaimed = UserTurn {
            authorship: Authorship::Unclaimed,
            denial: Some(Denial {
                kind: "user-rejected".into(),
                tool: None,
                input: None,
            }),
            ..turn("s1", "u1", 100)
        };

        assert_eq!(index.assign(&unclaimed), None);
        assert_eq!(index.count(), 0);
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;

    fn at(seconds: u32) -> Submission {
        Submission {
            citation: Citation {
                session: "s".into(),
                file: std::path::PathBuf::from("/s.jsonl"),
                uuid: format!("u{seconds}"),
                timestamp: format!("2026-08-01T00:00:{seconds:02}Z").parse().unwrap(),
            },
            turns: 1,
            chars: 1,
            agent_turns: 0,
            tokens: TokenUse::default(),
            elapsed_ms: 0,
            tools: BTreeMap::new(),
            models: Vec::new(),
            questions: 0,
            edits: 0,
            written: Vec::new(),
            commits: Vec::new(),
            committed: Vec::new(),
            interrupts: 0,
            denials: 0,
            steered_away: false,
            text: None,
        }
    }

    #[test]
    fn a_list_at_or_under_the_cap_is_returned_whole() {
        let all: Vec<Submission> = (0..5).map(at).collect();
        assert_eq!(systematic_sample(&all, 5).len(), 5);
        assert_eq!(systematic_sample(&all, 9).len(), 5);
    }

    #[test]
    fn a_sample_spans_the_window_rather_than_its_beginning() {
        let all: Vec<Submission> = (0..20).map(at).collect();
        let picked = systematic_sample(&all, 5);
        assert!(picked.len() <= 5);
        assert_eq!(picked.first().unwrap().citation.uuid, "u0");
        assert_eq!(picked.last().unwrap().citation.uuid, "u16");
    }

    #[test]
    fn the_same_list_yields_the_same_sample() {
        let all: Vec<Submission> = (0..20).map(at).collect();
        let uuids = |v: Vec<Submission>| -> Vec<String> {
            v.into_iter().map(|s| s.citation.uuid).collect()
        };
        assert_eq!(
            uuids(systematic_sample(&all, 7)),
            uuids(systematic_sample(&all, 7))
        );
    }
}
