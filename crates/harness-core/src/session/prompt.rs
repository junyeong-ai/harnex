//! # prompt — what the operator wrote twice
//!
//! A standing instruction that never enters `CLAUDE.md` gets retyped, and the
//! retyping is visible: the same paragraph appears verbatim in session after
//! session. This module counts that, and the count is the promotion candidate.
//!
//! The unit is an exact paragraph, not a similarity score. A shingle overlap
//! measure needs a window length and a stride, and the meaning of those
//! constants moves with the language — twelve characters is a phrase in Korean
//! and two words in English — so the same threshold carries a different false
//! positive rate per operator. Exact equality after whitespace collapse has no
//! such floor: two paragraphs either are the same paragraph or they are not.
//! `min_block_chars` is the one parameter, and it is declared in
//! `harness.toml` rather than chosen here.
//!
//! ## What this module refuses to do
//!
//! - Never lowercase or stem. Normalisation is whitespace only; anything more
//!   merges paragraphs that differ, which is the false positive this design
//!   exists to avoid.
//! - Never read a turn the runtime did not attribute to a person's typing.
//!   Only [`Authorship::Authored`] reaches the statistics.
//! - Never put prompt text in the result unless asked. The default carries
//!   lengths, counts and citations; the text stays on disk, reachable through
//!   the citation by anyone who should see it.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::record::{Authorship, Citation, UserTurn};

/// A paragraph the operator wrote in more than one session.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RepeatedBlock {
    pub chars: usize,
    pub sessions: usize,
    pub occurrences: usize,
    /// Every place this paragraph was written, oldest first.
    pub citations: Vec<Citation>,
    /// Present only when the caller asked for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// What the operator typed, and how much of it they had typed before.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PromptFacts {
    pub authored_prompts: usize,
    /// Characters across every paragraph that met `min_block_chars`.
    pub block_chars: usize,
    /// Of those, characters in a paragraph already written earlier in the same
    /// session.
    pub within_session_reuse_chars: usize,
    /// Of those, characters in a paragraph also written in another session.
    pub cross_session_reuse_chars: usize,
    /// Paragraphs written in two or more sessions, most-repeated first.
    pub repeated_blocks: Vec<RepeatedBlock>,
}

#[derive(Default)]
struct BlockRecord {
    citations: Vec<Citation>,
    sessions: BTreeSet<String>,
}

/// Accumulates paragraphs across every transcript in a run.
pub struct PromptAnalyzer {
    min_block_chars: usize,
    blocks: HashMap<String, BlockRecord>,
    seen_in_session: HashMap<String, BTreeSet<String>>,
    authored_prompts: usize,
    block_chars: usize,
    within_session_reuse_chars: usize,
}

impl PromptAnalyzer {
    pub fn new(min_block_chars: usize) -> Self {
        Self {
            min_block_chars,
            blocks: HashMap::new(),
            seen_in_session: HashMap::new(),
            authored_prompts: 0,
            block_chars: 0,
            within_session_reuse_chars: 0,
        }
    }

    pub fn observe(&mut self, turn: &UserTurn) {
        if turn.authorship != Authorship::Authored {
            return;
        }
        let Some(text) = turn.text.as_deref() else {
            return;
        };
        self.authored_prompts += 1;

        for block in paragraphs(text, self.min_block_chars) {
            let chars = block.chars().count();
            self.block_chars += chars;

            let session_blocks = self
                .seen_in_session
                .entry(turn.citation.session.clone())
                .or_default();
            if !session_blocks.insert(block.clone()) {
                self.within_session_reuse_chars += chars;
            }

            let record = self.blocks.entry(block).or_default();
            record.citations.push(turn.citation.clone());
            record.sessions.insert(turn.citation.session.clone());
        }
    }

