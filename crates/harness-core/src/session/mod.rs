//! # session — what the operator actually asked for, read from the transcripts
//!
//! Claude Code writes every session to JSONL under a machine-global root. That
//! record is the only local evidence of how work went: what was asked, what had
//! to be asked twice, and what a harness did about it. harnex parses the path
//! to it on every hook invocation and has never opened the file.
//!
//! This module opens it. [`collect`] walks the declared roots, reads every
//! transcript, and returns [`SessionFacts`] — counts and citations, never a
//! judgement. Whether a repeated paragraph means the operator is disciplined or
//! the harness is missing a rule is a reading of the facts, and readings belong
//! to the skill that can be revised without a release.
//!
//! ## Coverage before numbers
//!
//! Every result carries [`record::Coverage`], and two failures are distinguished
//! that otherwise arrive looking identical: a corpus that is empty, and a run
//! that read nothing. The second is an error. This is not hypothetical — a
//! verification pass written while designing this module opened zero files
//! because of a relative path, swallowed the error, and printed a confident
//! zero percent.
//!
//! [`require_coverage`] is the gate for commands that report rates. It is not
//! applied to commands whose subject *is* the coverage, which is why it lives
//! here rather than inside [`collect`].
//!
//! ## What this module refuses to do
//!
//! - Never judge. No metric here carries a causal name: the count of edits
//!   after a commit is `post_commit_reedit`, not "regression", because a name
//!   in the envelope hardens into a finding.
//! - Never identify a turn by its wording. Interruptions are the current
//!   example of the cost: the runtime marks only some of them structurally
//!   (`interruptedMessageId`, on roughly 70% and not gated by version), and the
//!   rest are identifiable only by matching a runtime-emitted literal. Counting
//!   the structural subset would understate interruptions by an unknown amount
//!   in the flattering direction, and matching the literal reports zero the day
//!   it is reworded. Interruptions therefore wait for a sound identification
//!   rather than shipping an approximate one.
//! - Never write. This module reads; nothing here mutates a project.
//! - Never reach the network. Every input is a local file.

pub mod discovery;
pub mod prompt;
pub mod record;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::config::SessionConfig;
use crate::error::{Error, Result};

pub use prompt::{PromptFacts, RepeatedBlock};
pub use record::{Authorship, Citation, Coverage};

/// What a caller wants out of a scan beyond the defaults.
#[derive(Debug, Clone, Default)]
pub struct CollectOptions {
    /// Include prompt text in the result. Off by default: the corpus spans
    /// every project on the machine, and a fact ledger has no business holding
    /// what was typed into all of them. Citations reach the text for anyone who
    /// should see it.
    pub with_text: bool,
    /// Ignore records older than this.
    pub since: Option<Timestamp>,
}

/// Counts and citations for one window of the corpus.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionFacts {
    pub coverage: Coverage,
    pub prompts: PromptFacts,
}

/// Read every transcript under the configured roots.
///
/// Reports rather than raises for anything a single record or file can be wrong
/// about; raises when the run as a whole did not happen.
pub fn collect(config: &SessionConfig, options: &CollectOptions) -> Result<SessionFacts> {
    let files = discovery::discover(&config.roots)?;

    let mut coverage = Coverage {
        files_discovered: files.len(),
        ..Coverage::default()
    };
    let mut analyzer = prompt::PromptAnalyzer::new(config.min_block_chars);

    for path in &files {
        let records = match record::read_transcript(path, &mut coverage) {
            Ok(r) => r,
            Err(_) => {
                coverage.files_unreadable += 1;
                continue;
            }
        };
        for rec in &records {
            if let Some(since) = options.since
                && rec.citation().timestamp < since
            {
                continue;
            }
            if let record::Record::User(turn) = rec {
                analyzer.observe(turn);
            }
        }
    }

    if coverage.files_discovered > 0 && coverage.files_read == 0 {
        return Err(Error::SessionRootUnreadable {
            path: files
                .first()
                .cloned()
                .unwrap_or_else(|| std::path::PathBuf::from(".")),
            message: format!(
                "{} transcripts were discovered and none could be read",
                coverage.files_discovered
            ),
        });
    }

    Ok(SessionFacts {
        coverage,
        prompts: analyzer.finish(options.with_text),
    })
}

