//! # repository — what became of the commits the work produced
//!
//! Every other fact here comes from a transcript, which records what was asked
//! and what the agent did. Whether any of it survived is a different question
//! and the transcript cannot answer it: the repository can, and only for the
//! project a window was scoped to.
//!
//! The transcript abbreviates a commit to seven or nine characters — measured,
//! 2,071 at nine and 241 at seven — so nothing here resolves one itself. Git is
//! asked, because a prefix is not a commit until a repository says which one.
//!
//! ## What this module refuses to do
//!
//! - Never read [`CommitFate::Unreachable`] as work undone. A rebase, an
//!   amend, a reset and an unmerged branch all land there, and a repository
//!   that squash-merges puts every feature commit there by design.
//! - Never present the observed commits as every commit. The transcript
//!   records one only sometimes — 29 of git's 42 over this project — so the
//!   count git gives for the same span rides beside it rather than being
//!   assumed equal to it.
//! - Never claim a revert it cannot see. [`CommitOutcome::reverted_by`] finds
//!   the line `git revert` writes and nothing else; a change undone by hand
//!   carries no such line and is invisible here.
//! - Never write into a project. The one file this module creates is the query
//!   git reads on stdin, in a scratch directory outside any project tree and
//!   through `path_guard` like every other write in the crate.
//! - Never reach the network. Every command reads the local object database.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::path_guard;

/// The line `git revert` writes into the message it generates.
///
/// Matched at the start of a line, because the same words quoted inside a
/// message body are somebody talking about a revert rather than one.
const REVERT_TRAILER: &str = "This reverts commit ";

/// Object id widths git uses: SHA-1 today, SHA-256 where a repository was
/// created with it. Hardcoding the first would leave `reverted_by` empty on
/// the second and say nothing about why.
const OBJECT_ID_WIDTHS: &[usize] = &[40, 64];

/// Where a commit stands relative to the branch that is checked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitFate {
    /// An ancestor of `HEAD`.
    Reachable,
    /// In the object database, but `HEAD` does not reach it. Says history was
    /// rewritten or the branch was never merged; says nothing about whether
    /// the work was undone.
    Unreachable,
    /// This repository could not resolve the abbreviation the transcript
    /// recorded: a commit from another clone, one collected away, or a prefix
    /// too short to name one. Git answers all three the same way, so they are
    /// one answer here rather than three guesses.
    Missing,
}

impl CommitFate {
    pub const ALL: &'static [Self] = &[Self::Reachable, Self::Unreachable, Self::Missing];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "reachable" => Self::Reachable,
            "unreachable" => Self::Unreachable,
            "missing" => Self::Missing,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reachable => "reachable",
            Self::Unreachable => "unreachable",
            Self::Missing => "missing",
        }
    }
}

/// One commit the work produced, and where it stands now.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommitOutcome {
    /// The abbreviation the transcript recorded.
    pub sha: String,
    /// What the repository resolved it to, when it could.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    pub fate: String,
    /// Commits carrying `git revert`'s own trailer for this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reverted_by: Vec<String>,
}

/// What the project's repository says about the window's commits.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RepositoryFacts {
    pub head: String,
    /// Oldest first, as the window observed them. A floor: the transcript
    /// records a commit only sometimes, so this is what the window saw and not
    /// what the window did.
    pub commits: Vec<CommitOutcome>,
    /// Commits git counts over the same span, which is what the floor above is
    /// a floor against. Absent when the window observed no span. Measured over
    /// this project the transcript held 29 of git's 42, so a rate denominated
    /// in observed commits reads high by however much this gap is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits_in_span: Option<usize>,
    /// Counts keyed by [`CommitFate::as_str`].
    pub by_fate: BTreeMap<String, usize>,
}

/// Ask the project's repository what became of these commits.
///
/// `None` when the path is not a git work tree, which is the ordinary case for
/// most of a machine's transcripts and not a failure.
pub fn survey(
    project: &Path,
    observed: &[String],
    span: Option<(jiff::Timestamp, jiff::Timestamp)>,
) -> Result<Option<RepositoryFacts>> {
    // Three answers, kept apart. A directory that is not there has no history
    // to ask about; git saying "not a work tree" is the same; a git that could
    // not be spawned is a failure, and reporting that as an absent repository
    // would say the project has no history rather than that nothing asked.
    if !project.is_dir() {
        return Ok(None);
    }
    match Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project)
        .output()
    {
        Ok(out) if !out.status.success() => return Ok(None),
        Ok(_) => {}
        Err(e) => {
            return Err(Error::CheckGitFailure {
                message: format!("git rev-parse --is-inside-work-tree spawn: {e}"),
            });
        }
    }
    let head = run(project, &["rev-parse", "HEAD"])?.trim().to_string();
    let resolved = resolve(project, observed)?;
    let reachable: BTreeSet<String> = run(project, &["rev-list", "HEAD"])?
        .lines()
        .map(str::to_string)
        .collect();
    let reverts = reverts(project)?;

    let mut by_fate: BTreeMap<String, usize> = CommitFate::ALL
        .iter()
        .map(|f| (f.as_str().to_string(), 0))
        .collect();
    let mut commits = Vec::with_capacity(observed.len());
    for (sha, full) in observed.iter().zip(resolved) {
        let fate = match &full {
            Some(full) if reachable.contains(full) => CommitFate::Reachable,
            Some(_) => CommitFate::Unreachable,
            None => CommitFate::Missing,
        };
        *by_fate.entry(fate.as_str().to_string()).or_default() += 1;
        commits.push(CommitOutcome {
            reverted_by: full
                .as_ref()
                .and_then(|f| reverts.get(f))
                .cloned()
                .unwrap_or_default(),
            resolved: full,
            sha: sha.clone(),
            fate: fate.as_str().to_string(),
        });
    }

    let commits_in_span = match span {
        Some((from, to)) => Some(
            run(
                project,
                &[
                    "log",
                    &format!("--since={from}"),
                    &format!("--until={to}"),
                    "--format=%H",
                    "HEAD",
                ],
            )?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        ),
        None => None,
    };

    Ok(Some(RepositoryFacts {
        head,
        commits,
        commits_in_span,
        by_fate,
    }))
}

