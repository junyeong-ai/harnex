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
//! - Never identify a turn by its wording. Where the runtime marks only part
//!   of a population — interruptions are the standing example, marked on 216
//!   of 394 — the marked subset is published as a floor carrying its measured
//!   coverage, and the remainder is left to a reader of the transcript.
//!   Matching the runtime's wording to close the gap reports zero the day the
//!   wording moves.
//! - Never write into a project. Reading is the whole of it; the one file
//!   written anywhere is `repository`'s scratch query for git, outside any
//!   project tree and through `path_guard`.
//! - Never reach the network. Every input is a local file.

pub mod baseline;
pub mod discovery;
pub mod harness;
pub mod intervention;
pub mod prompt;
pub mod record;
pub mod repository;
pub mod rework;
pub mod submission;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::config::SessionConfig;
use crate::error::{Error, Result};

pub use baseline::{
    Baseline, BaselineDiff, BaselineLedger, HarnessChange, Measured, Measurement, MetricDelta,
    SessionMetric,
};
pub use harness::{
    AssetInvocation, BlockedCall, DenialGroup, HarnessFacts, HookCost, RuleLoadGroup,
};
pub use intervention::{Intervention, InterventionFacts, InterventionKind};
pub use prompt::{PromptFacts, RepeatedBlock, Repetition};
pub use record::{Authorship, Citation, Compaction, Coverage, TokenUse, ToolUse};
pub use repository::{CommitFate, CommitOutcome, HarnessState, RepositoryFacts};
pub use rework::{PostCommitReedit, ReworkFacts};
pub use submission::{Submission, SubmissionIndex, SubmissionWindow, systematic_sample};

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
    /// Read only records made under this directory or below it. Absent reads
    /// every project on the machine.
    pub project: Option<PathBuf>,
    /// Read only records this session wrote. A subagent's transcript is a
    /// separate file carrying its parent's id, so this keeps a session whole
    /// rather than splitting the work it delegated away from it.
    pub session: Option<String>,
    /// Include the instruction-by-instruction list. Off by default: it is one
    /// entry per instruction where the rest of the result is one entry per
    /// finding, and most callers want the findings.
    pub with_submissions: bool,
}

/// Counts and citations for one window of the corpus.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionFacts {
    pub coverage: Coverage,
    pub prompts: PromptFacts,
    /// Empty unless [`CollectOptions::with_submissions`] asked for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submissions: Vec<Submission>,
    /// Where the session's context was compacted, oldest first.
    pub compactions: Vec<Compaction>,
    /// What the window spent, whether or not the caller asked for the
    /// instruction list.
    pub tokens: TokenUse,
    /// Tool calls across the window, by tool, with the calls that came back an
    /// error. Read beside `harness.denials`, which groups by the same names:
    /// friction is as much a function of which tool the work goes through as of
    /// how broad a rule is, and a call the harness refused is counted there
    /// rather than here.
    pub tools: BTreeMap<String, ToolUse>,
    /// What became of the commits the window produced. Present only for a
    /// window scoped to a project, and only when that project is a git work
    /// tree — nothing else can be asked what survived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryFacts>,
    pub interventions: InterventionFacts,
    pub rework: ReworkFacts,
    pub harness: HarnessFacts,
}

