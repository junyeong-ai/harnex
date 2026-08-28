//! # prompt — what the operator wrote twice, and which twice it was
//!
//! Repeating an instruction means two different things and the two want
//! opposite fixes. Saying it again in the same sitting means it did not survive
//! the context it was given in. Saying it again in a later session means it was
//! never installed anywhere the next session would read. Collapsing both into
//! one repeat rate produces a number that recommends nothing.
//!
//! The unit that separates them is the **submission**, decided by
//! [`super::SubmissionIndex`] and consumed here. Three messages typed in a row
//! before any reply are one instruction, not three, and no clock is consulted
//! to decide that.
//!
//! The unit inside a submission is an exact paragraph, not a similarity score.
//! A shingle overlap measure needs a window length and a stride, and the
//! meaning of those constants moves with the language — twelve characters is a
//! phrase in Korean and two words in English — so the same threshold carries a
//! different false positive rate per operator. Exact equality after whitespace
//! collapse has no such floor. `min_block_chars` is the one parameter, and it
//! is declared in `harness.toml` rather than chosen here.
//!
//! ## What this module refuses to do
//!
//! - **Never say why a paragraph was written twice.** A paragraph the operator
//!   wrote in two sessions may be a constraint no rule file holds, or the same
//!   error pasted twice, or a rule already in force that they did not trust to
//!   be. Nothing in a transcript separates those, and the third is not
//!   hypothetical: over this project's own corpus the largest such paragraphs
//!   are its always-loaded rule file in translation. The counts are the
//!   finding; the reason is a reading, and it belongs to whoever opens the
//!   citations.
//! - Never lowercase or stem. Normalisation is whitespace only; anything more
//!   merges paragraphs that differ, which is the false positive this design
//!   exists to avoid.
//! - Never read a turn the runtime did not attribute to a person's typing.
//! - Never group turns by elapsed time. The runtime records whether a turn was
//!   queued; a gap threshold would be a guess at what it already states.
//! - Never put prompt text in the result unless asked. The default carries
//!   lengths, counts and citations; the text stays on disk, reachable through
//!   the citation by anyone who should see it.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::record::Authorship;
use super::record::{Citation, UserTurn};

/// A paragraph the operator wrote more than once.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RepeatedBlock {
    pub chars: usize,
    /// Distinct sessions this paragraph was written in.
    pub sessions: usize,
    /// Distinct submissions this paragraph was written in.
    pub submissions: usize,
    pub occurrences: usize,
    /// Every place this paragraph was written, oldest first.
    pub citations: Vec<Citation>,
    /// Present only when the caller asked for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Paragraphs the operator wrote again, and what that repetition cost.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Repetition {
    /// Characters the repetition accounts for. Not the length of the
    /// paragraphs: a paragraph written three times where once would do costs
    /// two of it.
    pub chars: usize,
    /// The paragraphs, most repeated first.
    pub blocks: Vec<RepeatedBlock>,
}

/// What the operator typed, and which kind of twice they typed it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PromptFacts {
    /// Turns the operator wrote, before queued ones are folded into what they
    /// continue. Matches the `authored` count in coverage.
    pub authored_turns: usize,
    /// Instructions, after folding. The denominator for anything per-instruction.
    pub submissions: usize,
    /// Characters across every paragraph that met `min_block_chars`.
    pub block_chars: usize,
    /// Paragraphs written again inside a session that already held them.
    ///
    /// The session had the text and received it again. Why is not here: a
    /// paragraph retyped after a compaction, one retyped into a session whose
    /// rules already carry it, and one appended to weight it for a single task
    /// are three findings with three fixes and one count.
    pub within_sessions: Repetition,
    /// Paragraphs written again in a session that did not yet hold them.
    ///
    /// A second session received text a first one already had. Whether any
    /// harness held it is not observable here — project memory loaded on every
    /// turn is never attached to one, so it appears nowhere in a transcript.
    ///
    /// `None` where the window holds instructions from fewer than two
    /// sessions: a paragraph cannot be written again in a session the window
    /// does not contain, so a zero here would describe the window rather than
    /// the operator.
    pub across_sessions: Option<Repetition>,
}

