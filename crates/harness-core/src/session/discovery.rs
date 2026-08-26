//! # discovery — declared roots to absolute transcript paths
//!
//! Transcript roots are machine-global (`~/.claude/projects`), which is the
//! one place this crate reaches outside `${CLAUDE_PROJECT_DIR}`. That path
//! therefore lives in `harness.toml` and never in source: a built-in default
//! would put one machine's layout into a binary shipped to other machines.
//!
//! Every path this module returns is absolute. Claude Code names its project
//! directories after the encoded working directory, so they begin with `-`
//! (`~/.claude/projects/-Users-me-work-repo/`). A relative path to one parses
//! as a flag in any tool that takes them, and the failure is silence rather
//! than an error — two measurement passes during this module's design returned
//! empty output that way.
//!
//! The environment is read once, in [`discover`]; [`expand_home`] is a pure
//! function of its arguments so the expansion rule is testable without a
//! process-wide mutation this crate forbids.
//!
//! ## What this module refuses to do
//!
//! - Never return a relative path.
//! - Never treat an unreadable root as an empty one. A root that cannot be
//!   walked is an error; a corpus that is genuinely empty is an empty list,
//!   and the two must not arrive looking alike.
//! - Never expand a home reference it cannot resolve. `~` without `HOME` is
//!   reported, not silently left literal to match nothing.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const TRANSCRIPT_EXTENSION: &str = "jsonl";

/// Resolve a leading `~` against `home`.
///
/// Only the bare `~` and a `~/` prefix are expanded — `~other-user` is a shell
/// convention this crate has no business reimplementing, and is left alone.
fn expand_home(raw: &str, home: Option<&OsStr>) -> Result<PathBuf> {
    let rest = if raw == "~" {
        ""
    } else if let Some(r) = raw.strip_prefix("~/") {
        r
    } else {
        return Ok(PathBuf::from(raw));
    };
    let home = home.ok_or_else(|| Error::SessionRootUnreadable {
        path: PathBuf::from(raw),
        message: "root begins with '~' but HOME is not set".into(),
    })?;
    Ok(Path::new(home).join(rest))
}

/// Every transcript under `roots`, absolute and deduplicated, in stable order.
///
/// Roots may overlap; a file reached through two of them appears once. Order is
/// lexicographic so a run is reproducible and a diff between runs is about the
/// corpus rather than about directory iteration.
pub fn discover(roots: &[String]) -> Result<Vec<PathBuf>> {
    discover_with_home(roots, std::env::var_os("HOME").as_deref())
}

fn discover_with_home(roots: &[String], home: Option<&OsStr>) -> Result<Vec<PathBuf>> {
    let mut found = BTreeSet::new();

    for raw in roots {
        let root = expand_home(raw, home)?;
        let root = root
            .canonicalize()
            .map_err(|e| Error::SessionRootUnreadable {
                path: root.clone(),
                message: e.to_string(),
            })?;

        for entry in walkdir::WalkDir::new(&root).follow_links(false) {
            let entry = entry.map_err(|e| Error::SessionRootUnreadable {
                path: e.path().unwrap_or(&root).to_path_buf(),
                message: e.to_string(),
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(OsStr::to_str) == Some(TRANSCRIPT_EXTENSION) {
                found.insert(path.to_path_buf());
            }
        }
    }
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(paths: &[&Path]) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn discovers_transcripts_recursively_as_absolute_paths() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("-Users-me-repo").join("subagents");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("-Users-me-repo/a.jsonl"), "").unwrap();
        std::fs::write(nested.join("agent-1.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("-Users-me-repo/notes.md"), "").unwrap();

        let found = discover_with_home(&roots(&[dir.path()]), None).unwrap();

        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn overlapping_roots_yield_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("a.jsonl"), "").unwrap();

        let found = discover_with_home(&roots(&[dir.path(), &inner]), None).unwrap();

        assert_eq!(found.len(), 1);
    }

    #[test]
    fn missing_root_is_an_error_not_an_empty_corpus() {
        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("not-here");
        let err = discover_with_home(&roots(&[&absent]), None).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::SessionRootUnreadable);
    }

    #[test]
    fn empty_root_is_an_empty_corpus_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            discover_with_home(&roots(&[dir.path()]), None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn home_relative_root_resolves_against_the_given_home() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        std::fs::write(dir.path().join("logs/a.jsonl"), "").unwrap();

        let found =
            discover_with_home(&["~/logs".to_string()], Some(dir.path().as_os_str())).unwrap();

        assert_eq!(found.len(), 1);
    }

    #[test]
    fn home_relative_root_without_home_is_reported() {
        let err = discover_with_home(&["~/logs".to_string()], None).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::SessionRootUnreadable);
    }

    #[test]
    fn a_tilde_inside_a_path_is_not_a_home_reference() {
        let expanded = expand_home("/tmp/a~b", None).unwrap();
        assert_eq!(expanded, PathBuf::from("/tmp/a~b"));
    }
}
