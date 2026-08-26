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
//! - Never claim a revert it cannot see. [`CommitOutcome::reverted_by`] finds
//!   the line `git revert` writes and nothing else; a change undone by hand
//!   carries no such line and is invisible here.
//! - Never reach the network. Every command reads the local object database.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The line `git revert` writes into the message it generates.
const REVERT_TRAILER: &str = "This reverts commit ";

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
    /// Oldest first, as the window observed them.
    pub commits: Vec<CommitOutcome>,
    /// Counts keyed by [`CommitFate::as_str`].
    pub by_fate: BTreeMap<String, usize>,
}

/// Ask the project's repository what became of these commits.
///
/// `None` when the path is not a git work tree, which is the ordinary case for
/// most of a machine's transcripts and not a failure.
pub fn survey(project: &Path, observed: &[String]) -> Result<Option<RepositoryFacts>> {
    if run(project, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Ok(None);
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

    Ok(Some(RepositoryFacts {
        head,
        commits,
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
    let mut child = Command::new("git")
        .args(["cat-file", "--batch-check"])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git cat-file --batch-check spawn: {e}"),
        })?;
    let query: String = observed
        .iter()
        .map(|s| format!("{s}^{{commit}}\n"))
        .collect();
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(query.as_bytes())
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git cat-file --batch-check write: {e}"),
        })?;
    let output = child
        .wait_with_output()
        .map_err(|e| Error::CheckGitFailure {
            message: format!("git cat-file --batch-check wait: {e}"),
        })?;

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
        for reverted in body.match_indices(REVERT_TRAILER).filter_map(|(at, _)| {
            let rest = &body[at + REVERT_TRAILER.len()..];
            let candidate: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
            (candidate.len() == 40).then_some(candidate)
        }) {
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
