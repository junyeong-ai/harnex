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
//! - Never let the caller's environment reread a pathspec. Every git command
//!   harnex runs is built by [`command`].

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use jiff::Timestamp;
use unicode_normalization::UnicodeNormalization;

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
        "--exclude-per-directory=.gitignore",
        "--",
    ];
    args.extend_from_slice(pathspecs);
    let mut paths = listed_paths(dir, &args)?;
    // The index holds a path's stages side by side, so its repeats are
    // adjacent. `--deduplicate` would say this too, but the git of Ubuntu
    // 20.04 and Debian 11 refuses it, and every gate reading this listing
    // would fail there.
    paths.dedup();
    Ok(paths)
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

/// When a project's history last changed a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastChange {
    /// The last commit that changed it; the working tree still matches it.
    Committed(Timestamp),
    /// A change no commit holds yet — an edit, or a file neither committed nor
    /// ignored — so it is changing now.
    Uncommitted,
    /// Nothing this history records: a path outside the repository, inside a
    /// repository of its own, ignored, or not there at all.
    Unrecorded,
}

impl LastChange {
    /// One answer for what is read through several parts: a change to any is
    /// a change to it, a part nothing records leaves it unrecorded, and
    /// otherwise it changed when the latest of them did.
    fn with(self, other: Self) -> Self {
        match (self, other) {
            (Self::Uncommitted, _) | (_, Self::Uncommitted) => Self::Uncommitted,
            (Self::Unrecorded, _) | (_, Self::Unrecorded) => Self::Unrecorded,
            (Self::Committed(a), Self::Committed(b)) => Self::Committed(a.max(b)),
        }
    }
}

/// What one path reads, spelled from the repository's root: where the path
/// resolves, and every link the resolution passes through but one that
/// locates the project, since repointing one changes what it reads. Where it
/// resolves is dated as git records it, so a link among a directory's files is
/// dated as that link.
struct Reading {
    /// `None` where the path resolves outside the repository, or to nothing.
    content: Option<String>,
    /// `None` for a link outside the repository, which no history records.
    links: Vec<Option<String>>,
}

impl Reading {
    fn of(top: &Path, project: &Path, path: &Path) -> Result<Self, Failure> {
        let spec = |resolved: &Path| match resolved.strip_prefix(top) {
            Ok(relative) => relative
                .to_str()
                .map(|spec| Some(spec.to_owned()))
                .ok_or_else(|| Failure(format!("{} is not a UTF-8 path", path.display()))),
            Err(_) => Ok(None),
        };
        // A path that is not there has no history; any other reason it
        // cannot be resolved is a measurement that failed.
        let (content, links) = match resolve(path) {
            Ok(resolved) => resolved,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                return Ok(Self {
                    content: None,
                    links: Vec::new(),
                });
            }
            Err(e) => return Err(Failure(format!("{}: {e}", path.display()))),
        };
        // A link outside the repository that leads to the project or above it,
        // `/var` on macOS or a workspace opened through a link, locates the
        // project rather than what the path reads.
        let locates_project = |link: &Path| {
            std::fs::canonicalize(link)
                .map(|to| project.starts_with(to))
                .map_err(|e| Failure(format!("{}: {e}", link.display())))
        };
        let mut passed = Vec::new();
        for link in &links {
            match spec(link)? {
                None if locates_project(link)? => {}
                part => passed.push(part),
            }
        }
        Ok(Self {
            content: spec(&content)?,
            links: passed,
        })
    }

    /// The parts git is asked about.
    fn recorded(&self) -> impl Iterator<Item = &str> {
        self.content
            .iter()
            .chain(self.links.iter().flatten())
            .map(String::as_str)
    }
}