/// Refuse to report rates the input does not support.
///
/// Below the floor the honest answer is that the question was not answered.
/// A biased number is worse than a withheld one because it is actionable.
pub fn require_coverage(coverage: &Coverage, floor: f64) -> Result<()> {
    match coverage.authorship_ratio() {
        None => Err(Error::SessionCoverageBelowFloor {
            observed: 0.0,
            floor,
            message: "no turn in the window was attributed to a person".into(),
        }),
        Some(ratio) if ratio < floor => Err(Error::SessionCoverageBelowFloor {
            observed: ratio,
            floor,
            message: format!(
                "{} of {} turns the runtime attributed to a person carried a prompt source this binary recognises",
                coverage
                    .user_turns_by_authorship
                    .get(Authorship::Authored.as_str())
                    .copied()
                    .unwrap_or(0),
                coverage
                    .user_turns_by_authorship
                    .get(Authorship::Authored.as_str())
                    .copied()
                    .unwrap_or(0)
                    + coverage
                        .user_turns_by_authorship
                        .get(Authorship::SourceUnrecognised.as_str())
                        .copied()
                        .unwrap_or(0)
            ),
        }),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus(files: &[(&str, &str)]) -> (tempfile::TempDir, SessionConfig) {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let config = SessionConfig {
            roots: vec![dir.path().to_string_lossy().into_owned()],
            min_block_chars: 20,
            coverage_floor: 0.95,
        };
        (dir, config)
    }

    fn authored(session: &str, uuid: &str, ts: &str, text: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{session}","version":"2.1.246","origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":{}}}}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    const STANDING: &str = "resolve the root cause rather than patching the symptom";

    #[test]
    fn a_paragraph_across_two_sessions_surfaces_with_its_citations() {
        let (_dir, config) = corpus(&[
            (
                "a.jsonl",
                &authored("s1", "u1", "2026-08-01T00:00:00Z", STANDING),
            ),
            (
                "b.jsonl",
                &authored("s2", "u2", "2026-08-02T00:00:00Z", STANDING),
            ),
        ]);

        let facts = collect(&config, &CollectOptions::default()).unwrap();

        assert_eq!(facts.prompts.authored_prompts, 2);
        let block = &facts.prompts.repeated_blocks[0];
        assert_eq!(block.sessions, 2);
        assert_eq!(block.citations.len(), 2);
        assert!(block.citations.iter().all(|c| c.file.is_absolute()));
        assert!(block.text.is_none());
    }

    #[test]
    fn since_excludes_older_records() {
        let (_dir, config) = corpus(&[
            (
                "a.jsonl",
                &authored("s1", "u1", "2026-08-01T00:00:00Z", STANDING),
            ),
            (
                "b.jsonl",
                &authored("s2", "u2", "2026-08-02T00:00:00Z", STANDING),
            ),
        ]);
        let options = CollectOptions {
            since: Some("2026-08-02T00:00:00Z".parse().unwrap()),
            ..CollectOptions::default()
        };

        let facts = collect(&config, &options).unwrap();

        assert_eq!(facts.prompts.authored_prompts, 1);
        assert!(facts.prompts.repeated_blocks.is_empty());
    }

    #[test]
    fn an_empty_corpus_is_not_an_error_and_reports_no_coverage() {
        let (_dir, config) = corpus(&[]);
        let facts = collect(&config, &CollectOptions::default()).unwrap();
        assert_eq!(facts.coverage.files_discovered, 0);
        assert_eq!(facts.coverage.authorship_ratio(), None);
    }

    #[test]
    fn an_empty_corpus_withholds_rates_rather_than_reporting_zero() {
        let (_dir, config) = corpus(&[]);
        let facts = collect(&config, &CollectOptions::default()).unwrap();
        let err = require_coverage(&facts.coverage, config.coverage_floor).unwrap_err();
        assert_eq!(
            err.code(),
            crate::error::ErrorCode::SessionCoverageBelowFloor
        );
    }

    #[test]
    fn unrecognised_prompt_sources_pull_coverage_below_the_floor() {
        let odd = r#"{"type":"user","uuid":"u","timestamp":"2026-08-01T00:00:00Z","sessionId":"s","origin":{"kind":"human"},"promptSource":"shipped-tomorrow","message":{"content":"hello"}}"#;
        let (_dir, config) = corpus(&[(
            "a.jsonl",
            &format!(
                "{}\n{odd}\n",
                authored("s", "u1", "2026-08-01T00:00:00Z", STANDING)
            ),
        )]);

        let facts = collect(&config, &CollectOptions::default()).unwrap();

        assert_eq!(facts.coverage.authorship_ratio(), Some(0.5));
        assert!(require_coverage(&facts.coverage, config.coverage_floor).is_err());
    }

    #[test]
    fn unconsumed_record_types_are_reported_not_dropped() {
        let (_dir, config) = corpus(&[(
            "a.jsonl",
            &format!(
                "{}\n{}\n",
                authored("s", "u1", "2026-08-01T00:00:00Z", STANDING),
                r#"{"type":"worktree-state","uuid":"u2","timestamp":"2026-08-01T00:00:01Z","sessionId":"s"}"#
            ),
        )]);

        let facts = collect(&config, &CollectOptions::default()).unwrap();

        assert_eq!(
            facts.coverage.record_types_unconsumed.get("worktree-state"),
            Some(&1)
        );
        assert!(require_coverage(&facts.coverage, config.coverage_floor).is_ok());
    }
}