/// One session's transcripts as a single sequence, each file's own order intact.
///
/// A transcript is append-ordered, so its order is the order things happened.
/// Its timestamps are not: measured over the local corpus, 2.27% of adjacent
/// records carry a timestamp earlier than the one before them (68% of those an
/// attachment written behind the turn it belongs to) and 5.55% carry the same
/// one. Sorting the concatenation would rewrite that order — including for a
/// session with no subagent, where there is nothing to interleave at all.
///
/// So each file is consumed in its own order and only the choice between files
/// is made by time. A tie goes to the earlier stream, and `discovery` hands
/// them over lexicographically, so a run is reproducible.
fn interleave_by_time(streams: Vec<Vec<record::Record>>) -> Vec<record::Record> {
    let total: usize = streams.iter().map(Vec::len).sum();
    let mut streams: Vec<_> = streams
        .into_iter()
        .map(|records| records.into_iter().peekable())
        .collect();

    let mut merged = Vec::with_capacity(total);
    while let Some((_, next)) = streams
        .iter_mut()
        .enumerate()
        .filter_map(|(i, s)| s.peek().map(|r| (r.citation().timestamp, i)))
        .min()
    {
        merged.push(streams[next].next().expect("the stream that was peeked"));
    }
    merged
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
    let mut index = submission::SubmissionIndex::new();
    let mut prompts = prompt::PromptAnalyzer::new(config.min_block_chars);
    let mut submissions = submission::SubmissionAnalyzer::new();
    let mut interventions = intervention::InterventionAnalyzer::new();
    let mut compactions: Vec<Compaction> = Vec::new();
    let mut tokens = TokenUse::default();
    let mut commits: Vec<String> = Vec::new();
    let mut tools: BTreeMap<String, ToolUse> = BTreeMap::new();
    let mut sessions: BTreeSet<String> = BTreeSet::new();
    let mut rework = rework::ReworkAnalyzer::new();
    let mut harness = harness::HarnessAnalyzer::new();
    // A uuid is one event. The runtime replays a session's records into other
    // files — into a fork's transcript, into each subagent dispatched at once,
    // and into a resumed session under a new id — so the same event reaches
    // this loop from more than one file and from more than one session.
    let mut seen: HashSet<String> = HashSet::new();
    // A message is written to one transcript. Its records are several — one
    // per block it produced — so within that file they are distinct events;
    // another file reporting the same message is reporting copies of them,
    // whatever uuids it gave them.
    let mut wrote: HashMap<String, std::path::PathBuf> = HashMap::new();
    let mut in_window: BTreeSet<std::path::PathBuf> = BTreeSet::new();

    // A session's transcripts are read together and interleaved. A subagent
    // writes its own file under its parent's session, so reading files
    // independently leaves its work outside every instruction the parent gave.
    for group in discovery::group_by_session(&files) {
        let mut streams = Vec::with_capacity(group.len());
        for path in &group {
            // Counters are mutated as the file is read, so a file that fails
            // partway would leave the coverage of records the caller then
            // discards. The run must not be able to claim what it did not use.
            let before = coverage.clone();
            match record::read_transcript(
                path,
                record::Window {
                    since: options.since,
                    project: options.project.as_deref(),
                    session: options.session.as_deref(),
                },
                &mut coverage,
            ) {
                Ok(r) => streams.push(r),
                Err(_) => {
                    coverage = before;
                    coverage.files_unreadable += 1;
                }
            }
        }
        // Once, before anything reads them: an analyser handed the unfiltered
        // list would count a copy its neighbours had already discarded.
        let mut records = interleave_by_time(streams);
        records.retain(|rec| {
            let first = seen.insert(rec.citation().uuid.clone());
            if !first {
                coverage.records_duplicated += 1;
            }
            first
        });
        records.retain(|rec| {
            let record::Record::Assistant(turn) = rec else {
                return true;
            };
            let Some(message) = &turn.message else {
                return true;
            };
            let first = wrote
                .entry(message.clone())
                .or_insert_with(|| turn.citation.file.clone());
            let here = *first == turn.citation.file;
            if !here {
                coverage.records_duplicated += 1;
            }
            here
        });
        for rec in &records {
            sessions.insert(rec.citation().session.clone());
            in_window.insert(rec.citation().file.clone());
            let mut assigned = None;
            if let record::Record::User(turn) = rec {
                assigned = index.assign(turn);
                if let Some(id) =
                    assigned.filter(|_| turn.authorship == record::Authorship::Authored)
                {
                    prompts.observe(turn, id);
                }
                interventions.observe(turn);
            }
            match rec {
                record::Record::Compaction(c) => compactions.push(c.clone()),
                record::Record::Assistant(turn) => {
                    tokens.add(turn.tokens);
                    for action in &turn.actions {
                        tools.entry(action.tool.clone()).or_default().calls += 1;
                    }
                }
                record::Record::User(turn) => {
                    if let Some(sha) = &turn.commit {
                        commits.push(sha.clone());
                    }
                    if let Some(tool) = &turn.failed_tool {
                        tools.entry(tool.clone()).or_default().failed += 1;
                    }
                }
                _ => {}
            }
            submissions.observe(rec, assigned);
            harness.observe(rec);
        }
        rework.observe(&records);
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

    coverage.sessions = sessions.len();
    coverage.files_in_window = in_window.len();
    let span = coverage.observed_from.zip(coverage.observed_to);
    let repository = match &options.project {
        Some(project) => repository::survey(project, &commits, span)?,
        None => None,
    };

    let mut submissions = match options.with_submissions {
        true => submissions.finish(options.with_text),
        false => Vec::new(),
    };
    if let (Some(project), Some(repository)) = (&options.project, &repository) {
        attribute_committed_paths(project, repository, &mut submissions)?;
    }

    Ok(SessionFacts {
        coverage,
        prompts: prompts.finish(index.count(), options.with_text),
        submissions,
        interventions: interventions.finish(),
        compactions: {
            compactions.sort_by_key(|c| c.citation.timestamp);
            compactions
        },
        tokens,
        tools,
        repository,
        rework: rework.finish(),
        harness: harness.finish(options.with_text),
    })
}

/// Give each instruction the paths its commits changed.
///
/// The transcript abbreviates a commit and the repository resolved it, so the
/// join runs through that resolution rather than through the abbreviation: two
/// windows of the same project abbreviate to different widths, and a prefix is
/// not a commit until git says which one.
fn attribute_committed_paths(
    project: &std::path::Path,
    repository: &repository::RepositoryFacts,
    submissions: &mut [submission::Submission],
) -> Result<()> {
    let resolved: BTreeMap<&str, &str> = repository
        .commits
        .iter()
        .filter_map(|c| Some((c.sha.as_str(), c.resolved.as_deref()?)))
        .collect();
    if resolved.is_empty() {
        return Ok(());
    }
    let shas: Vec<String> = resolved.values().map(|s| s.to_string()).collect();
    let touched = repository::paths_touched(project, &shas)?;
    for held in submissions {
        let paths: BTreeSet<&std::path::Path> = held
            .commits
            .iter()
            .filter_map(|sha| resolved.get(sha.as_str()))
            .filter_map(|full| touched.get(*full))
            .flatten()
            .map(std::path::PathBuf::as_path)
            .collect();
        held.committed = paths
            .into_iter()
            .map(std::path::Path::to_path_buf)
            .collect();
    }
    Ok(())
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
            min_support: 1,
            baseline_path: dir.path().join("baselines.jsonl"),
            submission_sample: None,
            harness_paths: Vec::new(),
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

    fn stream(uuid_and_second: &[(&str, i64)]) -> Vec<record::Record> {
        uuid_and_second
            .iter()
            .map(|(uuid, second)| {
                record::Record::RuleLoad(record::RuleLoad {
                    citation: record::Citation {
                        session: "s1".into(),
                        file: PathBuf::from("/corpus/s1.jsonl"),
                        uuid: (*uuid).into(),
                        timestamp: Timestamp::from_second(*second).unwrap(),
                    },
                    path: PathBuf::from("/repo/CLAUDE.md"),
                    chars: 1,
                })
            })
            .collect()
    }

    fn order(records: &[record::Record]) -> Vec<&str> {
        records.iter().map(|r| r.citation().uuid.as_str()).collect()
    }

    #[test]
    fn a_transcripts_own_order_survives_a_timestamp_that_goes_backwards() {
        let merged = interleave_by_time(vec![stream(&[("a", 30), ("b", 10), ("c", 20)])]);
        assert_eq!(order(&merged), ["a", "b", "c"]);
    }

    #[test]
    fn a_subagents_records_land_between_the_parents_by_time() {
        let merged = interleave_by_time(vec![
            stream(&[("p1", 10), ("p2", 40)]),
            stream(&[("s1", 20), ("s2", 30)]),
        ]);
        assert_eq!(order(&merged), ["p1", "s1", "s2", "p2"]);
    }

    #[test]
    fn records_sharing_a_timestamp_resolve_to_the_earlier_transcript() {
        let merged = interleave_by_time(vec![stream(&[("p", 10)]), stream(&[("s", 10)])]);
        assert_eq!(order(&merged), ["p", "s"]);
    }

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

        assert_eq!(facts.prompts.authored_turns, 2);
        let block = &facts.prompts.across_sessions.as_ref().unwrap().blocks[0];
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

        assert_eq!(facts.prompts.authored_turns, 1);
        assert!(
            facts.prompts.across_sessions.is_none(),
            "one session left in the window cannot repeat across sessions"
        );
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
