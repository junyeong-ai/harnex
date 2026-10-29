//! The `paths:` grammar shared by rule and skill validation, `context
//! resolve` and `check`'s `paths-reading` arm.
//!
//! A value has two readings. Claude Code's decides when a rule loads or a
//! skill activates, and `context resolve` answers by it; the other is the
//! globs as written, in gitignore's syntax with case kept. They part only
//! where [`Reading`] says, and [`PathsReadingAuditor`] reports a value whose
//! dropped `/**` parts them on a file the project has, case folded on both
//! sides as Claude Code folds it.
//!
//! Neither reading is taken of a value [`DeclaredPaths::read`] refuses: an
//! entry Claude Code splits, expands or trims otherwise than its syntax says,
//! a glob the matcher Claude Code bundles (`ignore` 7.0.5), the latest one
//! (7.0.14) and, on ASCII names, git do not all read alike, a glob that can
//! match nothing it spells, an escape of a letter, a digit, `\`, `/`, `?` or
//! whitespace other than a space, and a bracket expression of anything but
//! plain characters and ascending ranges within one kind. The bundled release
//! hands both to a JavaScript regex, which gives some escaped letters and
//! digits a meaning and reads any other escape as its character, after
//! rewriting an escaped `\`, `/`, `?` or whitespace in some context; inside
//! brackets, what a member reads as turns on the characters around it. Each
//! is a [`Defect`], never a guess at what one release makes of it — the next
//! Claude Code may bundle another. A glob is judged as written, as the matcher
//! is given it with its
//! trailing `/**` dropped, and beside the `!` globs around it. Both readings
//! match UTF-16 code units, as the matcher does; git matches bytes, so `?` and
//! a bracket expression part from it on other names. Neither follows the
//! matcher in stopping a leading `**/` or a trailing `/**` at a line
//! terminator in a path, so neither answers for a path holding one
//! ([`is_answerable`]): `context resolve` refuses such a target, and the
//! auditor compares no such file.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use crate::envelope::{Finding, Location, Severity};
use crate::validate::frontmatter;

/// Whether a `paths:` value is a shape Claude Code reads as globs — a
/// comma-separated string or a list of strings.
pub(crate) fn is_glob_shaped(value: &yaml_serde::Value) -> bool {
    match value.as_sequence() {
        Some(seq) => seq.iter().all(yaml_serde::Value::is_string),
        None => value.is_string(),
    }
}

/// Whether a `paths:` value actually scopes the rule.
///
/// Presence of the key is not the question. Claude Code loads a rule at
/// launch when its globs, once each drops a trailing `/**`, are none or all
/// `**` — measured at 2.1.283: `paths:` with no value, an empty list, `**`
/// and `**/**` all load every session, while `["**", "src/**"]` waits for a
/// match. Reading the key alone would exempt such a rule from both the
/// always-loaded budget and the declaration requirement while it loads on
/// every turn.
pub(crate) fn declares_scope(value: Option<&yaml_serde::Value>) -> bool {
    value.is_some_and(|value| {
        expanded(value)
            .iter()
            .flat_map(|piece| &piece.globs)
            .any(|glob| {
                let read = glob.strip_suffix("/**").unwrap_or(glob);
                !read.is_empty() && read != "**"
            })
    })
}

/// The characters JavaScript's `.` does not match.
const LINE_TERMINATORS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

/// Whether the readings answer for `target` as Claude Code's matcher does.
/// Neither follows it in stopping a leading `**/` or a trailing `/**` at a
/// line terminator, so a path holding one is outside them.
pub(crate) fn is_answerable(target: &str) -> bool {
    !target.contains(LINE_TERMINATORS)
}

/// How a glob is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reading {
    drops_trailing_globstar: bool,
    folds_case: bool,
}

impl Reading {
    /// Claude Code's (2.1.311, 2.1.312). One trailing `/**` is dropped
    /// before matching, so `src/**` is a `src` directory at every depth and
    /// a `!` glob cannot re-include a file under it; case is folded, the
    /// `ignore` package's default, which Claude Code does not change.
    pub(crate) const CLAUDE_CODE: Self = Self {
        drops_trailing_globstar: true,
        folds_case: true,
    };

    /// The globs as written, case kept.
    pub(crate) const AS_WRITTEN: Self = Self {
        drops_trailing_globstar: false,
        folds_case: false,
    };
}

/// The globs a `paths:` value declares, in order, every one in the grammar
/// both readings share.
pub(crate) struct DeclaredPaths {
    globs: Vec<Glob>,
}

impl DeclaredPaths {
    /// The value's globs, or every defect that keeps it from being read.
    pub(crate) fn read(value: &yaml_serde::Value) -> Result<Self, Vec<Defect>> {
        let entry_defects: Vec<Defect> = entries(value)
            .into_iter()
            .filter_map(|entry| split_defect(entry).map(|kind| Defect::new(entry, kind)))
            .collect();
        if !entry_defects.is_empty() {
            return Err(entry_defects);
        }
        let mut read: Vec<(bool, Result<Glob, Defect>)> = Vec::new();
        for piece in expanded(value) {
            match piece.defect {
                Some(kind) => read.push((false, Err(Defect::new(&piece.text, kind)))),
                None => read.extend(
                    piece
                        .globs
                        .iter()
                        .filter(|glob| !glob.is_empty())
                        .map(|glob| (glob.starts_with('!'), Glob::parse(glob))),
                ),
            }
        }
        // A `!` glob narrows only what a glob before it matched.
        let first_included = read.iter().position(|(negated, _)| !negated);
        // The releases part on some globs once a `!` glob at or after them
        // re-includes a directory.
        let negated_through = read.iter().rposition(|(negated, _)| *negated);
        let mut globs = Vec::new();
        let mut defects = Vec::new();
        for (index, (negated, parsed)) in read.into_iter().enumerate() {
            match parsed {
                Ok(glob) => {
                    if negated && first_included.is_none_or(|first| index < first) {
                        defects.push(Defect::new(&glob.text, DefectKind::LeadingNegation));
                    } else if negated_through.is_some_and(|last| index <= last) {
                        defects.extend(glob.negation_defect());
                    }
                    globs.push(glob);
                }
                Err(defect) => defects.push(defect),
            }
        }
        if defects.is_empty() {
            Ok(Self { globs })
        } else {
            Err(defects)
        }
    }

    /// The scope the value declares under `reading`.
    pub(crate) fn scope(&self, reading: Reading) -> PathScope {
        PathScope {
            globs: self
                .globs
                .iter()
                .filter_map(|glob| glob.read(reading))
                .collect(),
            folds_case: reading.folds_case,
        }
    }
}

/// A glob [`DeclaredPaths::read`] refuses, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Defect {
    pub(crate) glob: String,
    /// What Claude Code's matcher is given instead, where dropping the
    /// trailing `/**` leaves a glob the written one is not.
    pub(crate) given_as: Option<String>,
    pub(crate) kind: DefectKind,
}

impl Defect {
    fn new(glob: &str, kind: DefectKind) -> Self {
        Self {
            glob: glob.to_string(),
            given_as: None,
            kind,
        }
    }

    /// How to write the glob so that every reader takes it alike.
    pub(crate) fn hint(&self) -> &'static str {
        self.kind.hint()
    }
}

