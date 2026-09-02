//! # git — the files a project owns, as git answers it
//!
//! A gate that enumerates its corpus by walking the directory answers about
//! whatever is on this machine — build output, a fixture tree, a sibling
//! checkout — and its verdict differs between two clones of one commit. The
//! project's ignore files are the one non-heuristic statement of which files
//! are its own, and only the project's: `--exclude-standard` would also read
//! the developer's global excludes and `.git/info/exclude`, and a global
//! `CLAUDE.md` line — a common habit for AI configuration — made the same
//! commit pass on one machine and fail on another. An author's untracked
//! file is still theirs, so the set is tracked plus untracked not ignored by
//! `.gitignore`. A tracked file is the project's whatever its path.
//!
//! ## What this module refuses to do
//!
//! - Never walk the tree in git's place: a directory that is not a
//!   repository is a failure the caller names, not a set of files.
//! - Never look inside another repository. A submodule is listed as its
//!   gitlink and a nested checkout as its directory; what they hold is
//!   theirs, and a caller that wants it asks them.
//! - Never decode a path lossily — see [`listed_paths`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// Why a listing could not be taken. Each caller wraps it in the typed error
/// of its own surface, so the wire code says which gate could not read.
pub(crate) struct Failure(pub(crate) String);

/// Every file the project at `dir` owns under `pathspecs` — all of it when
/// none is given — joined to `dir`, one entry per path: an unmerged file is
/// in the index once per stage, and would otherwise be listed once per
/// stage too. Tracked files are listed whether or not they are still on
/// disk.
pub(crate) fn owned_files(dir: &Path, pathspecs: &[&str]) -> Result<Vec<PathBuf>, Failure> {
    let mut args = vec![
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--deduplicate",
        "--exclude-per-directory=.gitignore",
        "--",
    ];
    args.extend_from_slice(pathspecs);
    listed_paths(dir, &args)
}

/// The NUL-delimited paths a git listing prints, joined to `dir`.
///
/// `-z` is the caller's to pass: without it git quotes any path outside
/// ASCII as octal escapes, and a listed `한글.md` never equals the file on
/// disk. A path that is not UTF-8 is a failure rather than a lossy decode:
/// the replacement character names a file that does not exist, which a gate
/// would then skip as absent while reporting the arm as run.
pub(crate) fn listed_paths(dir: &Path, args: &[&str]) -> Result<Vec<PathBuf>, Failure> {
    let command = format!("git {}", args.join(" "));
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| Failure(format!("{command} spawn: {e}")))?;
    if !output.status.success() {
        return Err(Failure(format!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let raw = String::from_utf8(output.stdout)
        .map_err(|_| Failure(format!("{command} listed a path that is not UTF-8")))?;
    Ok(raw
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| dir.join(path))
        .collect())
}
