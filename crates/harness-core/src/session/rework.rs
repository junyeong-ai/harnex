//! # rework — files edited again after the commit that shipped them
//!
//! A commit is the operator's own declaration that a unit of work is done.
//! Editing one of its files again before the next commit says the declaration
//! was early. That is worth counting, and it is countable without inventing a
//! boundary: the interval between two commits is bounded by the commits
//! themselves.
//!
//! The alternative was an episode delimited by elapsed time, and it was
//! measured rather than assumed. Gaps between authored turns decay smoothly —
//! 26% fall in 10–30 minutes, 9% in 30–45, 5% in 45–60, 8% in 1–2 hours — with
//! no valley anywhere to put a boundary in. Commits do not delimit work either:
//! 44% of sessions contain none, and 61% of commit intervals contain no
//! authored submission at all, because commits cluster inside a unit rather
//! than bounding one. So neither a clock nor a commit gives a work unit, and
//! this module claims neither. It reports the one thing both measurements
//! support: a file was committed, and then edited again before the next commit.
//!
//! ## What this module refuses to do
//!
//! - Never call the result a regression. The name is [`PostCommitReedit`]
//!   because that is what was observed; whether it indicates a premature
//!   completion is a reading, and readings belong to the skill.
//! - Never count a read as an edit. A tool result must carry a structured
//!   patch alongside the path before it counts.
//! - Never span sessions. A commit interval is bounded within the transcript
//!   that recorded the commits.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::record::{Citation, Record};

/// One file, committed and then edited again before the next commit.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PostCommitReedit {
    pub file: PathBuf,
    /// The commit that shipped the file.
    pub commit: String,
    /// Edits to it before the next commit.
    pub reedits: usize,
    /// Each of those edits, oldest first.
    pub citations: Vec<Citation>,
}

/// Commits seen, and what was edited again straight after one.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReworkFacts {
    /// Commits the transcript reported, which is a floor on the commits made
    /// — see [`super::record::UserTurn::commit`]. A rate denominated in this
    /// reads high by however much the runtime did not record.
    pub commits: usize,
    /// Files edited again before the next commit, most re-edits first.
    pub post_commit_reedits: Vec<PostCommitReedit>,
    /// Files edited again, but only after a later commit had intervened.
    /// Counted rather than listed: this is ordinary subsequent work, and it is
    /// reported so the figure above is not read as every re-edit there was.
    pub reedits_after_a_later_commit: usize,
}

#[derive(Default)]
struct SessionState {
    interval: u64,
    touched: Vec<PathBuf>,
    shipped: HashMap<PathBuf, (String, u64)>,
}

/// Accumulates commit intervals across every transcript in a run.
#[derive(Default)]
pub struct ReworkAnalyzer {
    sessions: HashMap<String, SessionState>,
    pending: HashMap<(String, PathBuf), PostCommitReedit>,
    commits: usize,
    reedits_after_a_later_commit: usize,
}