impl fmt::Display for Defect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.given_as {
            None => write!(f, "'{}' {}", self.glob, self.kind),
            Some(given) => write!(
                f,
                "'{}', which Claude Code's matcher is given as '{given}' once the trailing `/**` \
                 is dropped, {}",
                self.glob, self.kind
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefectKind {
    CommaInBrackets,
    StrayBrace,
    EscapedSyntax,
    NestedBraces,
    BraceLimit,
    UnexpandedGroup,
    UnsplitComma,
    PatternTerminator,
    Comment,
    EmptyGlob,
    EmptyNegation,
    LeadingNegation,
    ByteOrderMark,
    DanglingEscape,
    Escape(char),
    EscapedTrailingStar,
    TrailingWhitespace,
    TrailingSpace,
    EmptySegment,
    StarRun,
    TrailingStarRun,
    LeadingGlobstars,
    GlobstarDirectory,
    RootGlobstar,
    UnclosedBracket,
    SlashInBrackets,
    Bracket(BracketDefect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BracketDefect {
    Negated,
    ClosingFirst,
    Wildcard,
    Escape,
    Opening,
    Range,
}

impl fmt::Display for DefectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const RELEASES: &str = "which the releases of Claude Code's matcher read differently";
        match self {
            Self::CommaInBrackets => write!(
                f,
                "has a comma inside `[...]`, where Claude Code splits the entry as at any comma \
                 outside braces"
            ),
            Self::StrayBrace => write!(
                f,
                "has a `}}` without its pair, so Claude Code splits the brace group after it at \
                 its commas"
            ),
            Self::EscapedSyntax => write!(
                f,
                "escapes a comma, a brace or trailing whitespace, which Claude Code splits, \
                 expands or trims at before it reads escapes"
            ),
            Self::NestedBraces => write!(
                f,
                "nests a brace group, which Claude Code expands only up to the first `}}`"
            ),
            Self::BraceLimit => write!(
                f,
                "expands past what Claude Code expands one `paths:` value to (1,000 globs, \
                 4,194,304 UTF-16 code units with every intermediate expansion counted), so it \
                 is read with its braces"
            ),
            Self::UnexpandedGroup => write!(
                f,
                "has a brace group Claude Code leaves unexpanded, after a `{{}}` or before a line \
                 terminator, so it reads the braces as literals"
            ),
            Self::UnsplitComma => write!(
                f,
                "has a comma Claude Code neither splits at nor expands, because a brace before it \
                 is escaped or unpaired, so it reads as a literal comma"
            ),
            Self::PatternTerminator => {
                write!(
                    f,
                    "holds a NUL or a line feed, where a gitignore pattern ends"
                )
            }
            Self::Comment => write!(
                f,
                "starts with `#`, which makes it a comment that matches nothing"
            ),
            Self::EmptyGlob => write!(
                f,
                "is nothing but whitespace once Claude Code expands its braces, so it matches \
                 nothing"
            ),
            Self::EmptyNegation => write!(f, "is a `!` with no glob after it, {RELEASES}"),
            Self::LeadingNegation => write!(
                f,
                "is a `!` glob with no glob before it to narrow, so it excludes nothing"
            ),
            Self::ByteOrderMark => write!(
                f,
                "has a byte-order mark where it starts or after its `!`, which the releases of \
                 Claude Code's matcher and git strip in some positions and keep in others"
            ),
            Self::DanglingEscape => write!(f, "ends in `\\`, which escapes nothing"),
            Self::Escape(escaped) if escaped.is_ascii_alphanumeric() => write!(
                f,
                "escapes `{escaped}`, and the matcher Claude Code bundles reads some escaped \
                 letters and digits as regular-expression syntax (`\\d` is any digit), so none \
                 is held to read as itself"
            ),
            Self::Escape(escaped) => write!(
                f,
                "escapes `{}`, which the releases of Claude Code's matcher read differently in \
                 some context",
                escaped.escape_debug()
            ),
            Self::EscapedTrailingStar => write!(f, "ends in an escaped `*`, {RELEASES}"),
            Self::TrailingWhitespace => write!(
                f,
                "ends in whitespace other than a space, where only trailing spaces are held to \
                 be trimmed alike by every release of Claude Code's matcher"
            ),
            Self::TrailingSpace => write!(f, "ends in spaces after a `/`, {RELEASES}"),
            Self::EmptySegment => write!(
                f,
                "has an empty, `.` or `..` path segment, which no project path has"
            ),
            Self::StarRun => write!(
                f,
                "has a segment of three or more `*` before another segment, {RELEASES}"
            ),
            Self::TrailingStarRun => write!(
                f,
                "ends in a segment of three or more `*` with a `!` glob at or after it, \
                 {RELEASES}"
            ),
            Self::LeadingGlobstars => write!(f, "starts with `**/**/`, {RELEASES}"),
            Self::GlobstarDirectory => {
                write!(f, "ends in `/**/` after a named segment, {RELEASES}")
            }
            Self::RootGlobstar => {
                write!(f, "is `/**` with a `!` glob at or after it, {RELEASES}")
            }
            Self::UnclosedBracket => write!(f, "opens `[` without closing it"),
            Self::SlashInBrackets => write!(f, "has `/` inside `[...]`, {RELEASES}"),
            Self::Bracket(defect) => {
                let shape = match defect {
                    BracketDefect::Negated => "that starts with `!` or `^`",
                    BracketDefect::ClosingFirst => "that starts with `]`",
                    BracketDefect::Wildcard => "holding a `*` or `?`",
                    BracketDefect::Escape => "holding a `\\`",
                    BracketDefect::Opening => "holding a `[`",
                    BracketDefect::Range => "with a range that descends or leaves one kind",
                };
                match defect {
                    BracketDefect::Negated
                    | BracketDefect::ClosingFirst
                    | BracketDefect::Wildcard => {
                        write!(f, "has a bracket expression {shape}, {RELEASES}")
                    }
                    BracketDefect::Escape | BracketDefect::Opening | BracketDefect::Range => {
                        write!(
                            f,
                            "has a bracket expression {shape}, where only plain characters and \
                             ascending ranges within digits, lower-case letters, upper-case \
                             letters or characters past ASCII are held to read alike in every \
                             release of Claude Code's matcher"
                        )
                    }
                }
            }
        }
    }
}

impl DefectKind {
    fn hint(self) -> &'static str {
        match self {
            Self::CommaInBrackets => "write alternatives as `{a,b}` instead",
            Self::StrayBrace => "remove the `}`, or match a literal `}` with `?`",
            Self::EscapedSyntax => "match a literal comma, brace or trailing whitespace with `?`",
            Self::NestedBraces => {
                "flatten it into one group, `{a,b,c}`, list the alternatives as entries, or match \
                 a literal brace with `?`"
            }
            Self::BraceLimit => "cut alternatives; the limit counts every entry of the value",
            Self::UnexpandedGroup => {
                "remove the `{}` or the line terminator, or match a literal brace with `?`"
            }
            Self::UnsplitComma => {
                "pair each `{` with a `}`, or match a literal comma or brace with `?`"
            }
            Self::PatternTerminator => {
                "list each glob as its own entry, or separate globs with commas"
            }
            Self::Comment => "escape a literal `#` as `\\#`, or remove the entry",
            Self::EmptyGlob => "remove the alternatives that leave only whitespace",
            Self::EmptyNegation => "remove the `!`",
            Self::LeadingNegation => "list it after the globs it narrows, or remove it",
            Self::ByteOrderMark => "remove the byte-order mark",
            Self::DanglingEscape => "remove the backslash",
            Self::Escape('?') => "match a literal `?` with `?`",
            Self::Escape('\\') => "match the backslash with `?`",
            Self::Escape(escaped) if escaped.is_ascii_alphanumeric() => {
                "remove the backslash; a glob separates directories with `/`"
            }
            Self::Escape(_) => "remove the backslash",
            Self::EscapedTrailingStar => "match the trailing `*` with `?`",
            Self::TrailingWhitespace => "remove the whitespace, or match it with `?`",
            Self::TrailingSpace => "remove the spaces after the `/`",
            Self::EmptySegment => "remove the segment",
            Self::StarRun => "write `**`",
            Self::TrailingStarRun => "write `*` for one segment, or `**/*` for every depth",
            Self::LeadingGlobstars => "write one `**`",
            Self::GlobstarDirectory => "write `dir/**/*` for what is under `dir`",
            Self::RootGlobstar => "write `**`, or `!**` for its negation",
            Self::UnclosedBracket => "close it, or escape a literal `[` as `\\[`",
            Self::SlashInBrackets => "keep `/` out of the bracket expression",
            Self::Bracket(BracketDefect::Negated) => {
                "list the characters it should match; no negated bracket expression reads alike \
                 across the releases"
            }
            Self::Bracket(_) => {
                "list plain characters and ascending ranges within one kind, as `[abc]` or \
                 `[a-c]`, or escape a literal `[` as `\\[`"
            }
        }
    }
}

/// A `paths:` value read one way: a file is covered when the last glob
/// matching it, or a directory above it, is not a `!` glob, and a file under
/// a covered directory stays covered whatever a later `!` glob says.
pub(crate) struct PathScope {
    globs: Vec<ScopeGlob>,
    folds_case: bool,
}

impl PathScope {
    /// Whether the scope covers `target`, a project-relative file path
    /// written with `/`.
    pub(crate) fn matches(&self, target: &str) -> bool {
        self.covers(target, &mut HashMap::new())
    }

    /// Whether the scope covers each of `files`, each directory read once.
    fn covers_each(&self, files: &[&str]) -> Vec<bool> {
        let mut directories = HashMap::new();
        files
            .iter()
            .map(|file| self.covers(file, &mut directories))
            .collect()
    }

    fn covers<'p>(&self, file: &'p str, directories: &mut HashMap<&'p str, bool>) -> bool {
        self.directory_covered(parent(file), directories) || self.ignores(file, false)
    }

    fn directory_covered<'p>(&self, dir: &'p str, memo: &mut HashMap<&'p str, bool>) -> bool {
        if dir.is_empty() {
            return false;
        }
        if let Some(&covered) = memo.get(dir) {
            return covered;
        }
        let covered = self.directory_covered(parent(dir), memo) || self.ignores(dir, true);
        memo.insert(dir, covered);
        covered
    }

    fn ignores(&self, path: &str, directory: bool) -> bool {
        let segments = units(path);
        self.globs.iter().fold(false, |covered, glob| {
            if (directory || !glob.directory_only) && glob.matches(&segments, self.folds_case) {
                !glob.negated
            } else {
                covered
            }
        })
    }
}

fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..slash])
}

/// A project-relative path's segments in UTF-16 code units, the units
/// Claude Code's matcher reads a path in.
fn units(path: &str) -> Vec<Vec<u16>> {
    path.split('/')
        .map(|segment| segment.encode_utf16().collect())
        .collect()
}

/// A rule or skill whose `paths:` has a glob Claude Code drops a trailing
/// `/**` from: the only value [`PathsReadingAuditor`] reads the project's
/// files for.
pub(crate) struct DroppingPaths {
    declared: DeclaredPaths,
    location: Location,
}

