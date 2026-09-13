//! What a rule's `paths:` matches, as Claude Code decides it.
//!
//! The runtime hands a rule's patterns to the `ignore` npm package, so a
//! pattern means what a `.gitignore` line means: a name without a slash matches
//! at any depth, a slash anchors it to the root, a directory covers everything
//! below it, `!` re-includes, and case folds — the package's default, which the
//! runtime does not override. A path is relative to the directory holding the
//! rule's `.claude/rules`, and one that climbs out of it matches nothing.
//!
//! [`gix_ignore`] ports git's own wildmatch. Measured against `git -c
//! core.ignorecase=true check-ignore` over 3,847 pattern and path pairs — the
//! patterns and touched files of this project's transcripts, plus negation,
//! bracket, brace, escape and parent-directory cases — it disagreed on none.
//! The `ignore` crate disagreed on every brace pattern.
//!
//! ## What this module refuses to do
//!
//! - Never expand braces or strip a trailing `/**`. The runtime does both
//!   before it matches or records a pattern, so a brace still in a recorded one
//!   is a literal character.
//! - Never resolve a symlink. The runtime retries a path that climbs out of the
//!   root through the link; a transcript is read after the fact, when the link
//!   may be gone, so such a path matches nothing here.

use std::path::{Path, PathBuf};

use gix_ignore::Search;
use gix_ignore::glob::pattern::Case;
use gix_ignore::search::Ignore;

/// A rule's patterns, compiled against the directory they are relative to.
pub(crate) struct PathScope {
    root: PathBuf,
    search: Search,
}

impl PathScope {
    pub(crate) fn new(root: &Path, patterns: &[String]) -> Self {
        Self {
            root: root.to_path_buf(),
            search: Search::from_overrides(patterns, Ignore::default()),
        }
    }

    /// Whether `path` falls under these patterns.
    ///
    /// A directory the patterns cover is checked before anything below it,
    /// because gitignore cannot re-include a file whose parent it excludes.
    /// `path` is recorded as a JSON string, so every component is UTF-8.
    pub(crate) fn contains(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let Some(components) = relative
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<&str>>>()
        else {
            return false;
        };
        let covers = |path: &str, is_dir: bool| {
            self.search
                .pattern_matching_relative_path(path.into(), Some(is_dir), Case::Fold)
                .is_some_and(|m| !m.pattern.is_negative())
        };
        !components.is_empty()
            && ((1..components.len()).any(|depth| covers(&components[..depth].join("/"), true))
                || covers(&components.join("/"), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(patterns: &[&str]) -> PathScope {
        let patterns: Vec<String> = patterns.iter().map(|p| p.to_string()).collect();
        PathScope::new(Path::new("/repo"), &patterns)
    }

    fn at(path: &str) -> PathBuf {
        Path::new("/repo").join(path)
    }

    #[test]
    fn a_directory_covers_every_file_below_it() {
        let s = scope(&["crates/core/src"]);
        assert!(s.contains(&at("crates/core/src/session/record.rs")));
        assert!(!s.contains(&at("crates/core/tests/record.rs")));
    }

    #[test]
    fn a_name_without_a_slash_matches_at_any_depth_and_a_slash_anchors() {
        assert!(scope(&["mod.rs"]).contains(&at("src/a/mod.rs")));
        let anchored = scope(&["src/*.rs"]);
        assert!(anchored.contains(&at("src/lib.rs")));
        assert!(!anchored.contains(&at("lib/src/lib.rs")));
        assert!(
            !anchored.contains(&at("src/a/lib.rs")),
            "`*` stops at a slash"
        );
    }

    #[test]
    fn case_folds_as_the_runtime_leaves_it() {
        assert!(scope(&["src/*.rs"]).contains(&at("SRC/Main.RS")));
    }

    #[test]
    fn a_file_under_an_excluded_directory_cannot_be_re_included() {
        let s = scope(&["src", "!src/main.rs"]);
        assert!(s.contains(&at("src/main.rs")));
        let files = scope(&["src/*.rs", "!src/main.rs"]);
        assert!(!files.contains(&at("src/main.rs")));
        assert!(files.contains(&at("src/lib.rs")));
    }

    #[test]
    fn a_brace_left_in_a_recorded_pattern_is_a_literal() {
        let s = scope(&["src/{a,b}.rs"]);
        assert!(!s.contains(&at("src/a.rs")));
        assert!(s.contains(&at("src/{a,b}.rs")));
    }

    #[test]
    fn a_path_outside_the_root_matches_nothing() {
        let s = scope(&["**"]);
        assert!(s.contains(&at("any/file.rs")));
        assert!(!s.contains(Path::new("/elsewhere/any/file.rs")));
        assert!(
            !s.contains(Path::new("/repo")),
            "the root itself is no file"
        );
    }
}
