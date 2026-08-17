//! Glob patterns rooted at a literal directory.
//!
//! `glob::glob` parses its whole argument as a pattern, so joining a project
//! path onto a relative pattern hands the project's own directory names to the
//! pattern parser. A checkout at `~/src/repo [backup]` or `~/clients/acme (v2)?`
//! then matches nothing, and every caller reads the empty result as "the project
//! holds no such file" — a gate reporting clean having opened nothing. A `?` is
//! worse than silence: it matches a *different* directory, so the answer is
//! about a tree the caller never named.
//!
//! [`rooted`] is the one place the two halves meet: the root is escaped so it
//! matches itself, and only the caller's pattern stays a pattern.
//!
//! ## What this module refuses to do
//!
//! - Never escape the caller's pattern — `*` and `?` are why the caller is here.
//! - Never decide what an unreadable match means. It returns a pattern string;
//!   whether a traversal error is fatal or skippable belongs to the caller.
//! - Never silently drop a non-UTF-8 root. `glob` takes `&str`, and a path that
//!   cannot become one is reported rather than skipped.

use std::path::Path;

use crate::error::{Error, Result};

/// `pattern`, read relative to `root`, as a string `glob::glob` treats with
/// `root` as a literal prefix.
///
/// `pattern` keeps `join` semantics, so an absolute `pattern` still replaces
/// `root` — the caller owns where its pattern came from.
pub fn rooted(root: &Path, pattern: &str) -> Result<String> {
    let root = root.to_str().ok_or_else(|| Error::IoFailure {
        path: root.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "project path is not valid UTF-8, so it cannot be escaped into a glob",
        ),
    })?;
    let full = Path::new(&glob::Pattern::escape(root)).join(pattern);
    full.to_str()
        .map(str::to_string)
        .ok_or_else(|| Error::IoFailure {
            path: full.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "glob path is not valid UTF-8",
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metacharacter_in_the_root_matches_itself() {
        let dir = tempfile::tempdir().unwrap();
        // Every class of pattern syntax that can legally appear in a directory
        // name. The first four are what `glob` treats as syntax and what
        // escaping has to neutralise; the rest are the near-misses that must
        // pass through untouched, since escaping a character `glob` reads
        // literally would break the path just as thoroughly. `\` is among
        // them: it is a separator on Windows, so this crate's patterns never
        // give it a second meaning.
        for name in [
            "my[test]repo",
            "acme (v2)?",
            "release*",
            "a?b",
            "{a,b}",
            "!bang",
            "[!a]neg",
            r"back\slash",
            "훅[x]",
        ] {
            let root = dir.path().join(name);
            std::fs::create_dir_all(root.join(".claude/rules")).unwrap();
            std::fs::write(root.join(".claude/rules/x.md"), "x").unwrap();

            let pattern = rooted(&root, ".claude/rules/*.md").unwrap();
            let hits: Vec<_> = glob::glob(&pattern)
                .unwrap()
                .filter_map(|e| e.ok())
                .collect();
            assert_eq!(hits.len(), 1, "root '{name}' -> pattern '{pattern}'");
            assert_eq!(hits[0], root.join(".claude/rules/x.md"));
        }
    }

    #[test]
    fn a_sibling_the_root_would_have_matched_is_not_returned() {
        // The failure mode a compile-error check cannot see: `[ab]` compiles
        // fine and matches the *neighbouring* directory, so the caller gets a
        // confident answer about a tree it never named.
        let dir = tempfile::tempdir().unwrap();
        for name in ["[ab]", "a"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(dir.path().join("a/hit.md"), "wrong tree").unwrap();

        let pattern = rooted(&dir.path().join("[ab]"), "*.md").unwrap();
        let hits: Vec<_> = glob::glob(&pattern)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(hits.is_empty(), "matched a sibling: {hits:?}");
    }

    #[test]
    fn the_callers_pattern_keeps_its_metacharacters() {
        let rooted = rooted(Path::new("/tmp/plain"), ".claude/**/*.md").unwrap();
        assert!(rooted.ends_with(".claude/**/*.md"), "{rooted}");
        assert!(rooted.starts_with("/tmp/plain/"), "{rooted}");
    }
}