impl DroppingPaths {
    /// The `paths:` value of the rule or skill at `path`, if a glob in it
    /// drops a trailing `/**`. A value the validators refuse is theirs to
    /// report.
    pub(crate) fn read(content: &str, path: &Path) -> Option<Self> {
        let fm = frontmatter::parse(content, path).ok()??;
        let fields: yaml_serde::Value = yaml_serde::from_str(&fm.yaml_text).ok()?;
        let value = fields.get("paths")?;
        if !is_glob_shaped(value) || !declares_scope(Some(value)) {
            return None;
        }
        let declared = DeclaredPaths::read(value).ok()?;
        declared.globs.iter().any(Glob::drops).then(|| Self {
            declared,
            location: Location::line(path.to_path_buf(), fm.begin_line),
        })
    }
}

/// Reports a rule or skill whose `paths:` covers other files of the project
/// once Claude Code drops a trailing `/**` than as written.
pub(crate) struct PathsReadingAuditor {
    files: Vec<String>,
    segments: Vec<Vec<Vec<u16>>>,
}

impl PathsReadingAuditor {
    /// `files` are the project's, project-relative and written with `/`; a
    /// file outside what the readings answer for is not compared.
    pub(crate) fn new(mut files: Vec<String>) -> Self {
        files.retain(|file| is_answerable(file));
        let segments = files.iter().map(|file| units(file)).collect();
        Self { files, segments }
    }

    /// The finding for `paths`, if its readings part on a project file.
    pub(crate) fn audit(&self, paths: &DroppingPaths) -> Option<Finding> {
        let declared = &paths.declared;
        let dropping: Vec<&Glob> = declared.globs.iter().filter(|glob| glob.drops()).collect();
        // Both sides fold case as Claude Code does, so the dropped `/**` is
        // the one difference left between them.
        let folds_case = Reading::CLAUDE_CODE.folds_case;
        // Every other glob matches alike on both sides, so a file no
        // dropping glob may part is covered alike.
        let candidates: Vec<&str> = self
            .files
            .iter()
            .zip(&self.segments)
            .filter(|(_, segments)| {
                dropping
                    .iter()
                    .any(|glob| glob.may_part(segments, folds_case))
            })
            .map(|(file, _)| file.as_str())
            .collect();
        let covers = |drops: &[bool]| {
            let globs = declared
                .globs
                .iter()
                .zip(drops)
                .filter_map(|(glob, &drop)| {
                    glob.read(if drop {
                        Reading::CLAUDE_CODE
                    } else {
                        Reading::AS_WRITTEN
                    })
                })
                .collect();
            PathScope { globs, folds_case }.covers_each(&candidates)
        };
        let all_drops: Vec<bool> = declared.globs.iter().map(Glob::drops).collect();
        let dropped = covers(&all_drops);
        let written = covers(&vec![false; all_drops.len()]);
        let (applied, withheld): (Vec<usize>, Vec<usize>) = (0..candidates.len())
            .filter(|&i| dropped[i] != written[i])
            .partition(|&i| dropped[i]);
        if applied.is_empty() && withheld.is_empty() {
            return None;
        }
        // Each glob in turn keeps its dropped `/**` where the value still
        // covers what it does as written, so applying every rewrite left
        // clears the finding.
        let mut drops = vec![false; all_drops.len()];
        let mut rewrites: Vec<String> = Vec::new();
        for (index, glob) in declared.globs.iter().enumerate() {
            if !glob.drops() {
                continue;
            }
            drops[index] = true;
            if covers(&drops) != written {
                drops[index] = false;
                let rewrite = glob.rewrite();
                if !rewrites.contains(&rewrite) {
                    rewrites.push(rewrite);
                }
            }
        }
        let mut parts = Vec::new();
        if let Some(&first) = applied.first() {
            parts.push(format!(
                "applies to {} the globs as written leave out, such as `{}`",
                file_count(applied.len()),
                candidates[first]
            ));
        }
        if let Some(&first) = withheld.first() {
            parts.push(format!(
                "does not apply to {} the globs as written cover, such as `{}`",
                file_count(withheld.len()),
                candidates[first]
            ));
        }
        Some(Finding {
            slug: "paths-reading-differs".into(),
            severity: Severity::Minor,
            location: paths.location.clone(),
            message: format!(
                "Claude Code drops a trailing `/**` from `paths:`, so it {}",
                parts.join(", and ")
            ),
            hint: Some(format!(
                "A glob with its `/**` dropped matches the directory itself, at every depth \
                 unless another `/` anchors it, and no `!` glob re-includes a file under it: {}",
                rewrites.join("; ")
            )),
        })
    }
}

fn file_count(count: usize) -> String {
    match count {
        1 => "1 file".to_string(),
        _ => format!("{count} files"),
    }
}

/// One glob in the grammar both readings share.
struct Glob {
    text: String,
    as_written: Pattern,
    /// What Claude Code's matcher is given: the glob with a trailing `/**`
    /// dropped, `None` when nothing is left.
    as_given: Option<Pattern>,
}

#[derive(Clone)]
struct Pattern {
    negated: bool,
    anchored: bool,
    directory_only: bool,
    segments: Vec<Segment>,
    /// How the releases read the glob differently once it or a later glob
    /// is negated, a `!` glob then re-including a directory under it.
    negation_defect: Option<DefectKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Globstar,
    Name(Vec<Atom>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    /// A UTF-16 code unit, the unit Claude Code's matcher reads a glob in.
    Unit(u16),
    AnyChar,
    AnyRun,
    Class(Vec<(u16, u16)>),
}

enum Token {
    Atom(Atom),
    Slash,
}

impl Glob {
    fn parse(text: &str) -> Result<Self, Defect> {
        let as_written = Pattern::parse(text).map_err(|kind| Defect::new(text, kind))?;
        // A negated glob is a `!` glob at itself.
        if let Some(kind) = as_written.negation_defect.filter(|_| as_written.negated) {
            return Err(Defect::new(text, kind));
        }
        let as_given = match text.strip_suffix("/**") {
            None => Some(as_written.clone()),
            Some("") => None,
            Some(given) => {
                let defect = |kind| Defect {
                    glob: text.to_string(),
                    given_as: Some(given.to_string()),
                    kind,
                };
                let pattern = Pattern::parse(given).map_err(defect)?;
                if let Some(kind) = pattern.negation_defect.filter(|_| pattern.negated) {
                    return Err(defect(kind));
                }
                Some(pattern)
            }
        };
        Ok(Self {
            text: text.to_string(),
            as_written,
            as_given,
        })
    }

    /// Whether Claude Code drops a trailing `/**` from the glob.
    fn drops(&self) -> bool {
        self.text.ends_with("/**")
    }

    /// The defect the releases part on once the glob or a later one is
    /// negated, as written or as the matcher is given it.
    fn negation_defect(&self) -> Option<Defect> {
        if let Some(kind) = self.as_written.negation_defect {
            return Some(Defect::new(&self.text, kind));
        }
        let kind = self.as_given.as_ref()?.negation_defect?;
        Some(Defect {
            glob: self.text.clone(),
            given_as: self.text.strip_suffix("/**").map(str::to_string),
            kind,
        })
    }

    /// Whether dropping the glob's trailing `/**` can change what it matches
    /// of a file with `segments` or of a directory above it: either reading
    /// matches only where one of them matches the glob's last segment before
    /// the `/**`.
    fn may_part(&self, segments: &[Vec<u16>], folds_case: bool) -> bool {
        let written = &self.as_written.segments;
        let before_globstar = written.len().checked_sub(2).map(|at| &written[at]);
        let given = self
            .as_given
            .as_ref()
            .and_then(|given| given.segments.last());
        [before_globstar, given].into_iter().any(|last| match last {
            Some(Segment::Name(atoms)) => segments
                .iter()
                .any(|segment| name_matches(atoms, segment, folds_case)),
            _ => true,
        })
    }

    /// The glob as `reading` matches it, or `None` when that reading drops it.
    fn read(&self, reading: Reading) -> Option<ScopeGlob> {
        let pattern = if reading.drops_trailing_globstar {
            self.as_given.as_ref()?
        } else {
            &self.as_written
        };
        Some(pattern.scope_glob())
    }

