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
/// resolves, and every link inside the repository the resolution passes
/// through, since repointing one changes what it reads. Where it resolves is
/// dated as git records it, so a link among a directory's files is dated as
/// that link.
struct Reading {
    /// `None` where the path resolves outside the repository, or to nothing.
    content: Option<String>,
    links: Vec<String>,
}

impl Reading {
    fn of(top: &Path, path: &Path) -> Result<Self, Failure> {
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
        // A link outside the repository, `/var` on macOS, is not its to date.
        let mut in_repository = Vec::new();
        for link in &links {
            in_repository.extend(spec(link)?);
        }
        Ok(Self {
            content: spec(&content)?,
            links: in_repository,
        })
    }

    fn parts(&self) -> impl Iterator<Item = &str> {
        self.content.iter().chain(&self.links).map(String::as_str)
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
    let top = std::fs::canonicalize(top).map_err(|e| Failure(format!("{}: {e}", top.display())))?;
    let readings = paths
        .iter()
        .map(|path| Reading::of(&top, path))
        .collect::<Result<Vec<Reading>, Failure>>()?;
    let pathspecs: Vec<&str> = readings.iter().flat_map(Reading::parts).collect();
    let record = match pathspecs.is_empty() {
        true => Record::default(),
        false => Record::read(&top, &pathspecs)?,
    };
    readings
        .iter()
        .map(|reading| {
            let content = match &reading.content {
                Some(spec) => record.of(spec)?,
                None => LastChange::Unrecorded,
            };
            reading
                .links
                .iter()
                .try_fold(content, |read, link| Ok(read.with(record.of(link)?)))
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
    ///
    /// git is asked about the directory holding each path, so the specs stay
    /// few however many artifacts share one: it matches each spec against
    /// every path it lists, and what it lists beyond the paths asked matches
    /// none of them. A path at the root is asked for itself.
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
        let mut scopes: Vec<&str> = paths.iter().map(|path| scope(path)).collect();
        scopes.sort_unstable();
        scopes.dedup();
        let specs: Vec<String> = scopes
            .iter()
            .map(|scope| format!("{magic}{scope}"))
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
        // keeping.
        //
        // The user's configuration may not reshape what is parsed below:
        // `log.showSignature` prints verification lines into it, and
        // `log.showRoot=false` drops the names the first commit added.
        let log = stdout(
            top,
            &with_specs(&[
                "log",
                "-z",
                "--format=%x00%ct",
                "--name-only",
                "--first-parent",
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

/// What git is asked about for `path`: the directory holding it, or, at the
/// repository's root, the path itself, and the root as `.`.
fn scope(path: &str) -> &str {
    match Path::new(path).parent().and_then(Path::to_str) {
        Some(dir) if !dir.is_empty() => dir,
        _ if path.is_empty() => ".",
        _ => path,
    }
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

#[cfg(test)]
mod tests {
    use super::{Asked, Spelling};

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