/// `path` resolved as the system resolves it, a component at a time, each
/// link's target taking the link's place: where it ends, as its directories
/// store it, and every link it passed through, each in its directory's
/// stored spelling under the name the path gave it.
fn resolve(path: &Path) -> std::io::Result<(PathBuf, Vec<PathBuf>)> {
    // Linux stops following links at 40, macOS at 32.
    const MAX_LINKS: usize = 40;
    let steps = |path: &Path| -> Vec<PathBuf> {
        path.components()
            .rev()
            .map(|step| PathBuf::from(step.as_os_str()))
            .collect()
    };
    let mut rest = steps(&std::path::absolute(path)?);
    let mut at = PathBuf::new();
    let mut links = Vec::new();
    while let Some(step) = rest.pop() {
        match step.components().next() {
            Some(Component::Normal(name)) => {
                let next = at.join(name);
                if !std::fs::symlink_metadata(&next)?.file_type().is_symlink() {
                    at = next;
                    continue;
                }
                if links.len() == MAX_LINKS {
                    return Err(std::io::Error::other("too many levels of links"));
                }
                links.push(std::fs::canonicalize(&at)?.join(name));
                rest.extend(steps(&std::fs::read_link(&next)?));
            }
            Some(Component::ParentDir) => {
                at.pop();
            }
            Some(Component::RootDir | Component::Prefix(_)) => at.push(&step),
            Some(Component::CurDir) | None => {}
        }
    }
    Ok((std::fs::canonicalize(&at)?, links))
}

/// When the repository holding `dir` last changed each of `paths` on this
/// branch, in the order given — the whole repository, so a project in a
/// subdirectory reads what it links in from beside it. A directory is dated by
/// the files under it, and holds an uncommitted change when any of them does
/// or is untracked.
///
/// Each path is answered as `git status` and the history see it, so what
/// neither records is [`LastChange::Unrecorded`] rather than a change.
///
/// A shallow clone is a failure rather than an answer: its boundary commit
/// lists every file it holds, which reads as each one changing the day the
/// clone was cut.
pub(crate) fn last_changes(dir: &Path, paths: &[PathBuf]) -> Result<Vec<LastChange>, Failure> {
    // Nothing asked needs no repository: a sweep whose every kind was skipped
    // still answers.
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let top = stdout(dir, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim_end_matches('\n'));
    let canonical = |path: &Path| {
        std::fs::canonicalize(path).map_err(|e| Failure(format!("{}: {e}", path.display())))
    };
    let top = canonical(top)?;
    let project = canonical(dir)?;
    let readings = paths
        .iter()
        .map(|path| Reading::of(&top, &project, path))
        .collect::<Result<Vec<Reading>, Failure>>()?;
    let pathspecs: Vec<&str> = readings.iter().flat_map(Reading::recorded).collect();
    let record = match pathspecs.is_empty() {
        true => Record::default(),
        false => Record::read(&top, &pathspecs)?,
    };
    let answer = |part: Option<&str>| match part {
        Some(spec) => record.of(spec),
        None => Ok(LastChange::Unrecorded),
    };
    readings
        .iter()
        .map(|reading| {
            reading
                .links
                .iter()
                .try_fold(answer(reading.content.as_deref())?, |read, link| {
                    Ok(read.with(answer(link.as_deref())?))
                })
        })
        .collect()
}

/// What the working tree and the history hold of the paths asked about, each
/// under the path asked: a file git lists is a file under each directory
/// asked that holds it.
#[derive(Default)]
struct Record<'a> {
    pending: HashSet<&'a str>,
    latest: HashMap<&'a str, i64>,
}

