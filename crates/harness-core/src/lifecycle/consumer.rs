//! Consumer detection — find every file referencing a slug under the
//! anchored working directory.
//!
//! [`ConsumerDetector`] is anchored to a working directory at construction,
//! so [`ConsumerDetector::find_consumers`] takes only a slug.

use std::path::{Path, PathBuf};

use crate::config::ConsumerDetectorDecl;
use crate::error::{Error, Result};
use crate::git;

/// Matches the `{slug}`-substituted pattern against every plain file the
/// project owns, as its own ignore files define it through
/// [`git::owned_files`]. A directory walk would count a match in
/// build output, a fixture tree or a sibling checkout on this machine as a
/// consumer, and the count would differ between two clones of one commit;
/// a project that is not a repository is refused rather than walked. What
/// the project owns but reads no rule from — a lockfile, a ledger — is
/// `exclude_globs`' to name; no extension list stands in for it.
pub struct ConsumerDetector {
    decl: ConsumerDetectorDecl,
    working_dir: PathBuf,
    /// The plain files the project owns, listed once: the set is the same
    /// for every slug, and a sweep asks for one slug per artifact.
    files: Vec<PathBuf>,
}

impl ConsumerDetector {
    /// Lists the project's files up front, so a project that cannot be
    /// listed is known before any slug is asked about.
    pub fn new(decl: ConsumerDetectorDecl, working_dir: &Path) -> Result<Self> {
        let owned = git::owned_files(working_dir, &[])
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
            working_dir: working_dir.to_path_buf(),
            files,
        })
    }

    /// Every file, relative to the anchored working directory, that
    /// references `slug`.
    pub fn find_consumers(&self, slug: &str) -> Result<Vec<PathBuf>> {
        let needle = self.decl.pattern.replace("{slug}", slug);
        // The slug stands in literally: unescaped, a slug's `[` or `*` would be
        // read as a pattern, and the artifact's own file would count as its
        // consumer.
        let literal = glob::Pattern::escape(slug);
        let excludes: Vec<glob::Pattern> = self
            .decl
            .exclude_globs
            .iter()
            .filter_map(|g| glob::Pattern::new(&g.replace("{slug}", &literal)).ok())
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