    /// The spellings of a glob ending in `/**` that both readings take alike.
    fn rewrite(&self) -> String {
        let under = format!("{}/*", self.text);
        let pattern = &self.as_written;
        // `**/*/**/*` covers what `*/**/*` does.
        let unanchored_directory = !pattern.anchored
            && pattern.segments.len() == 2
            && matches!(&pattern.segments[0], Segment::Name(atoms) if *atoms != [Atom::AnyRun]);
        if unanchored_directory {
            let body = self.text.strip_prefix('!').unwrap_or(&self.text);
            let bang = if pattern.negated { "!" } else { "" };
            format!(
                "write `{under}` for what `{}` means as written, or `{bang}**/{body}/*` for every \
                 depth",
                self.text
            )
        } else {
            format!("write `{under}` for what `{}` means as written", self.text)
        }
    }
}

impl Pattern {
    fn parse(text: &str) -> Result<Self, DefectKind> {
        if text.contains(['\0', '\n']) {
            return Err(DefectKind::PatternTerminator);
        }
        if text.starts_with('#') {
            return Err(DefectKind::Comment);
        }
        let (text, trimmed) = without_trailing_spaces(text)?;
        if text.is_empty() {
            return Err(DefectKind::EmptyGlob);
        }
        let (negated, rest) = match text.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if rest.is_empty() {
            return Err(DefectKind::EmptyNegation);
        }
        if rest.starts_with('\u{feff}') {
            return Err(DefectKind::ByteOrderMark);
        }
        let (anchored, rest) = match rest.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let mut names: Vec<Vec<Atom>> = vec![Vec::new()];
        for token in tokens(rest)? {
            match token {
                Token::Atom(atom) => names.last_mut().expect("a name is open").push(atom),
                Token::Slash => names.push(Vec::new()),
            }
        }
        let directory_only = names.len() > 1 && names.last().is_some_and(Vec::is_empty);
        if directory_only {
            names.pop();
        }
        let star_run =
            |name: &Vec<Atom>| name.len() > 2 && name.iter().all(|atom| *atom == Atom::AnyRun);
        if names[..names.len() - 1].iter().any(star_run) {
            return Err(DefectKind::StarRun);
        }
        let ends_in_star_run = names.last().is_some_and(star_run);
        let segments = names
            .into_iter()
            .map(segment)
            .collect::<Result<Vec<_>, _>>()?;
        // The bundled release anchors `dir/` before it trims the spaces after,
        // which parts the readings wherever anchoring does: on one segment
        // other than `**`.
        if trimmed
            && directory_only
            && !anchored
            && segments.len() == 1
            && segments[0] != Segment::Globstar
        {
            return Err(DefectKind::TrailingSpace);
        }
        if segments.len() > 2 && segments[..2] == [Segment::Globstar, Segment::Globstar] {
            return Err(DefectKind::LeadingGlobstars);
        }
        let named_before_last = segments[..segments.len() - 1]
            .iter()
            .any(|segment| matches!(segment, Segment::Name(_)));
        if directory_only && segments.last() == Some(&Segment::Globstar) && named_before_last {
            return Err(DefectKind::GlobstarDirectory);
        }
        if !directory_only
            && matches!(segments.last(), Some(Segment::Name(atoms))
                if atoms.last() == Some(&Atom::Unit(u16::from(b'*'))))
        {
            return Err(DefectKind::EscapedTrailingStar);
        }
        let negation_defect = if anchored && !directory_only && segments == [Segment::Globstar] {
            Some(DefectKind::RootGlobstar)
        } else if ends_in_star_run && (anchored || named_before_last) {
            Some(DefectKind::TrailingStarRun)
        } else {
            None
        };
        Ok(Self {
            negated,
            anchored,
            directory_only,
            segments,
            negation_defect,
        })
    }

    fn scope_glob(&self) -> ScopeGlob {
        let mut segments = self.segments.clone();
        // A `/` before the end anchors a glob at the root; one with none
        // matches at every depth.
        if !self.anchored && segments.len() == 1 && segments[0] != Segment::Globstar {
            segments.insert(0, Segment::Globstar);
        }
        ScopeGlob {
            negated: self.negated,
            directory_only: self.directory_only,
            segments,
        }
    }
}

/// `text` without the trailing spaces every reader trims, an escaped one
/// kept, and whether there were any. Only trailing spaces are held to be
/// trimmed alike, so any other whitespace there is a defect.
fn without_trailing_spaces(text: &str) -> Result<(&str, bool), DefectKind> {
    let mut end = 0;
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            end = chars.offset();
        } else if !is_js_space(c) {
            end = index + c.len_utf8();
        }
    }
    if text[end..].chars().all(|c| c == ' ') {
        Ok((&text[..end], end < text.len()))
    } else {
        Err(DefectKind::TrailingWhitespace)
    }
}

fn tokens(text: &str) -> Result<Vec<Token>, DefectKind> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next().ok_or(DefectKind::DanglingEscape)? {
                escaped if escape_held_alike(escaped) => push_units(&mut out, escaped),
                escaped => return Err(DefectKind::Escape(escaped)),
            },
            // Claude Code splits at or expands every comma its syntax
            // separates at; one left is one it reads as a literal.
            ',' => return Err(DefectKind::UnsplitComma),
            '*' => out.push(Token::Atom(Atom::AnyRun)),
            '?' => out.push(Token::Atom(Atom::AnyChar)),
            '/' => out.push(Token::Slash),
            '[' => out.push(Token::Atom(Atom::Class(bracket(&mut chars)?))),
            c => push_units(&mut out, c),
        }
    }
    Ok(out)
}

/// Whether an escape of `escaped` outside a bracket expression is one every
/// release is held to read as the character: a space, ASCII punctuation other
/// than `\`, `/` and `?`, or a character past ASCII. The bundled release hands
/// an escape to a JavaScript regex, which gives some escaped letters and
/// digits a meaning (`\d`, `\b`, `\1`), so every letter and digit is refused
/// rather than the ones a release reads apart today; it rewrites an escaped
/// `\`, `/`, `?` or other whitespace in some context.
fn escape_held_alike(escaped: char) -> bool {
    !(escaped.is_ascii_alphanumeric()
        || matches!(escaped, '\\' | '/' | '?')
        || escaped != ' ' && is_js_space(escaped))
}

fn push_units(out: &mut Vec<Token>, c: char) {
    for &unit in c.encode_utf16(&mut [0; 2]).iter() {
        out.push(Token::Atom(Atom::Unit(unit)));
    }
}

/// The ranges of a bracket expression whose `[` is consumed, in UTF-16 code
/// units: the matcher reads a character outside the Basic Multilingual Plane
/// there as two members.
fn bracket(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<Vec<(u16, u16)>, DefectKind> {
    if !chars.clone().any(|c| c == ']') {
        return Err(DefectKind::UnclosedBracket);
    }
    match chars.peek() {
        Some('!' | '^') => return Err(DefectKind::Bracket(BracketDefect::Negated)),
        Some(']') => return Err(DefectKind::Bracket(BracketDefect::ClosingFirst)),
        _ => {}
    }
    let mut members: Vec<u16> = Vec::new();
    for c in chars.by_ref() {
        match c {
            ']' => return class_ranges(&members),
            '*' | '?' => return Err(DefectKind::Bracket(BracketDefect::Wildcard)),
            '\\' => return Err(DefectKind::Bracket(BracketDefect::Escape)),
            '[' => return Err(DefectKind::Bracket(BracketDefect::Opening)),
            '/' => return Err(DefectKind::SlashInBrackets),
            c => members.extend(c.encode_utf16(&mut [0; 2]).iter()),
        }
    }
    Err(DefectKind::UnclosedBracket)
}

/// The ranges `members` spell, a `-` between two members joining them.
fn class_ranges(members: &[u16]) -> Result<Vec<(u16, u16)>, DefectKind> {
    let dash = u16::from(b'-');
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < members.len() {
        let low = members[at];
        let (high, next) = match members.get(at + 1..at + 3) {
            Some(&[d, high]) if d == dash => (high, at + 3),
            _ => (low, at + 1),
        };
        let one_kind = range_kind(low).is_some() && range_kind(low) == range_kind(high);
        if next == at + 3 && !(low <= high && one_kind) {
            return Err(DefectKind::Bracket(BracketDefect::Range));
        }
        ranges.push((low, high));
        at = next;
    }
    Ok(ranges)
}

/// The kinds a range stays within: digits, lower-case and upper-case ASCII
/// letters, and characters past ASCII that are one code unit each.
fn range_kind(unit: u16) -> Option<u8> {
    match unit {
        0x30..=0x39 => Some(0),
        0x61..=0x7a => Some(1),
        0x41..=0x5a => Some(2),
        0x80..=0xd7ff | 0xe000..=0xffff => Some(3),
        _ => None,
    }
}

fn segment(mut atoms: Vec<Atom>) -> Result<Segment, DefectKind> {
    if atoms == [Atom::AnyRun, Atom::AnyRun] {
        return Ok(Segment::Globstar);
    }
    let dot = u16::from(b'.');
    let only_dot = |atom: &Atom| match atom {
        Atom::Unit(unit) => *unit == dot,
        Atom::Class(ranges) => ranges.iter().all(|&range| range == (dot, dot)),
        Atom::AnyChar | Atom::AnyRun => false,
    };
    if atoms.is_empty() || atoms.len() <= 2 && atoms.iter().all(only_dot) {
        return Err(DefectKind::EmptySegment);
    }
    // A run of `*`, three or more in a segment of their own included, reads
    // as one wherever the releases agree on it.
    atoms.dedup_by(|later, earlier| *later == Atom::AnyRun && *earlier == Atom::AnyRun);
    Ok(Segment::Name(atoms))
}

struct ScopeGlob {
    negated: bool,
    directory_only: bool,
    segments: Vec<Segment>,
}

impl ScopeGlob {
    /// Whether the glob matches `path`, its segments in UTF-16 code units. A
    /// `**` matches any run of segments, but a trailing one at least one, so
    /// `dir/**` is what is under `dir` and not `dir` itself.
    fn matches(&self, path: &[Vec<u16>], folds_case: bool) -> bool {
        let mut reached = vec![false; path.len() + 1];
        reached[0] = true;
        for (index, segment) in self.segments.iter().enumerate() {
            let mut next = vec![false; path.len() + 1];
            match segment {
                Segment::Globstar => {
                    let trailing = index + 1 == self.segments.len();
                    let mut seen = false;
                    for position in 0..=path.len() {
                        if trailing {
                            next[position] = seen;
                            seen |= reached[position];
                        } else {
                            seen |= reached[position];
                            next[position] = seen;
                        }
                    }
                }
                Segment::Name(atoms) => {
                    for position in 0..path.len() {
                        next[position + 1] =
                            reached[position] && name_matches(atoms, &path[position], folds_case);
                    }
                }
            }
            reached = next;
        }
        reached[path.len()]
    }
}

fn name_matches(atoms: &[Atom], name: &[u16], folds_case: bool) -> bool {
    let (mut atom, mut at) = (0, 0);
    let mut resume: Option<(usize, usize)> = None;
    while at < name.len() {
        match atoms.get(atom) {
            Some(Atom::AnyRun) => {
                resume = Some((atom, at));
                atom += 1;
            }
            Some(single) if single.accepts(name[at], folds_case) => {
                atom += 1;
                at += 1;
            }
            _ => match resume {
                Some((star, from)) => {
                    atom = star + 1;
                    at = from + 1;
                    resume = Some((star, from + 1));
                }
                None => return false,
            },
        }
    }
    atoms[atom..].iter().all(|rest| *rest == Atom::AnyRun)
}

impl Atom {
    /// Whether the atom takes `unit`, folding case as ECMAScript's `i` flag
    /// does without `u`: two units are alike when their [`canonical`] forms
    /// are, and a class takes a unit when it holds one alike.
    fn accepts(&self, unit: u16, folds_case: bool) -> bool {
        match self {
            Self::Unit(expected) => {
                unit == *expected || folds_case && canonical(unit) == canonical(*expected)
            }
            Self::AnyChar => true,
            Self::Class(ranges) => {
                let within = |unit: u16| {
                    ranges
                        .iter()
                        .any(|&(low, high)| (low..=high).contains(&unit))
                };
                within(unit)
                    || folds_case
                        && CASE_GROUPS
                            .get(&canonical(unit))
                            .is_some_and(|group| group.iter().copied().any(within))
            }
            Self::AnyRun => false,
        }
    }
}

/// ECMAScript's `Canonicalize` without the `u` flag: the unit's upper case,
/// unless that is not one code unit or takes a non-ASCII unit to ASCII.
fn canonical(unit: u16) -> u16 {
    let Some(c) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(upper), None) => match u16::try_from(u32::from(upper)) {
            Ok(folded) if unit < 128 || folded >= 128 => folded,
            _ => unit,
        },
        _ => unit,
    }
}