impl<'a> Record<'a> {
    /// One listing of what is pending and one walk of the history answer
    /// every path, each read as a literal, so a `[` in a name names that file.
    /// Run at `top`, every path is spelled from the repository's root.
    fn read(top: &Path, paths: &[&'a str]) -> Result<Self, Failure> {
        if stdout(top, &["rev-parse", "--is-shallow-repository"])?.trim() == "true" {
            return Err(Failure(
                "this clone is shallow, so every file its oldest commit holds reads as changed \
                 the day the clone was cut — fetch the full history (`git fetch --unshallow`)"
                    .into(),
            ));
        }
        let spelling = Spelling::of(top)?;
        // Where git ignores case, a pathspec matches as `git status` matches
        // the file system's names to the ones it tracks.
        let magic = match spelling.folds_case {
            true => ":(literal,icase)",
            false => ":(literal)",
        };
        // A directory is left out by the bytes it is stored under alone: with
        // `icase` it would also leave out a path asked in another case.
        let scopes = scopes(top, paths);
        let specs: Vec<String> = scopes
            .asked
            .iter()
            .map(|scope| format!("{magic}{scope}"))
            .chain(
                scopes
                    .left_out
                    .iter()
                    .map(|dir| format!(":(exclude,literal){dir}")),
            )
            .collect();
        let with_specs = |args: &[&'static str]| {
            let mut args: Vec<&str> = args.to_vec();
            args.push("--");
            args.extend(specs.iter().map(String::as_str));
            args
        };
        let asked = Asked::new(paths, spelling);
        let mut record = Self::default();
        let committed = has_commits(top)?;

        // Pending: an edit against `HEAD`, and a file untracked under the
        // developer's own excludes too, since whether this checkout holds a
        // change is its own question and a `.DS_Store` every Finder window
        // writes is not one. Before the first commit, every tracked file is an
        // addition. A file whose time alone moved is no edit, as `git status`
        // reads it, whatever `diff.autoRefreshIndex` says. `--no-renames` lists
        // a move as the path it left and the one it reached, since rename
        // detection names only the second and the directory the file left
        // changed too.
        let tracked_changes = match committed {
            true => stdout(
                top,
                &with_specs(&[
                    "-c",
                    "diff.autoRefreshIndex=true",
                    "diff",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    "HEAD",
                ]),
            )?,
            false => stdout(top, &with_specs(&["ls-files", "-z", "--cached"]))?,
        };
        let untracked = stdout(
            top,
            &with_specs(&["ls-files", "-z", "--others", "--exclude-standard"]),
        )?;
        record.pending = tracked_changes
            .split('\0')
            .chain(untracked.split('\0'))
            .filter(|listed| !listed.is_empty())
            .flat_map(|listed| asked.holding(listed))
            .collect();
        if !committed {
            return Ok(record);
        }

        // A change is dated when it reached this branch: `--first-parent`
        // credits a merge with what it changed against the branch it landed
        // on, so a side commit written long before keeps no old date, and a
        // conflict resolution is a change. Walking every parent instead hides
        // a merge that took one side whole, and its side commit's date stands.
        // Where the first parents run through a feature branch — a trunk
        // fast-forwarded past a merge of itself — the trunk's own changes date
        // from that merge, younger than they are: the sweep errs toward
        // keeping. `-m` asks for a merge's changes at all: an older git,
        // Ubuntu 20.04's, lists none without it and would date what a merge
        // brought by the side commit, long before it landed.
        //
        // The user's configuration may not reshape what is parsed below:
        // `log.showSignature` prints verification lines into it,
        // `log.showRoot=false` drops the names the first commit added, and
        // `log.diffMerges=combined` lists none of what a merge took whole from
        // its side. A git older than that key ignores it.
        let log = stdout(
            top,
            &with_specs(&[
                "-c",
                "log.diffMerges=first-parent",
                "log",
                "-z",
                "--format=%x00%ct",
                "--name-only",
                "--first-parent",
                "-m",
                "--no-renames",
                "--root",
                "--no-show-signature",
                "HEAD",
            ]),
        )?;

        // Each commit opens with an empty field, then its time, then the paths
        // it touched — the first of them behind the newline that ends the
        // header.
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
            for holder in asked.holding(path) {
                record
                    .latest
                    .entry(holder)
                    .and_modify(|seen| *seen = (*seen).max(time))
                    .or_insert(time);
            }
        }
        Ok(record)
    }

    /// The answer for one path asked about.
    fn of(&self, spec: &str) -> Result<LastChange, Failure> {
        match (self.pending.contains(spec), self.latest.get(spec)) {
            (true, _) => Ok(LastChange::Uncommitted),
            (false, Some(&seconds)) => Timestamp::from_second(seconds)
                .map(LastChange::Committed)
                .map_err(|e| Failure(format!("commit time {seconds} for '{spec}': {e}"))),
            (false, None) => Ok(LastChange::Unrecorded),
        }
    }
}

/// What git is asked about for a set of paths, spelled from the repository's
/// root.
struct Scopes<'a> {
    /// Each path, or the directory holding several of them; the root as `.`.
    asked: Vec<&'a str>,
    /// The directories inside one asked that no path lies in.
    left_out: Vec<String>,
}