impl ReworkAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transcript's records, in the order they were written.
    pub fn observe(&mut self, records: &[Record]) {
        for record in records {
            let Record::User(turn) = record else {
                continue;
            };
            let state = self
                .sessions
                .entry(turn.citation.session.clone())
                .or_default();

            if let Some(file) = &turn.edited_file {
                if let Some((sha, shipped_in)) = state.shipped.get(file) {
                    if *shipped_in == state.interval {
                        let entry = self
                            .pending
                            .entry((sha.clone(), file.clone()))
                            .or_insert_with(|| PostCommitReedit {
                                file: file.clone(),
                                commit: sha.clone(),
                                reedits: 0,
                                citations: Vec::new(),
                            });
                        entry.reedits += 1;
                        entry.citations.push(turn.citation.clone());
                    } else {
                        self.reedits_after_a_later_commit += 1;
                    }
                }
                state.touched.push(file.clone());
            }

            if let Some(sha) = &turn.commit {
                self.commits += 1;
                state.interval += 1;
                for file in state.touched.drain(..) {
                    state.shipped.insert(file, (sha.clone(), state.interval));
                }
            }
        }
    }

    pub fn finish(self) -> ReworkFacts {
        let mut reedits: Vec<PostCommitReedit> = self.pending.into_values().collect();
        for entry in &mut reedits {
            entry.citations.sort_by_key(|c| c.timestamp);
        }
        reedits.sort_by(|a, b| {
            b.reedits
                .cmp(&a.reedits)
                .then(a.citations[0].timestamp.cmp(&b.citations[0].timestamp))
                .then(a.file.cmp(&b.file))
        });
        ReworkFacts {
            commits: self.commits,
            post_commit_reedits: reedits,
            reedits_after_a_later_commit: self.reedits_after_a_later_commit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::record::{Authorship, UserTurn};

    fn base(uuid: &str, seconds: i64) -> UserTurn {
        UserTurn {
            citation: Citation {
                session: "s1".into(),
                file: PathBuf::from("/corpus/s.jsonl"),
                uuid: uuid.into(),
                timestamp: jiff::Timestamp::from_second(seconds).unwrap(),
            },
            authorship: Authorship::Unclaimed,
            text: None,
            queued: false,
            follows_agent_output: false,
            interrupted: false,
            commit: None,
            edited_file: None,
            denial: None,
        }
    }

    fn edit(uuid: &str, seconds: i64, path: &str) -> Record {
        Record::User(UserTurn {
            edited_file: Some(PathBuf::from(path)),
            ..base(uuid, seconds)
        })
    }

    fn commit(uuid: &str, seconds: i64, sha: &str) -> Record {
        Record::User(UserTurn {
            commit: Some(sha.into()),
            ..base(uuid, seconds)
        })
    }

    #[test]
    fn a_file_edited_again_before_the_next_commit_is_reported() {
        let mut a = ReworkAnalyzer::new();
        a.observe(&[
            edit("e1", 100, "/repo/loader.rs"),
            commit("c1", 200, "aaa"),
            edit("e2", 300, "/repo/loader.rs"),
        ]);
        let facts = a.finish();

        assert_eq!(facts.commits, 1);
        assert_eq!(facts.post_commit_reedits.len(), 1);
        let r = &facts.post_commit_reedits[0];
        assert_eq!(r.commit, "aaa");
        assert_eq!(r.reedits, 1);
        assert_eq!(r.citations[0].uuid, "e2");
    }

    #[test]
    fn a_file_edited_after_a_later_commit_is_counted_not_listed() {
        let mut a = ReworkAnalyzer::new();
        a.observe(&[
            edit("e1", 100, "/repo/loader.rs"),
            commit("c1", 200, "aaa"),
            edit("e2", 300, "/repo/other.rs"),
            commit("c2", 400, "bbb"),
            edit("e3", 500, "/repo/loader.rs"),
        ]);
        let facts = a.finish();

        assert!(facts.post_commit_reedits.is_empty());
        assert_eq!(facts.reedits_after_a_later_commit, 1);
    }

    #[test]
    fn editing_before_any_commit_is_not_rework() {
        let mut a = ReworkAnalyzer::new();
        a.observe(&[
            edit("e1", 100, "/repo/loader.rs"),
            edit("e2", 200, "/repo/loader.rs"),
        ]);
        let facts = a.finish();

        assert_eq!(facts.commits, 0);
        assert!(facts.post_commit_reedits.is_empty());
        assert_eq!(facts.reedits_after_a_later_commit, 0);
    }

    #[test]
    fn a_commit_interval_does_not_cross_sessions() {
        let mut a = ReworkAnalyzer::new();
        let other_session = Record::User(UserTurn {
            citation: Citation {
                session: "s2".into(),
                ..base("e2", 300).citation
            },
            edited_file: Some(PathBuf::from("/repo/loader.rs")),
            ..base("e2", 300)
        });
        a.observe(&[
            edit("e1", 100, "/repo/loader.rs"),
            commit("c1", 200, "aaa"),
            other_session,
        ]);

        assert!(a.finish().post_commit_reedits.is_empty());
    }

    #[test]
    fn repeated_reedits_of_one_file_accumulate_under_its_commit() {
        let mut a = ReworkAnalyzer::new();
        a.observe(&[
            edit("e1", 100, "/repo/loader.rs"),
            commit("c1", 200, "aaa"),
            edit("e2", 300, "/repo/loader.rs"),
            edit("e3", 400, "/repo/loader.rs"),
        ]);
        let facts = a.finish();

        assert_eq!(facts.post_commit_reedits.len(), 1);
        assert_eq!(facts.post_commit_reedits[0].reedits, 2);
    }
}