#[derive(Default)]
struct BlockRecord {
    citations: Vec<Citation>,
    sessions: BTreeSet<String>,
    submissions: BTreeSet<(String, u64)>,
}

/// Accumulates paragraphs across every transcript in a run.
pub struct PromptAnalyzer {
    min_block_chars: usize,
    blocks: HashMap<String, BlockRecord>,
    /// Sessions the operator gave an instruction in. What makes repetition
    /// across sessions a question the window can answer, rather than the count
    /// of sessions the window read.
    sessions: BTreeSet<String>,
    authored_turns: usize,
    block_chars: usize,
}

impl PromptAnalyzer {
    pub fn new(min_block_chars: usize) -> Self {
        Self {
            min_block_chars,
            blocks: HashMap::new(),
            sessions: BTreeSet::new(),
            authored_turns: 0,
            block_chars: 0,
        }
    }

    pub fn observe(&mut self, turn: &UserTurn, submission: u64) {
        let Some(text) = turn.text.as_deref() else {
            return;
        };
        self.authored_turns += 1;
        let session = turn.citation.session.clone();
        self.sessions.insert(session.clone());

        for block in paragraphs(text, self.min_block_chars) {
            self.block_chars += block.chars().count();
            let record = self.blocks.entry(block).or_default();
            record.citations.push(turn.citation.clone());
            record.sessions.insert(session.clone());
            record.submissions.insert((session.clone(), submission));
        }
    }

    pub fn finish(self, submissions: usize, with_text: bool) -> PromptFacts {
        let mut repeated = Vec::new();
        let mut restated = Vec::new();
        let mut restated_chars = 0;
        let mut cross_session_chars = 0;

        for (text, mut record) in self.blocks {
            let chars = text.chars().count();
            let sessions = record.sessions.len();
            let submissions = record.submissions.len();
            if sessions < 2 && submissions < 2 {
                continue;
            }
            record.citations.sort_by_key(|c| c.timestamp);
            let block = RepeatedBlock {
                chars,
                sessions,
                submissions,
                occurrences: record.citations.len(),
                citations: record.citations,
                text: with_text.then(|| text.clone()),
            };
            // A paragraph can be both, and most are: written again in a new
            // session, and written again inside one. The two failures want
            // opposite fixes, so they are counted apart rather than by an
            // exclusive branch that files one as the other. The split is
            // exact — (occurrences - 1) is (sessions - 1) plus
            // (submissions - sessions) plus (occurrences - submissions), and
            // the last of those is one instruction saying a thing twice,
            // which is neither failure.
            if sessions >= 2 {
                cross_session_chars += chars * (sessions - 1);
                repeated.push(block.clone());
            }
            if submissions > sessions {
                restated_chars += chars * (submissions - sessions);
                restated.push(block);
            }
        }

        repeated.sort_by(|a, b| rank(b, a));
        restated.sort_by(|a, b| {
            b.submissions
                .cmp(&a.submissions)
                .then(b.chars.cmp(&a.chars))
                .then(a.citations[0].timestamp.cmp(&b.citations[0].timestamp))
                .then(a.citations[0].uuid.cmp(&b.citations[0].uuid))
        });

        PromptFacts {
            authored_turns: self.authored_turns,
            submissions,
            block_chars: self.block_chars,
            within_sessions: Repetition {
                chars: restated_chars,
                blocks: restated,
            },
            across_sessions: (self.sessions.len() >= 2).then_some(Repetition {
                chars: cross_session_chars,
                blocks: repeated,
            }),
        }
    }
}