/// What git is asked about for `paths`, spelled from `top`, the repository's
/// root. git compares every spec with every name it walks, so several paths in
/// one directory, a set of skills, are asked as that directory wherever that
/// takes fewer specs: the directory, less each directory in it no path lies in,
/// since one can hold a package's whole history. git lists all it is asked, so
/// a directory is never asked whole where a path lies deeper in one it holds,
/// and asked so, it adds only the files beside the paths and what it held
/// before. Any other path is asked as itself, and none under another at all.
fn scopes<'a>(top: &Path, paths: &[&'a str]) -> Scopes<'a> {
    let place = |path: &'a str| -> Option<(&'a str, &'a OsStr)> {
        let path = Path::new(path);
        Some((path.parent()?.to_str()?, path.file_name()?))
    };
    let mut holding: HashMap<&'a str, HashSet<&'a OsStr>> = HashMap::new();
    for &path in paths {
        if let Some((dir, name)) = place(path) {
            holding.entry(dir).or_default().insert(name);
        }
    }
    let asked: HashSet<&'a str> = paths.iter().copied().collect();
    let above: HashSet<&'a str> = paths
        .iter()
        .flat_map(|&path| Path::new(path).ancestors().skip(1))
        .filter_map(Path::to_str)
        .collect();
    let mut merged: HashSet<&'a str> = HashSet::new();
    let mut left_out = Vec::new();
    for (&dir, names) in &holding {
        match unnamed_directories(top, dir, &asked, &above) {
            Some(unnamed) if 1 + unnamed.len() < names.len() => {
                merged.insert(dir);
                left_out.extend(unnamed);
            }
            _ => {}
        }
    }
    left_out.sort_unstable();
    let mut scopes: Vec<&'a str> = paths
        .iter()
        .map(|&path| match place(path) {
            Some((dir, _)) if merged.contains(dir) => dir,
            _ => path,
        })
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    // One under another adds nothing git does not list for that one.
    let scoped: HashSet<&'a str> = scopes.iter().copied().collect();
    scopes.retain(|scope| {
        !Path::new(scope)
            .ancestors()
            .skip(1)
            .filter_map(Path::to_str)
            .any(|above| scoped.contains(above))
    });
    Scopes {
        asked: scopes
            .into_iter()
            .map(|scope| match scope {
                "" => ".",
                scope => scope,
            })
            .collect(),
        left_out,
    }
}

/// The directories `dir` holds that neither are a path `asked` nor lie `above`
/// one, spelled from the repository's root `top`; a link is not one, since git
/// records a link and not what it points at. `None` where `dir` is not to be
/// asked whole: a path asked is it or holds it, and needs all it holds, since
/// git leaves out what is left out for every path; it holds a directory a path
/// lies deeper in; or it or one of its names cannot be read. Its paths asked
/// one by one answer the same.
fn unnamed_directories(
    top: &Path,
    dir: &str,
    asked: &HashSet<&str>,
    above: &HashSet<&str>,
) -> Option<Vec<String>> {
    if Path::new(dir)
        .ancestors()
        .filter_map(Path::to_str)
        .any(|holder| asked.contains(holder))
    {
        return None;
    }
    let mut unnamed = Vec::new();
    for entry in std::fs::read_dir(top.join(dir)).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let path = Path::new(dir).join(entry.file_name());
        let path = path.to_str()?;
        if asked.contains(path) {
            continue;
        }
        if above.contains(path) {
            return None;
        }
        unnamed.push(path.to_owned());
    }
    Some(unnamed)
}

/// How git matches the names a file system gives to the ones it lists, as
/// the repository is configured.
#[derive(Clone, Copy)]
struct Spelling {
    /// `core.precomposeUnicode`, which git reads on macOS only: git lists
    /// names composed, where the file system may give them decomposed.
    composes: bool,
    /// `core.ignorecase`: git lists a file under the case it was added with,
    /// which the file system may have changed since without git reading it as
    /// a change.
    folds_case: bool,
}

impl Spelling {
    fn of(top: &Path) -> Result<Self, Failure> {
        let set = |key: &str| -> Result<bool, Failure> {
            let value = stdout(top, &["config", "--type=bool", "--default=false", key])?;
            Ok(value.trim() == "true")
        };
        Ok(Self {
            composes: cfg!(target_os = "macos") && set("core.precomposeUnicode")?,
            folds_case: set("core.ignorecase")?,
        })
    }

