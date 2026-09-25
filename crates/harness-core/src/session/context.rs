//! # context — what entered a context window, and what carried it there
//!
//! A window fills from what the transcript records: tool results, the
//! arguments the agent wrote into its calls, its prose and its thinking, user
//! turns, the summary a compaction leaves, and what the runtime attaches on its
//! own. [`ContextAnalyzer`] adds each up by its carrier, in characters: a
//! request is billed whole, so no carrier has a token count of its own.
//!
//! The transcript does not hold the system prompt or the tool definitions,
//! which every request rebuilds; [`super::Compaction::resumed_tokens`] bounds
//! them.
//!
//! ## What this module refuses to do
//!
//! - Never size what the record does not size. An entry whose size the record
//!   does not carry is counted in `entries` and left out of `measured`, so a
//!   row whose two differ says its `chars` is a floor.
//! - Never identify a carrier by wording. A user turn is grouped by the
//!   runtime's own authorship, a compaction summary by the mark the runtime
//!   sets on it, an attachment by the type it names.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::record::{Authorship, Record};
use crate::wire_enum::wire_enum;

wire_enum! {
    /// What carried characters into a context window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub enum ContextSourceKind {
        /// A tool's result, named by the tool. An image or a tool reference in
        /// it enters with no characters to count.
        ToolResult => "tool-result",
        /// The arguments the agent wrote into a call, named by the tool and
        /// sized as JSON: the call stays in the conversation as written.
        ToolInput => "tool-input",
        /// The agent's prose.
        AgentProse => "agent-prose",
        /// The agent's thinking, counted and never sized. A model that keeps
        /// earlier turns' thinking in context — Opus 4.5 and later, Sonnet 4.6
        /// and later — carries the full thinking, restored from the block's
        /// signature, while the transcript holds only what the display setting
        /// returned: nothing under `omitted`, a summary under `summarized`.
        /// 259,378 of the local corpus's 300,033 thinking blocks hold no text.
        AgentThinking => "agent-thinking",
        /// A user turn's text, named by its [`Authorship`].
        UserTurn => "user-turn",
        /// The summary a compaction left for the window after it.
        CompactSummary => "compact-summary",
        /// Something the runtime attached on its own, named by its type:
        /// memory, reminders, listings, notices of files changed underneath.
        Attachment => "attachment",
    }
}

/// Characters one carrier brought into context across the window.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContextSource {
    /// A [`ContextSourceKind`].
    pub kind: String,
    /// The tool, the authorship or the attachment type, by `kind`. Absent for
    /// prose, thinking and a compaction summary, and for a tool result whose
    /// call is in another transcript.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether these entered a subagent's window rather than the main
    /// thread's. A row on the subagent side adds up windows that never saw
    /// each other.
    pub sidechain: bool,
    pub entries: usize,
    /// Of `entries`, those whose whole size the record carries.
    pub measured: usize,
    pub chars: usize,
}

#[derive(Default)]
struct Tally {
    entries: usize,
    measured: usize,
    chars: usize,
}

/// Adds up what entered context, carrier by carrier.
#[derive(Default)]
pub struct ContextAnalyzer {
    tallies: HashMap<(ContextSourceKind, Option<String>, bool), Tally>,
}