/// Every code unit by its [`canonical`] form, where more than one shares it.
static CASE_GROUPS: LazyLock<HashMap<u16, Vec<u16>>> = LazyLock::new(|| {
    let mut groups: HashMap<u16, Vec<u16>> = HashMap::new();
    for unit in 0..=u16::MAX {
        groups.entry(canonical(unit)).or_default().push(unit);
    }
    groups.retain(|_, group| group.len() > 1);
    groups
});

/// Why Claude Code splits an entry otherwise than its syntax says. It splits
/// at each comma its count of braces leaves at zero, counting an escaped
/// brace and an unpaired `}` alike, and trims each piece, before it reads an
/// escape or a bracket expression.
fn split_defect(entry: &str) -> Option<DefectKind> {
    let paired: HashSet<usize> = paired_braces(entry)
        .into_iter()
        .flat_map(|(open, close)| [open, close])
        .collect();
    // Claude Code's count, the count without escaped braces, and the depth
    // of the paired groups the syntax has.
    let (mut depth, mut unescaped_depth, mut group_depth) = (0i64, 0i64, 0i64);
    let mut bracket_members: Option<usize> = None;
    let mut chars = entry.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, '{')) => depth += 1,
                Some((_, '}')) => depth -= 1,
                Some((_, ',')) if depth == 0 => return Some(DefectKind::EscapedSyntax),
                _ => {}
            },
            '{' | '}' => {
                let step = if c == '{' { 1 } else { -1 };
                depth += step;
                unescaped_depth += step;
                if paired.contains(&index) {
                    group_depth += step;
                }
            }
            ',' if depth == 0 => {
                // Inside a group the syntax separates alternatives here; the
                // braces that offset the count are escaped or unpaired.
                if group_depth > 0 {
                    return Some(if unescaped_depth == 0 {
                        DefectKind::StrayBrace
                    } else {
                        DefectKind::EscapedSyntax
                    });
                }
                if bracket_members.is_some() {
                    return Some(DefectKind::CommaInBrackets);
                }
                // A comma is neither whitespace nor a `\`, so the text before
                // it trims as the piece it ends does.
                if trims_escape(&entry[..index]) {
                    return Some(DefectKind::EscapedSyntax);
                }
                continue;
            }
            _ => {}
        }
        match c {
            '[' if bracket_members.is_none() && depth == 0 => bracket_members = Some(0),
            ']' if bracket_members.is_some_and(|members| members > 0) => bracket_members = None,
            _ => {
                if let Some(members) = &mut bracket_members {
                    *members += 1;
                }
            }
        }
    }
    trims_escape(entry).then_some(DefectKind::EscapedSyntax)
}

/// The byte offsets of each unescaped `{` and the `}` it pairs with, as
/// nested groups pair.
fn paired_braces(text: &str) -> Vec<(usize, usize)> {
    let mut open = Vec::new();
    let mut paired = Vec::new();
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => open.push(index),
            '}' => {
                if let Some(start) = open.pop() {
                    paired.push((start, index));
                }
            }
            _ => {}
        }
    }
    paired
}

/// Whether trimming `text` as JavaScript does leaves a `\` that escaped the
/// whitespace trimmed after it.
fn trims_escape(text: &str) -> bool {
    let trimmed = text.trim_end_matches(is_js_space);
    trimmed.len() < text.len() && ends_in_escape(trimmed)
}

/// Whether `text` ends in a `\` that escapes what follows it.
fn ends_in_escape(text: &str) -> bool {
    text.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

fn entries(value: &yaml_serde::Value) -> Vec<&str> {
    match value.as_sequence() {
        Some(seq) => seq.iter().filter_map(yaml_serde::Value::as_str).collect(),
        None => value.as_str().into_iter().collect(),
    }
}

/// A piece Claude Code splits an entry into, and the globs it expands the
/// piece to before it drops a trailing `/**`.
struct Piece {
    text: String,
    globs: Vec<String>,
    /// Why the expansion is not what the piece's braces say.
    defect: Option<DefectKind>,
}

/// The pieces Claude Code reads from a `paths:` value (2.1.311), in order.
///
/// Each string — the value, or each entry of a list — is split at every comma
/// outside braces, a bracket expression's included, and each piece trimmed as
/// JavaScript trims. Each piece's brace groups are then expanded, the first
/// `{` with the first `}` after it, each alternative trimmed, under one budget
/// of [`BRACE_RESULTS_MAX`] patterns and [`BRACE_BYTES_MAX`] the whole value
/// shares; a piece that would exceed it stays unexpanded.
fn expanded(value: &yaml_serde::Value) -> Vec<Piece> {
    let mut budget = BraceBudget {
        results: BRACE_RESULTS_MAX,
        bytes: BRACE_BYTES_MAX,
    };
    entries(value)
        .into_iter()
        .flat_map(split_entry)
        .map(|piece| expand_braces(piece, &mut budget))
        .collect()
}

const BRACE_RESULTS_MAX: i64 = 1_000;
const BRACE_BYTES_MAX: i64 = 4 * 1024 * 1024;

/// What is left of the shared expansion budget, counted in UTF-16 code units
/// as the runtime counts a string's length.
struct BraceBudget {
    results: i64,
    bytes: i64,
}

fn split_entry(entry: &str) -> Vec<String> {
    let mut depth = 0i64;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (index, ch) in entry.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(js_trim(&entry[start..index]).to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    pieces.push(js_trim(&entry[start..]).to_string());
    pieces.retain(|piece| !piece.is_empty());
    pieces
}

fn expand_braces(text: String, budget: &mut BraceBudget) -> Piece {
    if !text.contains('{') {
        return Piece {
            globs: vec![text.clone()],
            text,
            defect: None,
        };
    }
    let length = js_len(&text);
    let mut defect = None;
    let mut globs = Vec::new();
    let mut pending = vec![text.clone()];
    while let Some(glob) = pending.pop() {
        let Some((head, body, tail)) = first_group(&glob) else {
            // Claude Code expands nothing past a `{` it cannot take a group
            // at.
            if paired_braces(&glob)
                .iter()
                .any(|&(open, close)| close > open + 1)
            {
                defect = defect.or(Some(DefectKind::UnexpandedGroup));
            }
            globs.push(glob);
            continue;
        };
        let raw: Vec<&str> = body.split(',').collect();
        defect = defect.or_else(|| group_defect(head, body, &raw));
        budget.bytes -= js_len(&glob);
        let count = (globs.len() + pending.len() + raw.len()) as i64;
        if budget.bytes < 0 || count > budget.results || count * length > budget.bytes {
            return Piece {
                globs: vec![text.clone()],
                text,
                defect: Some(DefectKind::BraceLimit),
            };
        }
        for alternative in raw.iter().rev() {
            pending.push(format!("{head}{}{tail}", js_trim(alternative)));
        }
    }
    budget.results -= globs.len() as i64;
    budget.bytes -= globs.len() as i64 * length;
    Piece {
        text,
        globs,
        defect,
    }
}

/// Why expanding the group `{body}` after `head` is not what its braces say:
/// Claude Code takes the first `{` and the first `}` after it, splits at
/// every comma between and trims each alternative, all before it reads an
/// escape.
fn group_defect(head: &str, body: &str, alternatives: &[&str]) -> Option<DefectKind> {
    // An alternative ending in an escape escaped the comma or the `}` after
    // it.
    let escapes_syntax = ends_in_escape(head)
        || alternatives
            .iter()
            .any(|alternative| ends_in_escape(alternative) || trims_escape(alternative));
    if escapes_syntax {
        return Some(DefectKind::EscapedSyntax);
    }
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => return Some(DefectKind::NestedBraces),
            _ => {}
        }
    }
    None
}