    /// The key two names share where git reads them as one name, or `None`
    /// where it reads every name byte for byte. git's `icase` folds ASCII
    /// letters only.
    fn alike(self, name: &str) -> Option<String> {
        if !self.composes && !self.folds_case {
            return None;
        }
        let name: String = match self.composes {
            true => name.nfc().collect(),
            false => name.to_owned(),
        };
        Some(match self.folds_case {
            true => name.to_ascii_lowercase(),
            false => name,
        })
    }
}

/// The paths asked about, matched against the paths git lists.
struct Asked<'a> {
    spelled: HashSet<&'a str>,
    alike: HashMap<String, Vec<&'a str>>,
    spelling: Spelling,
}

impl<'a> Asked<'a> {
    fn new(paths: &[&'a str], spelling: Spelling) -> Self {
        let mut alike: HashMap<String, Vec<&'a str>> = HashMap::new();
        for &path in paths {
            if let Some(key) = spelling.alike(path) {
                alike.entry(key).or_default().push(path);
            }
        }
        Self {
            spelled: paths.iter().copied().collect(),
            alike,
            spelling,
        }
    }

    /// Every path asked about that `listed` is or lies under: git lists files,
    /// and a change to one is a change to each directory holding it. A name
    /// matches as it is spelled, and only failing that as git reads it, so
    /// two names a file system keeps apart stay apart. A repository nested
    /// untracked inside this one is listed with a trailing `/`.
    fn holding(&self, listed: &str) -> Vec<&'a str> {
        let listed = Path::new(listed.trim_end_matches('/'));
        let mut holding = Vec::new();
        for ancestor in listed.ancestors().filter_map(Path::to_str) {
            if let Some(&path) = self.spelled.get(ancestor) {
                holding.push(path);
            } else if let Some(key) = self.spelling.alike(ancestor) {
                holding.extend(self.alike.get(&key).into_iter().flatten());
            }
        }
        holding
    }
}

/// Whether `HEAD` names a commit. A repository on an unborn branch has none,
/// and no file in it has a history yet.
fn has_commits(dir: &Path) -> Result<bool, Failure> {
    let status = command(dir)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
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

/// Git's global pathspec magic. Exported in the caller's shell, each rereads
/// every pathspec a command is given: `GIT_LITERAL_PATHSPECS` makes
/// `:(literal)` part of a file name, so an age matches no commit and reads as
/// unrecorded, and `GIT_ICASE_PATHSPECS` folds the case of a path asked for
/// exactly.
const PATHSPEC_VARIABLES: [&str; 4] = [
    "GIT_LITERAL_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
];

/// A git command run in `dir` that reads each pathspec as written, whatever
/// the caller's environment says about pathspecs: [`PATHSPEC_VARIABLES`].
pub(crate) fn command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(dir);
    for variable in PATHSPEC_VARIABLES {
        command.env_remove(variable);
    }
    command
}