fn rank(a: &RepeatedBlock, b: &RepeatedBlock) -> std::cmp::Ordering {
    a.sessions
        .cmp(&b.sessions)
        .then(a.chars.cmp(&b.chars))
        .then(b.citations[0].timestamp.cmp(&a.citations[0].timestamp))
        .then(b.citations[0].uuid.cmp(&a.citations[0].uuid))
}

/// Paragraphs of `text` that reach `min_chars`, each with its internal
/// whitespace collapsed to single spaces.
///
/// A paragraph break is one or more blank lines, which is how a person
/// separates a standing instruction from the request it accompanies.
fn paragraphs(text: &str, min_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    let mut flush = |lines: &mut Vec<&str>| {
        if lines.is_empty() {
            return;
        }
        let joined = lines.join(" ");
        lines.clear();
        let collapsed = collapse_whitespace(&joined);
        if collapsed.chars().count() >= min_chars {
            out.push(collapsed);
        }
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut current);
        } else {
            current.push(line);
        }
    }
    flush(&mut current);
    out
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_space = false;
    for ch in s.trim().chars() {
        if ch.is_whitespace() {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = false;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn turn(session: &str, uuid: &str, seconds: i64, text: &str) -> UserTurn {
        UserTurn {
            citation: Citation {
                session: session.into(),
                file: PathBuf::from("/corpus/s.jsonl"),
                uuid: uuid.into(),
                timestamp: jiff::Timestamp::from_second(seconds).unwrap(),
            },
            authorship: Authorship::Authored,
            text: Some(text.into()),
            queued: false,
            follows_agent_output: false,
            interrupted: false,
            commit: None,
            edited_file: None,
            denial: None,
            failed_tool: None,
        }
    }

    fn queued(session: &str, uuid: &str, seconds: i64, text: &str) -> UserTurn {
        UserTurn {
            queued: true,
            follows_agent_output: false,
            interrupted: false,
            ..turn(session, uuid, seconds, text)
        }
    }

    /// Drives the analyser through the index a run uses, so a test exercises
    /// the instruction boundary and the paragraph rule together.
    struct Run {
        index: super::super::SubmissionIndex,
        analyzer: PromptAnalyzer,
    }

    impl Run {
        fn new(min_block_chars: usize) -> Self {
            Self {
                index: super::super::SubmissionIndex::new(),
                analyzer: PromptAnalyzer::new(min_block_chars),
            }
        }

        fn observe(&mut self, turn: &UserTurn) {
            if let Some(id) = self.index.assign(turn) {
                self.analyzer.observe(turn, id);
            }
        }

        fn finish(self, with_text: bool) -> PromptFacts {
            self.analyzer.finish(self.index.count(), with_text)
        }
    }

    const STANDING: &str = "always resolve the root cause rather than patching the symptom";

    #[test]
    fn paragraphs_split_on_blank_lines_and_collapse_whitespace() {
        let blocks = paragraphs("one   two\nthree\n\n\nfour", 3);
        assert_eq!(
            blocks,
            vec!["one two three".to_string(), "four".to_string()]
        );
    }

    #[test]
    fn paragraphs_below_the_minimum_are_not_blocks() {
        assert!(paragraphs("hi\n\nalso short", 40).is_empty());
    }

    #[test]
    fn queued_turns_fold_into_the_instruction_they_continue() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&queued("s1", "u2", 103, "and check the exporter too"));
        a.observe(&queued("s1", "u3", 107, "and the loader"));
        let facts = a.finish(false);

        assert_eq!(facts.authored_turns, 3);
        assert_eq!(facts.submissions, 1, "three messages, one instruction");
    }

    #[test]
    fn the_same_paragraph_inside_one_submission_is_not_a_restatement() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&queued("s1", "u2", 103, STANDING));
        let facts = a.finish(false);

        assert!(facts.within_sessions.blocks.is_empty());
        assert_eq!(facts.within_sessions.chars, 0);
    }

    #[test]
    fn the_same_paragraph_in_a_later_submission_is_a_restatement() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&turn("s1", "u2", 900, STANDING));
        let facts = a.finish(false);

        assert!(
            facts.across_sessions.is_none(),
            "one session cannot answer whether a paragraph crossed sessions"
        );
        assert_eq!(facts.within_sessions.blocks.len(), 1);
        assert_eq!(facts.within_sessions.blocks[0].submissions, 2);
        assert_eq!(facts.within_sessions.chars, STANDING.chars().count());
    }

    #[test]
    fn the_same_paragraph_in_a_later_session_is_a_repeat_not_a_restatement() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&turn("s2", "u2", 900, STANDING));
        let facts = a.finish(false);

        let across = facts.across_sessions.unwrap();
        assert_eq!(facts.within_sessions.blocks.len(), 0);
        assert_eq!(across.blocks.len(), 1);
        assert_eq!(across.blocks[0].sessions, 2);
    }

    #[test]
    fn a_queued_turn_with_nothing_open_starts_a_submission() {
        let mut a = Run::new(20);
        a.observe(&queued("s1", "u1", 100, STANDING));
        assert_eq!(a.finish(false).submissions, 1);
    }

    #[test]
    fn a_queued_turn_does_not_continue_another_session() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&queued("s2", "u2", 103, STANDING));
        assert_eq!(a.finish(false).submissions, 2);
    }

    #[test]
    fn text_is_withheld_unless_asked_for() {
        let mut a = Run::new(20);
        a.observe(&turn("s1", "u1", 100, STANDING));
        a.observe(&turn("s2", "u2", 200, STANDING));
        assert_eq!(
            a.finish(true).across_sessions.unwrap().blocks[0]
                .text
                .as_deref(),
            Some(STANDING)
        );
    }

    #[test]
    fn turns_the_runtime_did_not_attribute_to_typing_are_not_read() {
        let mut a = Run::new(20);
        let mut unclaimed = turn("s1", "u1", 100, STANDING);
        unclaimed.authorship = Authorship::Unclaimed;
        a.observe(&unclaimed);
        a.observe(&turn("s2", "u2", 200, STANDING));

        let facts = a.finish(false);
        assert_eq!(facts.authored_turns, 1);
        assert!(
            facts.across_sessions.is_none(),
            "the turn the runtime did not attribute leaves one session speaking"
        );
    }

    #[test]
    fn blocks_rank_by_session_count_before_length() {
        let wide = "a standing instruction repeated across three separate sessions";
        let long = "a longer paragraph that only ever appeared in two of the sessions here";
        let mut a = Run::new(20);
        for (i, s) in ["s1", "s2", "s3"].iter().enumerate() {
            a.observe(&turn(s, "u", 100 + i as i64, wide));
        }
        a.observe(&turn("s4", "v1", 300, long));
        a.observe(&turn("s5", "v2", 400, long));

        let facts = a.finish(false);
        let across = facts.across_sessions.unwrap();
        assert_eq!(across.blocks[0].sessions, 3);
        assert_eq!(across.blocks[1].sessions, 2);
    }

    #[test]
    fn whitespace_differences_do_not_split_a_paragraph() {
        let mut a = Run::new(20);
        a.observe(&turn(
            "s1",
            "u1",
            100,
            "resolve   the root\ncause not the symptom",
        ));
        a.observe(&turn(
            "s2",
            "u2",
            200,
            "resolve the root cause not the symptom",
        ));
        assert_eq!(a.finish(false).across_sessions.unwrap().blocks.len(), 1);
    }

    #[test]
    fn case_differences_do_split_a_paragraph() {
        let mut a = Run::new(20);
        a.observe(&turn(
            "s1",
            "u1",
            100,
            "resolve the root cause not the symptom",
        ));
        a.observe(&turn(
            "s2",
            "u2",
            200,
            "Resolve the root cause not the symptom",
        ));
        assert!(
            a.finish(false).across_sessions.unwrap().blocks.is_empty(),
            "two sessions can answer the question; the answer is that nothing repeated"
        );
    }
}