    pub fn finish(self, with_text: bool) -> PromptFacts {
        let mut repeated: Vec<RepeatedBlock> = Vec::new();
        let mut cross_session_reuse_chars = 0;

        for (text, mut record) in self.blocks {
            if record.sessions.len() < 2 {
                continue;
            }
            let chars = text.chars().count();
            cross_session_reuse_chars += chars * record.citations.len();
            record.citations.sort_by_key(|c| c.timestamp);
            repeated.push(RepeatedBlock {
                chars,
                sessions: record.sessions.len(),
                occurrences: record.citations.len(),
                citations: record.citations,
                text: with_text.then(|| text.clone()),
            });
        }

        repeated.sort_by(|a, b| {
            b.sessions
                .cmp(&a.sessions)
                .then(b.chars.cmp(&a.chars))
                .then(a.citations[0].timestamp.cmp(&b.citations[0].timestamp))
        });

        PromptFacts {
            authored_prompts: self.authored_prompts,
            block_chars: self.block_chars,
            within_session_reuse_chars: self.within_session_reuse_chars,
            cross_session_reuse_chars,
            repeated_blocks: repeated,
        }
    }
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
        }
    }

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
    fn a_paragraph_in_two_sessions_is_a_repeated_block() {
        let standing = "always resolve the root cause rather than patching the symptom";
        let mut a = PromptAnalyzer::new(20);
        a.observe(&turn("s1", "u1", 100, standing));
        a.observe(&turn(
            "s2",
            "u2",
            200,
            &format!("{standing}\n\nnow do the thing"),
        ));
        let facts = a.finish(false);

        assert_eq!(facts.authored_prompts, 2);
        assert_eq!(facts.repeated_blocks.len(), 1);
        let block = &facts.repeated_blocks[0];
        assert_eq!(block.sessions, 2);
        assert_eq!(block.occurrences, 2);
        assert_eq!(block.citations[0].uuid, "u1");
        assert!(block.text.is_none());
    }

    #[test]
    fn a_paragraph_in_one_session_twice_is_within_session_reuse_only() {
        let standing = "always resolve the root cause rather than patching the symptom";
        let mut a = PromptAnalyzer::new(20);
        a.observe(&turn("s1", "u1", 100, standing));
        a.observe(&turn("s1", "u2", 200, standing));
        let facts = a.finish(false);

        assert!(facts.repeated_blocks.is_empty());
        assert_eq!(facts.within_session_reuse_chars, standing.chars().count());
    }

    #[test]
    fn text_is_withheld_unless_asked_for() {
        let standing = "always resolve the root cause rather than patching the symptom";
        let mut a = PromptAnalyzer::new(20);
        a.observe(&turn("s1", "u1", 100, standing));
        a.observe(&turn("s2", "u2", 200, standing));
        assert_eq!(
            a.finish(true).repeated_blocks[0].text.as_deref(),
            Some(standing)
        );
    }

    #[test]
    fn turns_the_runtime_did_not_attribute_to_typing_are_not_read() {
        let standing = "always resolve the root cause rather than patching the symptom";
        let mut a = PromptAnalyzer::new(20);
        let mut unclaimed = turn("s1", "u1", 100, standing);
        unclaimed.authorship = Authorship::Unclaimed;
        a.observe(&unclaimed);
        a.observe(&turn("s2", "u2", 200, standing));

        let facts = a.finish(false);
        assert_eq!(facts.authored_prompts, 1);
        assert!(facts.repeated_blocks.is_empty());
    }

    #[test]
    fn blocks_rank_by_session_count_before_length() {
        let wide = "a standing instruction repeated across three separate sessions";
        let long = "a longer paragraph that only ever appeared in two of the sessions here";
        let mut a = PromptAnalyzer::new(20);
        for (i, s) in ["s1", "s2", "s3"].iter().enumerate() {
            a.observe(&turn(s, "u", 100 + i as i64, wide));
        }
        a.observe(&turn("s1", "v1", 300, long));
        a.observe(&turn("s2", "v2", 400, long));

        let facts = a.finish(false);
        assert_eq!(facts.repeated_blocks[0].sessions, 3);
        assert_eq!(facts.repeated_blocks[1].sessions, 2);
    }

    #[test]
    fn whitespace_differences_do_not_split_a_paragraph() {
        let mut a = PromptAnalyzer::new(20);
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
        assert_eq!(a.finish(false).repeated_blocks.len(), 1);
    }

    #[test]
    fn case_differences_do_split_a_paragraph() {
        let mut a = PromptAnalyzer::new(20);
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
        assert!(a.finish(false).repeated_blocks.is_empty());
    }
}