/// What a git command printed, which must be UTF-8: every caller reads paths
/// from it, and a lossy decode names a file that does not exist.
fn stdout(dir: &Path, args: &[&str]) -> Result<String, Failure> {
    let command = format!("git {}", args.join(" "));
    let output = self::command(dir)
        .args(args)
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

#[cfg(test)]
mod tests {
    use super::{Asked, Spelling, scopes};

    #[cfg(unix)]
    #[test]
    fn git_is_asked_about_a_directory_only_where_that_takes_fewer_specs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let top = tmp.path();
        for dir in [
            ".claude/skills/a",
            ".claude/skills/b",
            ".claude/skills/c",
            ".claude/skills/d",
            ".claude/skills/core",
            ".claude/rules/frontend",
            "apps/web/src",
            "packages/x",
            "packages/y",
            "packages/z",
            "shared/rules",
            "vendor/a",
            "vendor/b",
            "docs/x/scripts",
            "pkgs/web/src",
        ] {
            std::fs::create_dir_all(top.join(dir)).unwrap();
        }
        for file in [
            ".claude/skills/README.md",
            "apps/web/AGENTS.md",
            "shared/rules/r.md",
            "shared/rules/s.md",
            "vendor/a/CLAUDE.md",
            "docs/x/a.md",
            "docs/x/b.md",
            "docs/x/c.md",
            "pkgs/web/a.md",
            "pkgs/web/b.md",
            "pkgs/web/c.md",
        ] {
            std::fs::write(top.join(file), "").unwrap();
        }
        std::os::unix::fs::symlink("AGENTS.md", top.join("apps/web/CLAUDE.md")).unwrap();
        std::os::unix::fs::symlink("../../packages/z", top.join(".claude/skills/linked")).unwrap();

        let scopes = scopes(
            top,
            &[
                // A set of skills beside one a foundation kind carves out.
                ".claude/skills/a",
                ".claude/skills/b",
                ".claude/skills/c",
                ".claude/skills/d",
                // Rules beside a folder of them, which would be listed whole.
                ".claude/rules/one.md",
                ".claude/rules/two.md",
                ".claude/rules/three.md",
                ".claude/rules/four.md",
                ".claude/rules/frontend/five.md",
                // A `CLAUDE.md` read through its link, beside a package's sources.
                "apps/web/CLAUDE.md",
                "apps/web/AGENTS.md",
                "packages/x",
                "packages/y",
                // Two artifacts reading one rule.
                "shared/rules/r.md",
                "shared/rules/r.md",
                // Every package linked in, and one read on its own besides.
                "vendor/a",
                "vendor/b",
                "vendor/a/CLAUDE.md",
                // A directory artifact, and the documents inside it; and a
                // whole tree, and documents deeper in it. Each needs all it
                // holds, so nothing in it is left out.
                "docs/x",
                "docs/x/a.md",
                "docs/x/b.md",
                "docs/x/c.md",
                "pkgs",
                "pkgs/web/a.md",
                "pkgs/web/b.md",
                "pkgs/web/c.md",
            ],
        );
        assert_eq!(
            scopes.asked,
            [
                ".claude/rules/four.md",
                ".claude/rules/frontend/five.md",
                ".claude/rules/one.md",
                ".claude/rules/three.md",
                ".claude/rules/two.md",
                ".claude/skills",
                "apps/web/AGENTS.md",
                "apps/web/CLAUDE.md",
                "docs/x",
                "packages/x",
                "packages/y",
                "pkgs",
                "shared/rules/r.md",
                "vendor",
            ]
        );
        assert_eq!(scopes.left_out, [".claude/skills/core"]);
    }

    #[test]
    fn a_path_that_resolves_to_the_root_asks_git_about_everything() {
        // Rules that would be asked as their directory less the drafts beside
        // them are not: the root needs the drafts too.
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("rules/drafts")).unwrap();
        let scopes = scopes(tmp.path(), &["", "rules/a.md", "rules/b.md", "rules/c.md"]);
        assert_eq!((scopes.asked, scopes.left_out), (vec!["."], vec![]));
    }

    const BYTES: Spelling = Spelling {
        composes: false,
        folds_case: false,
    };

    #[test]
    fn a_listed_name_matches_as_spelled_before_as_composed() {
        let composed = "\u{e9}.md";
        let decomposed = "e\u{301}.md";
        let skill = "\u{1112}\u{1161}\u{11ab}";
        let composes = Spelling {
            composes: true,
            ..BYTES
        };
        let asked = Asked::new(&[composed, decomposed, skill], composes);
        assert_eq!(asked.holding(composed), [composed]);
        assert_eq!(asked.holding(decomposed), [decomposed]);
        assert_eq!(asked.holding("\u{d55c}/SKILL.md"), [skill]);
        // Where git keeps names byte for byte, so does the match.
        assert!(
            Asked::new(&[skill], BYTES)
                .holding("\u{d55c}/SKILL.md")
                .is_empty()
        );
    }

    #[test]
    fn a_listed_name_matches_in_another_case_only_where_git_ignores_case() {
        let folds = Spelling {
            folds_case: true,
            ..BYTES
        };
        let asked = |spelling| Asked::new(&["rules/old.md", "\u{e9}.md"], spelling);
        assert_eq!(asked(folds).holding("Rules/Old.md"), ["rules/old.md"]);
        assert!(asked(BYTES).holding("Rules/Old.md").is_empty());
        // git folds ASCII letters only.
        assert!(asked(folds).holding("\u{c9}.md").is_empty());
    }
}
