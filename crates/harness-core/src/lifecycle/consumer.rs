//! Consumer detection — find every file referencing a slug under the
//! anchored working directory.
//!
//! Pluggable via the [`ConsumerDetector`] trait. Two built-in strategies:
//! - [`GrepConsumerDetector`] — works without nodex; matches the `pattern`
//!   (after `{slug}` substitution) against the contents of every file the
//!   project owns.
//! - [`GraphBacklinksConsumerDetector`] — precise, requires nodex on PATH;
//!   queries `nodex query backlinks <node_id>` where `node_id` derives
//!   from `pattern.replace("{slug}", slug)`.
//!
//! Detectors are anchored to a working directory at construction time —
//! the `find_consumers` method only takes a slug. [`consumer_detector_for`]
//! is the factory the CLI uses; it dispatches on the declared strategy
//! string.

use std::path::{Path, PathBuf};

use crate::config::ConsumerDetectorDecl;
use crate::error::{Error, Result};
use crate::git;
use crate::graph::{DefaultNodexRunner, NodexClient};
use crate::wire_enum::wire_enum;

/// Strategy for surfacing the files that reference a given slug.
pub trait ConsumerDetector: Send + Sync {
    fn kind(&self) -> &str;
    fn strategy(&self) -> &str;
    /// Return every file (relative to the anchored working directory)
    /// that references `slug` per this detector's strategy.
    fn find_consumers(&self, slug: &str) -> Result<Vec<PathBuf>>;
}

wire_enum! {
    /// Closed set of supported consumer detection strategies. Adding a variant
    /// requires updating [`from_str`], [`as_str`], [`ALL`], and the match in
    /// [`consumer_detector_for`] — all enforced at compile time via exhaustive
    /// `match`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConsumerStrategy {
        Grep => "grep",
        GraphBacklinks => "graph-backlinks",
    }
}

/// Grep strategy. Reads every plain file the project owns — as its own
/// ignore files define it, through [`git::owned_files`] — and checks each
/// for the substituted pattern. A directory walk would count a match in
/// build output, a fixture tree or a sibling checkout on this machine as a
/// consumer, and the count would differ between two clones of one commit;
/// a project that is not a repository is refused rather than walked. What
/// the project owns but reads no rule from — a lockfile, a ledger — is
/// `exclude_globs`' to name; no extension list stands in for it.
pub struct GrepConsumerDetector {
    decl: ConsumerDetectorDecl,
    working_dir: PathBuf,
    /// The plain files the project owns, listed once: the set is the same
    /// for every slug, and a sweep asks for one slug per artifact.
    files: Vec<PathBuf>,
}

impl GrepConsumerDetector {
    /// Lists the project's files up front, so a project that cannot be
    /// listed is known before any slug is asked about.
    pub fn new(decl: ConsumerDetectorDecl, working_dir: PathBuf) -> Result<Self> {
        let owned = git::owned_files(&working_dir, &[])
            .map_err(|git::Failure(message)| Error::LifecycleGitFailure { message })?;
        let mut files = Vec::with_capacity(owned.len());
        for path in owned {
            // The listing names more than plain files, and only a plain file
            // references anything here: a tracked file deleted from the tree,
            // a submodule's gitlink and a nested repository's directory hold
            // nothing of this project's, and a symlink's content is its link
            // text — its target may be outside the project, or a file already
            // counted under its own path.
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() => files.push(path),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::IoFailure { path, source: e }),
            }
        }
        Ok(Self {
            decl,
            working_dir,
            files,
        })
    }
}

impl ConsumerDetector for GrepConsumerDetector {
    fn kind(&self) -> &str {
        &self.decl.kind
    }
    fn strategy(&self) -> &str {
        "grep"
    }

    fn find_consumers(&self, slug: &str) -> Result<Vec<PathBuf>> {
        let needle = self.decl.pattern.replace("{slug}", slug);
        let excludes: Vec<glob::Pattern> = self
            .decl
            .exclude_globs
            .iter()
            .filter_map(|g| glob::Pattern::new(&g.replace("{slug}", slug)).ok())
            .collect();
        let mut out = Vec::new();
        for path in &self.files {
            let relative = path.strip_prefix(&self.working_dir).unwrap_or(path);
            if excludes.iter().any(|p| p.matches_path(relative)) {
                continue;
            }
            // Read bytes and decode lossily: a file that cannot be read must
            // fail rather than be skipped — a skipped consumer is a false
            // `NoConsumers` signal — while one stray byte neither aborts the
            // sweep nor hides a match.
            let bytes = std::fs::read(path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            if String::from_utf8_lossy(&bytes).contains(&needle) {
                out.push(relative.to_path_buf());
            }
        }
        Ok(out)
    }
}

/// Nodex-backlinks strategy. Substitutes `{slug}` into `decl.pattern` to
/// derive a node ID, then issues `nodex query backlinks <id>`. Returns
/// the `path` of every referencing node.
pub struct GraphBacklinksConsumerDetector {
    decl: ConsumerDetectorDecl,
    client: NodexClient<DefaultNodexRunner>,
}

impl GraphBacklinksConsumerDetector {
    pub fn new(decl: ConsumerDetectorDecl, client: NodexClient<DefaultNodexRunner>) -> Self {
        Self { decl, client }
    }
}

impl ConsumerDetector for GraphBacklinksConsumerDetector {
    fn kind(&self) -> &str {
        &self.decl.kind
    }
    fn strategy(&self) -> &str {
        "graph-backlinks"
    }

    fn find_consumers(&self, slug: &str) -> Result<Vec<PathBuf>> {
        let node_id = self.decl.pattern.replace("{slug}", slug);
        let backlinks = self.client.backlinks(&node_id)?;
        // Every backlink IS a consumer. A node that references the slug but
        // carries no `path` must still count — dropping it would undercount
        // consumers into a false `NoConsumers` signal. Identify pathless
        // backlinks by their node id instead of discarding them.
        Ok(backlinks
            .into_iter()
            .map(|n| n.path.unwrap_or_else(|| PathBuf::from(n.id)))
            .collect())
    }
}

/// Build the appropriate detector for the declared strategy, anchored
/// to `working_dir`. Fails explicitly when the detector cannot read its
/// corpus — `graph-backlinks` without nodex, `grep` outside a repository —
/// and never falls back to the other strategy.
pub fn consumer_detector_for(
    decl: ConsumerDetectorDecl,
    working_dir: &Path,
) -> Result<Box<dyn ConsumerDetector>> {
    let strategy = ConsumerStrategy::from_str(&decl.strategy).ok_or_else(|| {
        Error::LifecycleConsumerStrategyUnknown {
            strategy: decl.strategy.clone(),
        }
    })?;
    match strategy {
        ConsumerStrategy::Grep => Ok(Box::new(GrepConsumerDetector::new(
            decl,
            working_dir.to_path_buf(),
        )?)),
        ConsumerStrategy::GraphBacklinks => {
            let client =
                NodexClient::anchored(working_dir).ok_or_else(|| Error::GraphSpawnFailure {
                    message:
                        "nodex binary not found on PATH; graph-backlinks strategy requires nodex"
                            .into(),
                })?;
            Ok(Box::new(GraphBacklinksConsumerDetector::new(decl, client)))
        }
    }
}

#[cfg(test)]
mod strategy_tests {
    use super::ConsumerStrategy;

    #[test]
    fn from_str_round_trips_every_variant() {
        for s in ConsumerStrategy::ALL {
            assert_eq!(ConsumerStrategy::from_str(s.as_str()), Some(*s));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(ConsumerStrategy::from_str("nope"), None);
    }
}
