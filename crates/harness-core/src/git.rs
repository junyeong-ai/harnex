//! # git — the files a project owns and when each last changed, as git answers it
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
//! - Never read a file's age from the checkout. A modification time is the
//!   moment this clone wrote the file, so every file in a fresh clone reads as
//!   changed today; the history is the same in every clone of a commit.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use jiff::Timestamp;

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
    Ok(stdout(dir, args)?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| dir.join(path))
        .collect())
}

/// The committer time of the last commit that touched each of `paths`, in the
/// order given, or `None` for a path whose working-tree content no commit
/// records: an edit not yet committed, or a file never committed.
///
/// One walk of the history answers every path, each read as a literal, so a
/// `[` in a name names that file. A shallow clone is a failure rather than an
/// answer: its boundary commit lists every file it holds, which reads as each
/// one changing the day the clone was cut.
pub(crate) fn last_committed(
    dir: &Path,
    paths: &[PathBuf],
) -> Result<Vec<Option<Timestamp>>, Failure> {
    let specs = paths
        .iter()
        .map(|path| {
            path.strip_prefix(dir)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| {
                    Failure(format!(
                        "{} is not a UTF-8 path under {}",
                        path.display(),
                        dir.display()
                    ))
                })
        })
        .collect::<Result<Vec<&str>, Failure>>()?;
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    if stdout(dir, &["rev-parse", "--is-shallow-repository"])?.trim() == "true" {
        return Err(Failure(
            "this clone is shallow, so every file its oldest commit holds reads as changed \
             the day the clone was cut — fetch the full history (`git fetch --unshallow`)"
                .into(),
        ));
    }
    if !has_commits(dir)? {
        return Ok(vec![None; specs.len()]);
    }

    let with_specs = |args: &[&'static str]| {
        let mut args = args.to_vec();
        args.push("--");
        args.extend(&specs);
        args
    };
    let edited = stdout(
        dir,
        &with_specs(&[
            "--literal-pathspecs",
            "diff",
            "--name-only",
            "-z",
            "--relative",
            "HEAD",
        ]),
    )?;
    let edited: HashSet<&str> = edited.split('\0').collect();
    let log = stdout(
        dir,
        &with_specs(&[
            "--literal-pathspecs",
            "log",
            "-z",
            "--format=%x00%ct",
            "--name-only",
            "--relative",
            "HEAD",
        ]),
    )?;

    // Each commit opens with an empty field, then its time, then the paths it
    // touched — the first of them behind the newline that ends the header.
    let mut latest: HashMap<&str, i64> = HashMap::new();
    let mut fields = log.split('\0');
    let mut time: Option<i64> = None;
    let mut first = false;
    while let Some(field) = fields.next() {
        if field.is_empty() {
            time = match fields.next() {
                Some(raw) => Some(raw.parse().map_err(|_| {
                    Failure(format!(
                        "git log printed '{raw}' where a commit time belongs"
                    ))
                })?),
                None => None,
            };
            first = true;
            continue;
        }
        let Some(time) = time else {
            return Err(Failure(format!(
                "git log printed the path '{field}' before any commit"
            )));
        };
        let path = match first {
            true => field.strip_prefix('\n').unwrap_or(field),
            false => field,
        };
        first = false;
        latest
            .entry(path)
            .and_modify(|seen| *seen = (*seen).max(time))
            .or_insert(time);
    }

    specs
        .iter()
        .map(|spec| match (edited.contains(spec), latest.get(spec)) {
            (false, Some(&seconds)) => Timestamp::from_second(seconds)
                .map(Some)
                .map_err(|e| Failure(format!("commit time {seconds} for '{spec}': {e}"))),
            _ => Ok(None),
        })
        .collect()
}

/// Whether `HEAD` names a commit. A repository on an unborn branch has none,
/// and no file in it has a history yet.
fn has_commits(dir: &Path) -> Result<bool, Failure> {
    let status = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Failure(format!("git rev-parse --verify HEAD spawn: {e}")))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Failure(format!(
            "git rev-parse --verify HEAD exited with {status} in {}",
            dir.display()
        ))),
    }
}

/// What a git command printed, which must be UTF-8: every caller reads paths
/// from it, and a lossy decode names a file that does not exist.
fn stdout(dir: &Path, args: &[&str]) -> Result<String, Failure> {
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
    String::from_utf8(output.stdout)
        .map_err(|_| Failure(format!("{command} printed a path that is not UTF-8")))
}
