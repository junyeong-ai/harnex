//! Consumer detection — find every file referencing a slug under the
//! anchored working directory.
//!
//! Pluggable via the [`ConsumerDetector`] trait. Two built-in strategies:
//! - [`GrepConsumerDetector`] — fast, works without nodex; matches the
//!   `pattern` (after `{slug}` substitution) against file contents.
//! - [`GraphBacklinksConsumerDetector`] — precise, requires nodex on PATH;
//!   queries `nodex query backlinks <node_id>` where `node_id` derives
//!   from `pattern.replace("{slug}", slug)`.
//!
//! Detectors are anchored to a working directory at construction time —
//! the `find_consumers` method only takes a slug. [`consumer_detector_for`]
//! is the factory the CLI uses; it dispatches on the declared strategy
//! string.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::ConsumerDetectorDecl;
use crate::error::{Error, Result};
use crate::graph::{DefaultNodexRunner, NodexClient};

/// Strategy for surfacing the files that reference a given slug.
pub trait ConsumerDetector: Send + Sync {
    fn kind(&self) -> &str;
    fn strategy(&self) -> &str;
    /// Return every file (relative to the anchored working directory)
    /// that references `slug` per this detector's strategy.
    fn find_consumers(&self, slug: &str) -> Result<Vec<PathBuf>>;
}

/// Closed set of supported consumer detection strategies. Adding a variant
/// requires updating [`from_str`], [`as_str`], [`ALL`], and the match in
/// [`consumer_detector_for`] — all enforced at compile time via exhaustive
/// `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerStrategy {
    Grep,
    GraphBacklinks,
}

impl ConsumerStrategy {
    pub const ALL: &'static [Self] = &[Self::Grep, Self::GraphBacklinks];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "grep" => Self::Grep,
            "graph-backlinks" => Self::GraphBacklinks,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grep => "grep",
            Self::GraphBacklinks => "graph-backlinks",
        }
    }
}

const SKIP_DIRS: &[&str] = &["target", "node_modules", ".git", ".harness"];
const SKIP_EXTS: &[&str] = &[
    "exe", "bin", "so", "dylib", "dll", "wasm", "png", "jpg", "jpeg", "gif", "pdf", "zip", "gz",
    "tar", "lock",
];

/// Filesystem-grep strategy. Reads every text file under the anchored
/// working directory (skipping common build / binary surfaces) and
/// checks for the substituted pattern.
pub struct GrepConsumerDetector {
    decl: ConsumerDetectorDecl,
    working_dir: PathBuf,
}

impl GrepConsumerDetector {
    pub fn new(decl: ConsumerDetectorDecl, working_dir: PathBuf) -> Self {
        Self { decl, working_dir }
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
        for entry in WalkDir::new(&self.working_dir) {
            // A walk error (e.g. an unreadable directory) must surface, NOT
            // be dropped: silently skipping a subtree could miss a file that
            // references the slug, producing a false `NoConsumers` signal and
            // retiring a still-referenced artifact. Fail loudly instead.
            let entry = entry.map_err(|e| Error::IoFailure {
                path: e
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.working_dir.clone()),
                source: e
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("directory walk failed")),
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let relative = path.strip_prefix(&self.working_dir).unwrap_or(path);
            if path.components().any(|c| {
                c.as_os_str()
                    .to_str()
                    .is_some_and(|s| SKIP_DIRS.contains(&s))
            }) {
                continue;
            }
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SKIP_EXTS.contains(&e))
            {
                continue;
            }
            if excludes.iter().any(|p| p.matches_path(relative)) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(path)
                && content.contains(&needle)
            {
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
/// to `working_dir`. Fails explicitly when `graph-backlinks` is selected
/// but nodex is missing — never silently falls back to grep.
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
        ))),
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