impl ContextAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    fn add(
        &mut self,
        kind: ContextSourceKind,
        name: Option<&str>,
        sidechain: bool,
        chars: usize,
        measured: bool,
    ) {
        let tally = self
            .tallies
            .entry((kind, name.map(str::to_string), sidechain))
            .or_default();
        tally.entries += 1;
        tally.measured += usize::from(measured);
        tally.chars += chars;
    }

    pub fn observe(&mut self, record: &Record) {
        match record {
            Record::User(turn) => {
                let sidechain = turn.authorship == Authorship::Sidechain;
                for result in &turn.results {
                    self.add(
                        ContextSourceKind::ToolResult,
                        result.tool.as_deref(),
                        sidechain,
                        result.chars,
                        result.text_only,
                    );
                }
                if let Some(text) = &turn.text {
                    let (kind, name) = match turn.compact_summary {
                        true => (ContextSourceKind::CompactSummary, None),
                        false => (ContextSourceKind::UserTurn, Some(turn.authorship.as_str())),
                    };
                    self.add(kind, name, sidechain, text.chars().count(), turn.text_only);
                }
            }
            Record::Assistant(turn) => {
                for action in &turn.actions {
                    self.add(
                        ContextSourceKind::ToolInput,
                        Some(&action.tool),
                        turn.sidechain,
                        action.input_chars,
                        true,
                    );
                }
                if turn.chars > 0 {
                    self.add(
                        ContextSourceKind::AgentProse,
                        None,
                        turn.sidechain,
                        turn.chars,
                        true,
                    );
                }
                for _ in 0..turn.thinking {
                    self.add(
                        ContextSourceKind::AgentThinking,
                        None,
                        turn.sidechain,
                        0,
                        false,
                    );
                }
            }
            Record::Attachment(attachment) => self.add(
                ContextSourceKind::Attachment,
                Some(&attachment.kind),
                attachment.sidechain,
                attachment.rendered.unwrap_or(0),
                attachment.rendered.is_some(),
            ),
            Record::StopSummary(_) | Record::Compaction(_) => {}
        }
    }

    /// Most characters first, then by carrier, so a corpus serialises one way.
    pub fn finish(self) -> Vec<ContextSource> {
        let mut rows: Vec<(ContextSourceKind, ContextSource)> = self
            .tallies
            .into_iter()
            .map(|((kind, name, sidechain), t)| {
                (
                    kind,
                    ContextSource {
                        kind: kind.as_str().to_string(),
                        name,
                        sidechain,
                        entries: t.entries,
                        measured: t.measured,
                        chars: t.chars,
                    },
                )
            })
            .collect();
        rows.sort_by(|(ak, a), (bk, b)| {
            b.chars
                .cmp(&a.chars)
                .then(ak.cmp(bk))
                .then(a.name.cmp(&b.name))
                .then(a.sidechain.cmp(&b.sidechain))
        });
        rows.into_iter().map(|(_, row)| row).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::session::record::{
        AssistantTurn, Attachment, Citation, TokenUse, ToolAction, ToolResult, UserTurn,
    };

    fn cite(uuid: &str) -> Citation {
        Citation {
            session: "s1".into(),
            file: PathBuf::from("/corpus/s.jsonl"),
            uuid: uuid.into(),
            timestamp: jiff::Timestamp::from_second(100).unwrap(),
        }
    }

    fn user(uuid: &str, authorship: Authorship) -> UserTurn {
        UserTurn {
            citation: cite(uuid),
            authorship,
            text: None,
            text_only: false,
            results: Vec::new(),
            compact_summary: false,
            queued: false,
            follows_agent_output: false,
            interrupted: false,
            commit: None,
            edited_file: None,
            denial: None,
            failed_tool: None,
            prompt_id: None,
        }
    }

    fn row<'a>(
        rows: &'a [ContextSource],
        kind: ContextSourceKind,
        name: Option<&str>,
    ) -> &'a ContextSource {
        rows.iter()
            .find(|r| r.kind == kind.as_str() && r.name.as_deref() == name)
            .unwrap_or_else(|| panic!("no {} {name:?} row", kind.as_str()))
    }

    fn run(records: &[Record]) -> Vec<ContextSource> {
        let mut a = ContextAnalyzer::new();
        for r in records {
            a.observe(r);
        }
        a.finish()
    }

    #[test]
    fn a_result_carrying_an_image_is_an_entry_whose_size_is_a_floor() {
        let rows = run(&[Record::User(UserTurn {
            results: vec![
                ToolResult {
                    tool: Some("Read".into()),
                    chars: 40,
                    text_only: true,
                },
                ToolResult {
                    tool: Some("Read".into()),
                    chars: 2,
                    text_only: false,
                },
            ],
            ..user("u1", Authorship::Unclaimed)
        })]);

        let read = row(&rows, ContextSourceKind::ToolResult, Some("Read"));
        assert_eq!((read.entries, read.measured, read.chars), (2, 1, 42));
    }

    #[test]
    fn a_compaction_summary_is_its_own_carrier_and_not_an_unclaimed_turn() {
        let rows = run(&[
            Record::User(UserTurn {
                text: Some("summary".into()),
                text_only: true,
                compact_summary: true,
                ..user("u1", Authorship::Unclaimed)
            }),
            Record::User(UserTurn {
                text: Some("[Request interrupted by user]".into()),
                text_only: true,
                ..user("u2", Authorship::Unclaimed)
            }),
        ]);

        assert_eq!(row(&rows, ContextSourceKind::CompactSummary, None).chars, 7);
        let unclaimed = row(&rows, ContextSourceKind::UserTurn, Some("unclaimed"));
        assert_eq!((unclaimed.entries, unclaimed.chars), (1, 29));
    }

    #[test]
    fn thinking_is_counted_and_never_sized() {
        let rows = run(&[Record::Assistant(AssistantTurn {
            citation: cite("a1"),
            actions: vec![ToolAction {
                tool: "Bash".into(),
                asset: None,
                input_chars: 16,
            }],
            tokens: TokenUse::default(),
            message: None,
            model: None,
            sidechain: false,
            chars: 5,
            thinking: 3,
        })]);

        let thinking = row(&rows, ContextSourceKind::AgentThinking, None);
        assert_eq!(
            (thinking.entries, thinking.measured, thinking.chars),
            (3, 0, 0)
        );
        assert_eq!(row(&rows, ContextSourceKind::AgentProse, None).chars, 5);
        assert_eq!(
            row(&rows, ContextSourceKind::ToolInput, Some("Bash")).chars,
            16
        );
    }

    #[test]
    fn an_attachment_without_a_recorded_size_is_counted_apart_from_one_with() {
        let attachment = |uuid: &str, rendered: Option<usize>| {
            Record::Attachment(Attachment {
                citation: cite(uuid),
                kind: "edited_text_file".into(),
                rendered,
                memory: Vec::new(),
                sidechain: false,
            })
        };
        let rows = run(&[attachment("x1", Some(300)), attachment("x2", None)]);

        let edited = row(
            &rows,
            ContextSourceKind::Attachment,
            Some("edited_text_file"),
        );
        assert_eq!((edited.entries, edited.measured, edited.chars), (2, 1, 300));
    }

    #[test]
    fn the_two_windows_are_two_rows() {
        let result = |uuid: &str, authorship| {
            Record::User(UserTurn {
                results: vec![ToolResult {
                    tool: Some("Bash".into()),
                    chars: 10,
                    text_only: true,
                }],
                ..user(uuid, authorship)
            })
        };
        let rows = run(&[
            result("u1", Authorship::Unclaimed),
            result("u2", Authorship::Sidechain),
            result("u3", Authorship::Sidechain),
        ]);

        let sides: Vec<(bool, usize)> = rows.iter().map(|r| (r.sidechain, r.chars)).collect();
        assert_eq!(sides, [(true, 20), (false, 10)]);
    }
}