/// Ask git to resolve each abbreviation, in the order given. Git is the only
/// thing that can: a seven-character prefix is not a commit until a repository
/// says which one.
fn resolve(project: &Path, observed: &[String]) -> Result<Vec<Option<String>>> {
    if observed.is_empty() {
        return Ok(Vec::new());
    }
    // The transcript is not a trusted source of revision expressions. Anything
    // that is not an abbreviation resolves to nothing rather than being handed
    // to git, which would otherwise accept `HEAD~1` or a tag name and report
    // whatever it found as the work this window did.
    let abbreviations: Vec<Option<&String>> = observed
        .iter()
        .map(|s| {
            (!s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .then_some(s)
        })
        .collect();
    // The query goes through a file rather than a pipe. Writing every line to
    // a piped stdin before reading stdout deadlocks once git's output fills the
    // kernel buffer and it blocks writing while this blocks writing — measured
    // here between six and eight thousand commits, which one busy project
    // reaches. A file has no such limit and needs no second thread to drain.
    let query: String = abbreviations
        .iter()
        .map(|a| match a {
            // A line git cannot resolve keeps the answer positional without
            // asking it about something the transcript made up.
            Some(sha) => format!("{sha}^{{commit}}\n"),
            None => "\n".to_string(),
        })
        .collect();
    let scratch = tempfile::tempdir().map_err(|e| Error::CheckGitFailure {
        message: format!("git cat-file --batch-check scratch: {e}"),
    })?;
    let query_path = scratch.path().join("commits");
    path_guard::write_atomic(&query_path, query.as_bytes())?;
    let stdin = std::fs::File::open(&query_path).map_err(|e| Error::IoFailure {
        path: query_path.clone(),
        source: e,
    })?;

    let output = Command::new("git")
        .args(["cat-file", "--batch-check"])
        .current_dir(project)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git cat-file --batch-check spawn: {e}"),
        })?
        .wait_with_output()
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git cat-file --batch-check wait: {e}"),
        })?;

    if !output.status.success() {
        return Err(Error::CheckGitFailure {
            message: format!("git cat-file --batch-check exited {}", output.status),
        });
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let out: Vec<Option<String>> = body
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let first = parts.next().unwrap_or_default().to_string();
            (parts.next() == Some("commit")).then_some(first)
        })
        .collect();
    // One line per query is the contract; a short read would drop the tail of
    // the window and report commits it never asked about as gone.
    if out.len() != observed.len() {
        return Err(Error::CheckGitFailure {
            message: format!(
                "git cat-file --batch-check answered {} of {} commits",
                out.len(),
                observed.len()
            ),
        });
    }
    Ok(out)
}

/// Full shas each reverting commit named, keyed by the commit it reverted.
fn reverts(project: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let log = run(
        project,
        &[
            "log",
            "--fixed-strings",
            &format!("--grep={REVERT_TRAILER}"),
            "--format=%H%x00%B%x00",
            "HEAD",
        ],
    )?;
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut fields = log.split('\0');
    while let (Some(sha), Some(body)) = (fields.next(), fields.next()) {
        let sha = sha.trim();
        if sha.is_empty() {
            continue;
        }
        for reverted in body
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix(REVERT_TRAILER))
            .filter_map(|rest| {
                let id: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
                OBJECT_ID_WIDTHS.contains(&id.len()).then_some(id)
            })
        {
            out.entry(reverted).or_default().push(sha.to_string());
        }
    }
    Ok(out)
}

fn run(project: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project)
        .output()
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git {} spawn: {e}", args.join(" ")),
        })?;
    if !output.status.success() {
        return Err(Error::CheckGitFailure {
            message: format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod fate_tests {
    use super::CommitFate;

    #[test]
    fn from_str_round_trips_every_variant() {
        for f in CommitFate::ALL {
            assert_eq!(CommitFate::from_str(f.as_str()), Some(*f));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(CommitFate::from_str("undone"), None);
    }
}
