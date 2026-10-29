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

use tempfile::TempDir;

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

/// The pin every release of this minor satisfies. A fixture states it instead
/// of a literal version, so a release rewrites no fixture.
pub const FIXTURE_PIN: &str = concat!(
    ">=",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// `sections` under a `[meta]` that pins [`FIXTURE_PIN`].
pub fn pinned(sections: &str) -> String {
    format!("[meta]\nharnex_version = \"{FIXTURE_PIN}\"\n{sections}")
}

/// A project a harness lives in: a git repository by construction, since the
/// gates ask git which files are the project's own.
pub fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    let status = git(dir.path()).args(["init", "-q"]).status().unwrap();
    assert!(status.success(), "git init");
    dir
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

/// Every bash a shipped script must parse under that this machine has: the one
/// on `PATH`, and `/bin/bash`, which macOS keeps at 3.2 whatever `PATH` holds.
pub fn bashes() -> Vec<&'static str> {
    ["bash", "/bin/bash"]
        .into_iter()
        .filter(|bash| Command::new(bash).arg("--version").output().is_ok())
        .collect()
}

/// `bash -n` under every bash in [`bashes`]; `Err` names the one that refused
/// and what it said.
pub fn parses_under_every_bash(path: &Path) -> Result<(), String> {
    for bash in bashes() {
        let out = Command::new(bash)
            .arg("-n")
            .arg(path)
            .output()
            .map_err(|e| format!("{bash}: {e}"))?;
        if !out.status.success() {
            return Err(format!("{bash}: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    Ok(())
}