/// `^([^{]*)\{([^}]+)\}(.*)$` as JavaScript reads it: the text before the
/// first `{`, a non-empty body up to the first `}` after it, and a rest that
/// holds no line terminator, which `.` does not match.
fn first_group(glob: &str) -> Option<(&str, &str, &str)> {
    let open = glob.find('{')?;
    let close = open + 1 + glob[open + 1..].find('}')?;
    let tail = &glob[close + 1..];
    (close > open + 1 && !tail.contains(LINE_TERMINATORS))
        .then(|| (&glob[..open], &glob[open + 1..close], tail))
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// Whether JavaScript's `trim` and `\s` read `c` as whitespace.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_len(text: &str) -> i64 {
    text.encode_utf16().count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(paths: &[&str]) -> yaml_serde::Value {
        yaml_serde::Value::Sequence(
            paths
                .iter()
                .map(|glob| yaml_serde::Value::String((*glob).into()))
                .collect(),
        )
    }

    fn globs(paths: &yaml_serde::Value) -> Vec<String> {
        expanded(paths)
            .into_iter()
            .flat_map(|piece| piece.globs)
            .collect()
    }

    fn scope(paths: &[&str], reading: Reading) -> PathScope {
        DeclaredPaths::read(&value(paths))
            .unwrap_or_else(|defects| panic!("{paths:?}: {defects:?}"))
            .scope(reading)
    }

    #[test]
    fn both_validators_accept_compound_scalar_patterns() {
        use crate::config::{RulesPolicy, SkillsPolicy};
        use crate::validate::{RuleValidator, SkillValidator};
        let rules: RulesPolicy = toml::from_str("").unwrap();
        let skills: SkillsPolicy = toml::from_str("").unwrap();
        for pattern in [
            "docs/[a-c]*.md",
            "**/*.ts, !**/*.test.ts",
            "src/*.{ts,tsx}",
            "{src,test}/**/*.rs",
            "photos \\[2024/**",
        ] {
            let content = format!(
                "---\nname: probe\ndescription: Read-only probe.\npaths: '{pattern}'\n---\n"
            );
            assert!(
                RuleValidator::new(&rules)
                    .validate_text(&content, Path::new(".claude/rules/probe.md"))
                    .is_empty(),
                "rule: {pattern}"
            );
            assert!(
                SkillValidator::new(&skills)
                    .validate_text(&content, Path::new(".claude/skills/probe/SKILL.md"))
                    .is_empty(),
                "skill: {pattern}"
            );
        }
    }

    #[test]
    fn expanded_is_what_claude_code_reads() {
        for (paths, expected) in [
            (
                value(&["src/**/*.{ts, tsx}, docs/[a,b]*.md"]),
                vec!["src/**/*.ts", "src/**/*.tsx", "docs/[a", "b]*.md"],
            ),
            (
                value(&[" docs/** ", "a.md,b.md"]),
                vec!["docs/**", "a.md", "b.md"],
            ),
            (value(&["{a,{b,c}}.md"]), vec!["a}.md", "b.md", "c}.md"]),
            (value(&["{}.md", "x{y"]), vec!["{}.md", "x{y"]),
            (value(&["x{y,z", "a},b"]), vec!["x{y,z", "a},b"]),
            (value(&["{a}.md"]), vec!["a.md"]),
            (value(&["a\\ "]), vec!["a\\"]),
            (
                yaml_serde::Value::String("data/\\,*.csv".into()),
                vec!["data/\\", "*.csv"],
            ),
        ] {
            assert_eq!(globs(&paths), expected, "{paths:?}");
        }
    }

    #[test]
    fn brace_budget_is_shared_and_leaves_a_glob_over_it_unexpanded() {
        let over_budget = |paths: &yaml_serde::Value| -> Vec<bool> {
            expanded(paths)
                .into_iter()
                .map(|piece| piece.defect == Some(DefectKind::BraceLimit))
                .collect()
        };
        let nine = "{a,b}".repeat(9);
        assert_eq!(globs(&value(&[&nine])).len(), 512);
        let ten = "{a,b}".repeat(10);
        assert_eq!(globs(&value(&[&ten])), [ten.as_str()]);
        assert_eq!(over_budget(&value(&[&ten])), [true]);
        let second = format!("{nine}y");
        let shared = globs(&value(&[&format!("{nine}x"), &second]));
        assert_eq!(shared.len(), 513);
        assert_eq!(shared.last().map(String::as_str), Some(second.as_str()));
        assert_eq!(over_budget(&value(&["{}.md"])), [false]);

        // Lengths in UTF-16 code units; `가` is one unit and three UTF-8 bytes.
        let piece = |units: usize, prefix: &str| {
            format!("{prefix}{}{{a,b}}", "가".repeat(units - 5 - prefix.len()))
        };
        assert_eq!(globs(&value(&[&piece(1_398_101, "")])).len(), 2);
        let over = piece(1_398_102, "");
        assert_eq!(globs(&value(&[&over])), [over.as_str()]);
        let second = piece(700_000, "x");
        assert_eq!(globs(&value(&[&second])).len(), 2);
        let shared = globs(&value(&[&piece(700_000, ""), &second]));
        assert_eq!(shared.len(), 3);
        assert_eq!(shared.last().map(String::as_str), Some(second.as_str()));

        // A count or length at a limit expands; one past it does not.
        let group = |alternatives: usize| {
            let numbers: Vec<String> = (0..alternatives).map(|n| n.to_string()).collect();
            format!("{{{}}}", numbers.join(","))
        };
        let padded = |alternatives: usize, units: usize| {
            let group = group(alternatives);
            format!("{group}{}", "x".repeat(units - group.len()))
        };
        assert_eq!(globs(&value(&[&group(1000)])).len(), 1000);
        assert_eq!(over_budget(&value(&[&group(1001)])), [true]);
        assert_eq!(globs(&value(&["{a,b}", &group(998)])).len(), 1000);
        assert_eq!(over_budget(&value(&["{a,b}", &group(999)])), [false, true]);
        assert_eq!(globs(&value(&[&padded(511, 8192)])).len(), 511);
        assert_eq!(over_budget(&value(&[&padded(511, 8193)])), [true]);
        assert_eq!(globs(&value(&["{a,b}", &padded(500, 8192)])).len(), 502);
    }

    #[test]
    fn globs_every_reader_reads_alike_are_declared() {
        for glob in [
            "*.md",
            "/a.md",
            "sub/",
            "/sub/",
            "**/a.md",
            "sub/**/a.md",
            "**",
            "**/**",
            "**/",
            "/**/",
            "**/**/",
            "/**/**/",
            "x/*/a.md",
            "x/**/**/a.md",
            "sub/**/**",
            "***",
            "***/",
            "/***",
            "a/***",
            "a/***/",
            "**/***",
            "sub**",
            "**sub",
            "a**b/x",
            "?.md",
            "x?/**/y",
            "a b.md",
            "a\u{a0}b.md",
            "a/b /**",
            "a/b/ /**",
            "/a/ /**",
            "[ab].md",
            "[ab].md,c.md",
            "[a-c].md",
            "[a-a].md",
            "[a-].md",
            "[-a].md",
            "[0-9a-zA-Z_.-].md",
            "[가-힣].md",
            "p[$.|+(){^]q",
            "p[ \t]q",
            "p[a:b:]q",
            "p[zab]q",
            "[.a]/x",
            "\\[id\\]/**",
            "app/\\[locale\\]/*.tsx",
            "lit/\\*.md",
            "\\!keep.md",
            "a\\!b",
            "\\#a.md",
            "a\\#b",
            "a\\.b",
            "a\\}b",
            "}\\{,b",
            "x{a\\,b",
            "a\\ b.md",
            "**/ { }",
            "a\rb",
            "a\u{2028}b",
            "src/**/*.rs\n",
            "*.md, !a.md",
            "*.md, !**",
            "a!.md",
            "a#.md",
            "{a,b}.md",
            "{a,}.md",
            "{a}.md",
            "{}.md",
            "{a}{}.md",
            "x{y",
            "a}b",
            "a{{b",
            "src/**/*.{ts,tsx}",
        ] {
            assert!(
                DeclaredPaths::read(&value(&[glob])).is_ok(),
                "{glob}: {:?}",
                DeclaredPaths::read(&value(&[glob])).err()
            );
        }
    }

    #[test]
    fn globs_the_readers_part_on_are_defects() {
        use BracketDefect as B;
        use DefectKind as D;
        for (entry, kind) in [
            ("docs/[a,b]*.md", D::CommaInBrackets),
            ("[]a,b].md", D::CommaInBrackets),
            ("a}{b,c}", D::StrayBrace),
            ("}{a,b}", D::StrayBrace),
            ("{a,b}}{c,d}", D::StrayBrace),
            ("data/\\,*.csv", D::EscapedSyntax),
            ("\\{a,b\\}", D::EscapedSyntax),
            ("{a\\},b}", D::EscapedSyntax),
            ("{a\\,b}", D::EscapedSyntax),
            ("}\\{\\,b", D::EscapedSyntax),
            ("\\}\\{\\,b", D::EscapedSyntax),
            ("{\\}}\\{,b", D::EscapedSyntax),
            ("a\\ ", D::EscapedSyntax),
            ("{a\\ ,b}.md", D::EscapedSyntax),
            ("{\\ ,b}\\[a]", D::EscapedSyntax),
            ("{a,{b,c}}.md", D::NestedBraces),
            ("{{name}}/**", D::NestedBraces),
            ("x{y,z", D::UnsplitComma),
            ("a},b", D::UnsplitComma),
            ("x\\{a,b", D::UnsplitComma),
            ("{}{a,b}.md", D::UnexpandedGroup),
            ("{}{a}.md", D::UnexpandedGroup),
            ("\\{}{a}", D::UnexpandedGroup),
            ("{a}\rb", D::UnexpandedGroup),
            ("a\nb", D::PatternTerminator),
            ("a\0b", D::PatternTerminator),
            ("#a.md", D::Comment),
            ("!", D::EmptyNegation),
            ("{ } { }", D::EmptyGlob),
            ("!\u{feff}a.md", D::ByteOrderMark),
            ("{ }\u{feff}x", D::ByteOrderMark),
            ("{ }\u{feff}#x", D::ByteOrderMark),
            ("a\\", D::DanglingEscape),
            ("a\\?.md", D::Escape('?')),
            ("a\\b.md", D::Escape('b')),
            ("src\\components\\*.tsx", D::Escape('c')),
            ("e/a\\/b.md", D::Escape('/')),
            ("a\\\\b.md", D::Escape('\\')),
            ("a\\\tb", D::Escape('\t')),
            ("lit/a\\*", D::EscapedTrailingStar),
            ("lit/a\\*/**", D::EscapedTrailingStar),
            ("a\u{a0}/**", D::TrailingWhitespace),
            ("a\r{ }", D::TrailingWhitespace),
            ("a/ /**", D::TrailingSpace),
            ("/", D::EmptySegment),
            ("sub//a.md", D::EmptySegment),
            ("./a.md", D::EmptySegment),
            ("../a.md", D::EmptySegment),
            ("[.]/x", D::EmptySegment),
            ("x/[.][.]/y", D::EmptySegment),
            ("***/a", D::StarRun),
            ("a/****/b", D::StarRun),
            ("**/**/a.md", D::LeadingGlobstars),
            ("sub/**/", D::GlobstarDirectory),
            ("a/**/**/", D::GlobstarDirectory),
            ("!/**", D::RootGlobstar),
            ("!/**/**", D::RootGlobstar),
            ("!/***", D::TrailingStarRun),
            ("!a/***/", D::TrailingStarRun),
            ("photos [2024/**", D::UnclosedBracket),
            ("{[}a,b]", D::UnclosedBracket),
            ("[a/b].md", D::SlashInBrackets),
            ("[!a].md", D::Bracket(B::Negated)),
            ("[^a].md", D::Bracket(B::Negated)),
            ("[]a].md", D::Bracket(B::ClosingFirst)),
            ("[*].md", D::Bracket(B::Wildcard)),
            ("p[a?]q", D::Bracket(B::Wildcard)),
            ("[a\\]].md", D::Bracket(B::Escape)),
            ("p[a\\-z]q", D::Bracket(B::Escape)),
            ("[[:alpha:]].md", D::Bracket(B::Opening)),
            ("[[].md", D::Bracket(B::Opening)),
            ("[b-a].md", D::Bracket(B::Range)),
            ("[A-z].md", D::Bracket(B::Range)),
            ("[(-+].md", D::Bracket(B::Range)),
            ("a[.-0]b", D::Bracket(B::Range)),
            (&"{a,b}".repeat(10), D::BraceLimit),
        ] {
            let kinds: Vec<DefectKind> = DeclaredPaths::read(&value(&["src/**", entry]))
                .err()
                .unwrap_or_default()
                .into_iter()
                .map(|defect| defect.kind)
                .collect();
            assert_eq!(kinds, [kind], "{entry}");
        }
    }

    #[test]
    fn globs_the_releases_part_on_once_negated_are_defects_only_then() {
        use DefectKind as D;
        for (paths, expected) in [
            (&["/**", "!d/"][..], &[D::RootGlobstar][..]),
            (&["/**/**", "!d/"], &[D::RootGlobstar]),
            (&["/***", "!d"], &[D::TrailingStarRun]),
            (&["a/***", "!*.md"], &[D::TrailingStarRun]),
            (&["a/***/", "!a/b/"], &[D::TrailingStarRun]),
            (&["*/***", "!*.md"], &[D::TrailingStarRun]),
            (&["**/a/***", "!q/a/b.md"], &[D::TrailingStarRun]),
            (&["**", "!a/***"], &[D::TrailingStarRun]),
            (
                &["/**", "a/***", "!x"],
                &[D::RootGlobstar, D::TrailingStarRun],
            ),
            (&["a", "!x", "/**"], &[]),
            (&["/**", "a/***"], &[]),
            (&["***", "!x"], &[]),
            (&["***/", "!x/"], &[]),
            (&["**/***", "!x"], &[]),
            (&["**", "!***"], &[]),
            (&["**", "/**/", "!d/"], &[]),
        ] {
            let kinds: Vec<DefectKind> = DeclaredPaths::read(&value(paths))
                .err()
                .unwrap_or_default()
                .into_iter()
                .map(|defect| defect.kind)
                .collect();
            assert_eq!(kinds, expected, "{paths:?}");
        }
    }

    #[test]
    fn a_negation_with_no_glob_before_it_is_a_defect() {
        for (paths, expected) in [
            (&["!a.md"][..], &["!a.md"][..]),
            (&["!a.md", "!b.md", "*.md"], &["!a.md", "!b.md"]),
            (&["!a.md, *.md, !b.md"], &["!a.md"]),
            (&["*.md", "!a.md"], &[]),
            (&["\\!a.md", "!b.md"], &[]),
        ] {
            let leading: Vec<String> = DeclaredPaths::read(&value(paths))
                .err()
                .unwrap_or_default()
                .into_iter()
                .filter(|defect| defect.kind == DefectKind::LeadingNegation)
                .map(|defect| defect.glob)
                .collect();
            assert_eq!(leading, expected, "{paths:?}");
        }
    }

    #[test]
    fn a_defect_names_what_the_matcher_is_given() {
        let defects = DeclaredPaths::read(&value(&["lit/a\\*/**", "#a.md"]))
            .err()
            .expect("both globs are defects");
        let messages: Vec<String> = defects.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "'lit/a\\*/**', which Claude Code's matcher is given as 'lit/a\\*' once the \
                 trailing `/**` is dropped, ends in an escaped `*`, which the releases of Claude \
                 Code's matcher read differently",
                "'#a.md' starts with `#`, which makes it a comment that matches nothing",
            ]
        );
        assert_eq!(defects[0].hint(), "match the trailing `*` with `?`");
    }

    #[test]
    fn escape_hint_names_the_separator_only_for_a_letter_or_digit() {
        let hint = |glob: &str| {
            DeclaredPaths::read(&value(&[glob]))
                .err()
                .expect("an unstable escape")[0]
                .hint()
        };
        assert_eq!(
            hint("src\\components\\*.tsx"),
            "remove the backslash; a glob separates directories with `/`"
        );
        assert_eq!(hint("a\\\tb"), "remove the backslash");
    }

    #[test]
    fn claude_code_reads_as_measured() {
        for (paths, target, expected) in [
            (&["*.yaml"][..], "sub/deep/a.yaml", true),
            (&["README.md"], "apps/README.md", true),
            (&["deep"], "sub/deep/a.md", true),
            (&["/*.toml"], "Cargo.toml", true),
            (&["/*.toml"], "sub/Cargo.toml", false),
            (&["sub/"], "a/sub/deep/b.md", true),
            (&["sub/"], "sub", false),
            (&["docs/GUIDE.md"], "docs/guide.md", true),
            (&["[A-Z].MD"], "a.md", true),
            (&["[a-c].md"], "B.md", true),
            (&["σ.md"], "ς.md", true),
            (&["[σ].md"], "Σ.md", true),
            (&["k.md"], "\u{212a}.md", false),
            (&["ß.md"], "\u{1e9e}.md", false),
            (&["I.md"], "ı.md", false),
            (&["[ı].md"], "I.md", false),
            (&["?.md"], "😀.md", false),
            (&["??.md"], "😀.md", true),
            (&["[가-힣].md"], "나.md", true),
            (&["docs/*.md"], "docs/sub/a.md", false),
            (&["docs/*.md"], "apps/docs/a.md", false),
            (&["src/**"], "apps/src/b.test.ts", true),
            (&["src/**"], "src", true),
            (&["src/"], "src", false),
            (&["docs/api/**"], "docs/api/a.md", true),
            (&["docs/api/**"], "x/docs/api/a.md", false),
            (&["a/b /**"], "a/b/c.md", true),
            (&["a/b /**"], "x/a/b/c.md", false),
            (&["*/**"], "a.md", true),
            (&["***"], "x/y.md", true),
            (&["a/***"], "a/b/c", true),
            (&["a/***"], "x/a/b", false),
            (&["a/***/"], "a/b/c", true),
            (&["a/***/"], "a/b.md", false),
            (&["a{{b"], "a{{b", true),
            (&["a\\!b"], "a!b", true),
            (&["\\.github/**"], ".github/ci.yml", true),
            (&["src/**/*.rs\n"], "src/a.rs", true),
            (&["a**b"], "x/axyb", true),
            (&["{README.md,docs/**}"], "pkg/docs/b.md", true),
            (&["{a}.md"], "a.md", true),
            (&["{}.md"], "{}.md", true),
            (&["x{y"], "x{y", true),
            (&["**/*.md"], ".hidden/a.md", true),
            (&["src/*.{ts,tsx}"], "src/sub/a.ts", false),
            (&["{src,test}/**/*.{ts,tsx}"], "test/a.ts", true),
            (&["photos \\[2024/**"], "photos [2024/a.jpg", true),
            (&["app/\\[locale\\]/**/*.tsx"], "app/[locale]/x.tsx", true),
            (&["app/[locale]/**"], "app/[locale]/x.tsx", false),
            (&["a?.md"], "ab.md", true),
            (&["**/*.ts", "!**/*.test.ts"], "apps/src/b.test.ts", false),
            (&["**/*.ts", "!**/*.test.ts"], "apps/src/b.ts", true),
            (&["lib/**", "!lib/**/*.test.ts"], "lib/a.test.ts", true),
            (&["*.md", "!a.md", "a.md"], "a.md", true),
            (&["*.md", "!**"], "a.md", false),
            (&["\\!keep.md"], "!keep.md", true),
            (&["\\#a.md"], "#a.md", true),
        ] {
            assert_eq!(
                scope(paths, Reading::CLAUDE_CODE).matches(target),
                expected,
                "{paths:?}: {target}"
            );
        }
    }

    #[test]
    fn as_written_keeps_the_trailing_globstar_and_case() {
        for (paths, target, expected) in [
            (&["src/**"][..], "src/a.ts", true),
            (&["src/**"], "apps/src/a.ts", false),
            (&["src/**"], "src", false),
            (&["*/**"], "a.md", false),
            (&["/**"], "a/b.md", true),
            (&["*.md"], "x/y/a.md", true),
            (&["docs/GUIDE.md"], "docs/guide.md", false),
            (&["σ.md"], "ς.md", false),
            (&["lib/**", "!lib/**/*.test.ts"], "lib/a.test.ts", false),
            (&["lib/**", "!lib/**/*.test.ts"], "lib/s/a.test.ts", true),
        ] {
            assert_eq!(
                scope(paths, Reading::AS_WRITTEN).matches(target),
                expected,
                "{paths:?}: {target}"
            );
        }
    }

    #[test]
    fn scope_is_declared_by_what_the_globs_leave() {
        for (paths, scoped) in [
            (value(&[]), false),
            (value(&["**", "**/**", " "]), false),
            (value(&["{**,**/**}"]), false),
            (value(&["**", "src/**"]), true),
            (value(&["***"]), true),
            (value(&["#a.md"]), true),
        ] {
            assert_eq!(declares_scope(Some(&paths)), scoped, "{paths:?}");
        }
        assert!(!declares_scope(None));
        assert!(!declares_scope(Some(&yaml_serde::Value::Null)));
    }

    fn audit(files: &[&str], paths: &str) -> Option<Finding> {
        let dropping = DroppingPaths::read(
            &format!("---\npaths: {paths}\n---\nBody\n"),
            Path::new(".claude/rules/probe.md"),
        )?;
        PathsReadingAuditor::new(files.iter().map(|file| (*file).to_string()).collect())
            .audit(&dropping)
    }

    #[test]
    fn auditor_names_the_files_and_the_spelling_both_readings_share() {
        let files = [
            "docs/a.md",
            "apps/web/docs/b.md",
            "apps/web/docs/c.md",
            "src/a.ts",
        ];
        let finding = audit(&files, "\"docs/**\"").expect("nested docs part the readings");
        assert_eq!(finding.slug, "paths-reading-differs");
        assert_eq!(finding.severity, Severity::Minor);
        assert_eq!(
            finding.message,
            "Claude Code drops a trailing `/**` from `paths:`, so it applies to 2 files the globs \
             as written leave out, such as `apps/web/docs/b.md`"
        );
        assert_eq!(
            finding.hint.as_deref(),
            Some(
                "A glob with its `/**` dropped matches the directory itself, at every depth \
                 unless another `/` anchors it, and no `!` glob re-includes a file under it: \
                 write `docs/**/*` for what `docs/**` means as written, or `**/docs/**/*` for \
                 every depth"
            )
        );
        for portable in [
            "\"docs/**/*\"",
            "\"**/docs/**/*\"",
            "\"/docs/**\"",
            "\"src/*.ts\"",
        ] {
            assert!(audit(&files, portable).is_none(), "{portable}");
        }
    }

    #[test]
    fn auditor_reports_a_negation_the_dropped_globstar_disarms() {
        let finding = audit(
            &["lib/a.test.ts", "lib/a.ts", "src/b.ts"],
            "[\"lib/**\", \"src/**\", \"!lib/**/*.test.ts\"]",
        )
        .expect("a direct child stays covered");
        assert!(
            finding.message.contains(
                "applies to 1 file the globs as written leave out, such as `lib/a.test.ts`"
            ),
            "{}",
            finding.message
        );
        let hint = finding.hint.unwrap();
        assert!(
            hint.ends_with(
                ": write `lib/**/*` for what `lib/**` means as written, or `**/lib/**/*` for \
                 every depth"
            ),
            "{hint}"
        );
    }

    #[test]
    fn applying_every_rewrite_the_hint_names_clears_the_finding() {
        let files = ["a/b/f.md", "b/g.md", "x/a/y.md"];
        let finding = audit(&files, "[\"b\", \"!b/**\", \"!/a/**\"]")
            .expect("dropping `!b/**` re-includes `b`");
        let hint = finding.hint.unwrap();
        assert!(
            hint.ends_with(
                ": write `!b/**/*` for what `!b/**` means as written, or `!**/b/**/*` for every \
                 depth; write `!/a/**/*` for what `!/a/**` means as written"
            ),
            "{hint}"
        );
        assert!(audit(&files, "[\"b\", \"!b/**/*\", \"!/a/**/*\"]").is_none());
    }

    #[test]
    fn auditor_names_each_rewrite_once_and_no_alternative_covering_the_same() {
        let finding = audit(&["a/c.md"], "[\"*/**\", \"*/**\", \"!a/c.md\"]")
            .expect("the dropped glob covers `a` itself");
        let hint = finding.hint.unwrap();
        assert!(
            hint.ends_with(": write `*/**/*` for what `*/**` means as written"),
            "{hint}"
        );
    }

    #[test]
    fn auditor_reads_a_file_only_the_glob_as_written_reaches() {
        let finding = audit(&["a /x.md"], "\"a /**\"").expect("the matcher trims `a `");
        assert_eq!(
            finding.message,
            "Claude Code drops a trailing `/**` from `paths:`, so it does not apply to 1 file the \
             globs as written cover, such as `a /x.md`"
        );
    }

    #[test]
    fn auditor_compares_no_file_holding_a_line_terminator() {
        assert!(audit(&["apps/docs/a.md"], "\"docs/**\"").is_some());
        assert!(audit(&["apps/docs/a\r.md"], "\"docs/**\"").is_none());
    }

    #[test]
    fn auditor_folds_case_on_both_sides_as_claude_code_does() {
        assert!(audit(&["Docs/a.md"], "\"docs/*.md\"").is_none());
        assert!(audit(&["src/a.ts", "SRC/b.ts"], "\"src/**\"").is_none());
        let finding = audit(&["docs/a.md", "apps/Docs/b.md"], "\"docs/**\"")
            .expect("the dropped `/**` reaches a nested `Docs`");
        assert!(
            finding.message.ends_with("`apps/Docs/b.md`"),
            "{}",
            finding.message
        );
        let finding = audit(&["SRC/x.md", "SRC/y.md"], "[\"src/**\", \"!**/x.md\"]")
            .expect("the dropped `/**` disarms the `!` glob on `SRC/x.md`");
        assert!(
            finding.message.ends_with("`SRC/x.md`"),
            "{}",
            finding.message
        );
    }

    #[test]
    fn auditor_leaves_what_it_cannot_read_to_the_validators() {
        let files = ["apps/docs/a.md"];
        for paths in [
            "\"**\"",
            "\"/**\"",
            "\"[!a]/**\"",
            "3",
            "[\"docs/**\", 3]",
            "\"docs/**\"\n bad: [",
        ] {
            assert!(audit(&files, paths).is_none(), "{paths}");
        }
    }
}
