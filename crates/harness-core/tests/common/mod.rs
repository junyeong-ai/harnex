//! What a guard over this repository reads: the index, not the disk.
//!
//! A gate whose input set is a directory walk answers about whatever is on
//! this machine — an editor's backup, a scratch file, a sibling worktree — and
//! its verdict here and in CI diverge on the same commit. What ships is what
//! is tracked, so every guard that enumerates a corpus asks git.

// Each test binary compiles this module on its own and uses the part it
// needs, so a helper unused by one binary is still owned by another.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The tracked files under `pathspec`, as absolute paths.
pub fn tracked(root: &Path, pathspec: &str) -> Vec<PathBuf> {
    let listing = git(root)
        .args(["ls-files", "-z", "--", pathspec])
        .output()
        .expect("git ls-files runs in this repository");
    assert!(listing.status.success(), "git ls-files failed: {listing:?}");
    String::from_utf8(listing.stdout)
        .expect("tracked paths are UTF-8")
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| root.join(path))
        .collect()
}

/// A git command that answers about `root` and nothing else: no global or
/// system configuration, so a hooks path or a template directory set on this
/// machine cannot reach a temporary repository a test builds.
pub fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}
