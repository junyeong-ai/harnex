//! Consumer detection — find every file of the project referencing a slug.
//!
//! [`ConsumerDetector`] lists the project's files once, at construction, and
//! [`ConsumerDetector::find`] answers every slug a sweep asks about in one
//! read of each file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aho_corasick::AhoCorasick;

use crate::config::ConsumerDetectorDecl;
use crate::error::{Error, Result};
use crate::git;

/// Matches each `{slug}`-substituted pattern against every plain file the
/// project owns, as its own ignore files define it through
/// [`git::owned_files`]. A directory walk would count a match in
/// build output, a fixture tree or a sibling checkout on this machine as a
/// consumer, and the count would differ between two clones of one commit;
/// a project that is not a repository is refused rather than walked. What
/// the project owns but reads no rule from — a lockfile, a ledger — is
/// `exclude_globs`' to name; no extension list stands in for it.
pub struct ConsumerDetector {
    working_dir: PathBuf,
    files: Vec<PathBuf>,
}

/// One question to the detector: the files referencing `slug`, as `decl`,
/// the detector declared for its kind, writes it.
pub struct ConsumerAsk<'a> {
    pub decl: &'a ConsumerDetectorDecl,
    pub slug: &'a str,
}

impl ConsumerDetector {
    /// Lists the project's files up front, so a project that cannot be
    /// listed is known before any slug is asked about.
    pub fn new(working_dir: &Path) -> Result<Self> {
        let files = git::owned_files(working_dir, &[])
            .map_err(|git::Failure(message)| Error::LifecycleGitFailure { message })?;
        Ok(Self {
            working_dir: working_dir.to_path_buf(),
            files,
        })
    }

    /// For each ask, in order, every file relative to the working directory
    /// that references it. Each file is read once, whatever the number of
    /// asks, and one every ask excludes is not touched.
    pub fn find(&self, asks: &[ConsumerAsk<'_>]) -> Result<Vec<Vec<PathBuf>>> {
        // Asks looking for the same text share one needle: two kinds may
        // write one pattern.
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut needles: Vec<String> = Vec::new();
        let mut asks_of: Vec<Vec<usize>> = Vec::new();
        for (i, ask) in asks.iter().enumerate() {
            let needle = ask.decl.pattern.replace("{slug}", ask.slug);
            let n = *index.entry(needle).or_insert_with_key(|needle| {
                needles.push(needle.clone());
                asks_of.push(Vec::new());
                needles.len() - 1
            });
            asks_of[n].push(i);
        }
        let excludes = asks
            .iter()
            .map(|ask| excludes(ask.decl, ask.slug))
            .collect::<Result<Vec<_>>>()?;
        let automaton = AhoCorasick::new(&needles).map_err(|e| Error::ConfigInvalid {
            message: format!("the consumer patterns cannot be searched together: {e}"),
            location: None,
        })?;

        let mut found = vec![Vec::new(); asks.len()];
        let mut present = vec![false; needles.len()];
        for path in &self.files {
            let relative = path.strip_prefix(&self.working_dir).unwrap_or(path);
            // A file every ask excludes counts for none of them, so it is not
            // touched: what it holds answers nothing, and it, or a directory
            // above it, may be unreadable.
            if excludes
                .iter()
                .all(|globs| globs.iter().any(|p| p.matches_path(relative)))
            {
                continue;
            }
            // The listing names more than plain files, and only a plain file
            // references anything here: a tracked file deleted from the tree,
            // or under a directory that has become a file, a submodule's
            // gitlink and a nested repository's directory hold nothing of this
            // project's, and a symlink's content is its link text — its target
            // may be outside the project, or a file already counted under its
            // own path. Only the file itself is looked at: a directory on its
            // path that has become a link is followed.
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => continue,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                    ) =>
                {
                    continue;
                }
                Err(e) => {
                    return Err(Error::IoFailure {
                        path: path.clone(),
                        source: e,
                    });
                }
            }
            // A file that cannot be read fails rather than being skipped: a
            // skipped consumer is a false `NoConsumers` signal. The bytes are
            // searched as they are, so one stray byte neither aborts the
            // sweep nor hides a match.
            let bytes = std::fs::read(path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            present.fill(false);
            // Overlapping, so a needle inside another's match is still seen.
            for hit in automaton.find_overlapping_iter(&bytes) {
                present[hit.pattern().as_usize()] = true;
            }
            for (n, asks) in asks_of.iter().enumerate() {
                if !present[n] {
                    continue;
                }
                for &i in asks {
                    if !excludes[i].iter().any(|p| p.matches_path(relative)) {
                        found[i].push(relative.to_path_buf());
                    }
                }
            }
        }
        Ok(found)
    }
}

/// The exclude globs of `decl` for `slug`. The slug stands in literally:
/// unescaped, a slug's `[` or `*` would be read as a pattern, and the
/// artifact's own file would count as its consumer.
fn excludes(decl: &ConsumerDetectorDecl, slug: &str) -> Result<Vec<glob::Pattern>> {
    let slug = glob::Pattern::escape(slug);
    decl.exclude_globs
        .iter()
        .map(|g| {
            let glob = g.replace("{slug}", &slug);
            glob::Pattern::new(&glob).map_err(|e| Error::ConfigInvalid {
                message: format!(
                    "consumer detector for kind '{}' has invalid exclude_glob '{glob}': {e}",
                    decl.kind
                ),
                location: None,
            })
        })
        .collect()
}
