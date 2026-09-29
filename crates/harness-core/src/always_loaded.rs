//! # always_loaded — the text a repository puts into every Claude Code session
//!
//! Part of every session's context is assembled from files a repository
//! commits: its memory files and what they import, the rules that carry no
//! scope, the output style its settings select, and one listing entry per
//! skill, command and agent. [`resolve`] reads that set the way the runtime
//! does and counts what each member contributes, so a budget can be held over
//! the whole set rather than over one rule file at a time ([`over_budget`]).
//!
//! The reading is the runtime's, measured from the request it sends and the
//! loader it ships, on Claude Code 2.1.283 unless a bullet names 2.1.284:
//!
//! - A session reads its own directory and each one above it up to the
//!   repository's top, every kind alike, with its own directory's settings
//!   alone ([`levels`]). A skill, command, agent or output style defined at
//!   two levels is the nearest one's, and a skill's name hides a command's
//!   wherever either is defined (2.1.284).
//! - `CLAUDE.md` and `.claude/CLAUDE.md` both load; `AGENTS.md` and
//!   `.claude/AGENTS.md` load in their place when neither exists
//!   ([`memory_files`]).
//! - A file loads only as a regular file within its kind's size limit
//!   ([`MEMORY_FILE_LIMIT`], [`SKILL_FILE_LIMIT`], [`DEFINITION_FILE_LIMIT`]),
//!   decoded as UTF-8 with invalid bytes replaced.
//! - Frontmatter opens with `---` and closes at the next `---`, wherever that
//!   falls; without one the whole file is body. YAML that does not parse is
//!   parsed again after the runtime's repair — a `key: value` line whose
//!   unquoted value holds a YAML indicator is quoted, leading tabs become
//!   spaces — and yields no keys when that fails too.
//! - A rule under `.claude/rules/`, and each file its imports reach, loads when
//!   its own `paths:` scopes nothing (`validate::path_globs::declares_scope`);
//!   a file `CLAUDE.md` imports loads whatever its `paths:` says. A skill
//!   whose `paths:` scopes something waits for a matching file and is not
//!   listed.
//! - A memory file loses each top-level HTML block that opens with a comment,
//!   and a file holding a comment has its line breaks read as `\n`. An inline
//!   comment, and one inside a list or a quote, stays.
//! - `@path` imports a file when it opens a text run or follows whitespace
//!   outside code and comments, begins with a letter, a digit, `.`, `_`, `-`,
//!   `~/` or `/`, and names a file whose extension is in
//!   [`IMPORT_TEXT_EXTENSIONS`] or that has none. It resolves against the
//!   directory the importing file's content lives in, a link's target
//!   included, reaches [`MAX_IMPORT_HOPS`] deep, and loads a file once.
//! - `claudeMdExcludes` removes a memory file whose absolute path it matches —
//!   a rule's path under `.claude/rules/` or its link target — and a relative
//!   pattern matches nothing. The patterns are picomatch's (2.1.284); one
//!   outside the dialect [`ExcludeGlob`] reads is named in `unread_excludes`
//!   and excludes nothing here.
//! - The output style is the body of the file whose `name`, or else, for a
//!   file declaring none, whose file name is `outputStyle`, comments included.
//! - A skill or command lists `description` — else its body's first non-empty
//!   line, a heading's text, cut to [`FALLBACK_DESCRIPTION_CAP`] — and
//!   ` - when_to_use`, cut to [`LISTING_ENTRY_CAP`], unless
//!   `disable-model-invocation` is set or the committed `skillOverrides`
//!   offer it `off`, `user-invocable-only` or `name-only`. An agent with a
//!   usable `name` lists its `description`. A file reached twice is listed
//!   once, and an agent name registers once.
//!
//! A member counts its own text. The framing the runtime writes around it —
//! a file header, a list marker, an agent's tools — is not the repository's
//! to cut. When the whole skill listing outgrows the runtime's budget, which
//! scales with the model's context window, some entries are sent as a name
//! alone, so a listing member counts what the repository asks for.
//!
//! ## What this module refuses to do
//!
//! - Never counts what the repository does not own. `CLAUDE.local.md`, a
//!   user-level memory file, a memory file or rule above the repository, auto
//!   memory and `settings.local.json` are each developer's own, and reading
//!   the last would pass a tree locally that CI fails. Text whose file lies
//!   outside the repository — an import reaching out, a link pointing out —
//!   and an output style it does not ship are [`Unmeasured`], named rather
//!   than guessed.
//! - Never runs a hook. A `SessionStart` hook's output joins every session
//!   too, and its size is the script's to bound.
//! - Never counts tokens. That needs the model's tokenizer, which is not
//!   available offline, and characters are exact.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use serde::Serialize;
use yaml_serde::{Mapping, Value};

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::{
    AgentValidator, RuleValidator, SkillValidator, SurfaceValidator, path_globs,
};
use crate::wire_enum::wire_enum;
/// Extensions an import may carry, lowercased; an import with none loads too.
/// Read from the include filter the 2.1.283 CLI ships, since the memory page
/// names none.
pub const IMPORT_TEXT_EXTENSIONS: &[&str] = &[
    ".md",
    ".txt",
    ".text",
    ".json",
    ".yaml",
    ".yml",
    ".toml",
    ".xml",
    ".csv",
    ".html",
    ".htm",
    ".css",
    ".scss",
    ".sass",
    ".less",
    ".js",
    ".ts",
    ".tsx",
    ".jsx",
    ".mjs",
    ".cjs",
    ".mts",
    ".cts",
    ".py",
    ".pyi",
    ".pyw",
    ".rb",
    ".erb",
    ".rake",
    ".go",
    ".rs",
    ".java",
    ".kt",
    ".kts",
    ".scala",
    ".c",
    ".cpp",
    ".cc",
    ".cxx",
    ".h",
    ".hpp",
    ".hxx",
    ".cs",
    ".swift",
    ".sh",
    ".bash",
    ".zsh",
    ".fish",
    ".ps1",
    ".bat",
    ".cmd",
    ".env",
    ".ini",
    ".cfg",
    ".conf",
    ".config",
    ".properties",
    ".sql",
    ".graphql",
    ".gql",
    ".proto",
    ".vue",
    ".svelte",
    ".astro",
    ".ejs",
    ".hbs",
    ".pug",
    ".jade",
    ".php",
    ".pl",
    ".pm",
    ".lua",
    ".r",
    ".R",
    ".dart",
    ".ex",
    ".exs",
    ".erl",
    ".hrl",
    ".clj",
    ".cljs",
    ".cljc",
    ".edn",
    ".hs",
    ".lhs",
    ".elm",
    ".ml",
    ".mli",
    ".f",
    ".f90",
    ".f95",
    ".for",
    ".cmake",
    ".make",
    ".makefile",
    ".gradle",
    ".sbt",
    ".rst",
    ".adoc",
    ".asciidoc",
    ".org",
    ".tex",
    ".latex",
    ".lock",
    ".log",
    ".diff",
    ".patch",
];

/// The closed sets this module mirrors from Claude Code, stamped by `spec`.
pub const SPEC_SETS: &[(&str, &[&str])] = &[("import-text-extensions", IMPORT_TEXT_EXTENSIONS)];

/// How many imports deep a chain loads, counted from the memory file or rule
/// that starts it.
pub const MAX_IMPORT_HOPS: usize = 4;

/// Length a listing entry's text is cut to, the last character an ellipsis,
/// counted as [`runtime_len`] counts — unless the committed settings set
/// `skillListingMaxDescChars`.
pub const LISTING_ENTRY_CAP: usize = 1536;

/// Length a description taken from a body line is cut to, counted as
/// [`runtime_len`] counts.
pub const FALLBACK_DESCRIPTION_CAP: usize = 100;

/// Bytes past which the runtime skips a memory file whole — `CLAUDE.md`, a
/// rule, an import.
pub const MEMORY_FILE_LIMIT: u64 = 4_194_304;

/// Bytes past which the runtime skips a skill's `SKILL.md`.
pub const SKILL_FILE_LIMIT: u64 = 1_000_000;

/// Bytes past which the runtime skips a command, an agent or an output style.
pub const DEFINITION_FILE_LIMIT: u64 = 1_048_576;

/// Commands and output styles are read from every directory below their root.
const COMMAND_GLOB: &str = ".claude/commands/**/*.md";
const OUTPUT_STYLE_GLOB: &str = ".claude/output-styles/**/*.md";

static FRONTMATTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A---\s*\n((?s:.*?))---\s*\n?").expect("FRONTMATTER regex"));

/// A top-level `key: value` line, as the runtime's YAML repair matches one.
static PLAIN_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([a-zA-Z_-]+):\s+([^\n\r\u{2028}\u{2029}]+)$").expect("PLAIN_PAIR regex")
});

/// A value the runtime's YAML repair quotes.
static INDICATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[{}\[\]*&#!|>%@`]|: ").expect("INDICATOR regex"));

wire_enum! {
    /// Why a member is in every session.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum MemberKind {
        Memory => "memory",
        Rule => "rule",
        Import => "import",
        OutputStyle => "output-style",
        Skill => "skill",
        Command => "command",
        Agent => "agent",
    }
}

wire_enum! {
    /// Why a member that loads is not counted.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum UnmeasuredReason {
        /// Text whose file lies outside the project's repository: an import
        /// reaching out, `~/` included, or a link pointing out.
        OutsideProject => "outside-project",
        /// An output style no level of the repository ships: built in, or a
        /// user's.
        NotInProject => "not-in-project",
    }
}

/// One file's contribution to every session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Member {
    pub kind: MemberKind,
    /// Project-relative path of the file the text comes from.
    pub path: String,
    pub chars: usize,
}

/// A member that loads and is not counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Unmeasured {
    pub kind: MemberKind,
    /// The import as written, the style's name, or the file's project path.
    pub name: String,
    pub reason: UnmeasuredReason,
}

/// Everything the repository puts into every session, largest first.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct AlwaysLoaded {
    pub total_chars: usize,
    pub members: Vec<Member>,
    pub unmeasured: Vec<Unmeasured>,
    /// `claudeMdExcludes` patterns harnex does not read as the runtime does:
    /// a memory file one of them excludes is counted all the same.
    pub unread_excludes: Vec<String>,
}

/// Read the set a session started in `project`, the directory `harness.toml`
/// lives in, loads from the repository it lies in.
pub fn resolve(project: &Path) -> Result<AlwaysLoaded> {
    let project = canonical_root(project)?;
    let levels = levels(&project);
    let settings = ProjectSettings::read(&project)?;
    let mut walk = Walk {
        top: levels.last().expect("a project is its own first level"),
        project: &project,
        excludes: &settings.excludes,
        listing_cap: settings.listing_cap,
        overrides: &settings.skill_overrides,
        seen: HashSet::new(),
        listed: HashSet::new(),
        names: HashSet::new(),
        agents: HashSet::new(),
        members: Vec::new(),
        unmeasured: Vec::new(),
    };

    for level in &levels {
        for path in memory_files(level) {
            walk.memory_file(&path, MemberKind::Memory, 0, Tree::Memory);
        }
        for path in discover(level, <RuleValidator as SurfaceValidator>::GLOB)? {
            walk.memory_file(&path, MemberKind::Rule, 0, Tree::Rules);
        }
    }
    if let Some(name) = &settings.output_style {
        walk.output_style(name, &levels)?;
    }
    // Every level's skills before any command: a skill's name hides a
    // command's wherever either is defined.
    for level in &levels {
        for path in discover(level, <SkillValidator as SurfaceValidator>::GLOB)? {
            let name = path
                .parent()
                .and_then(Path::file_name)
                .map(|dir| dir.to_string_lossy().into_owned());
            walk.listing(&path, MemberKind::Skill, name);
        }
    }
    for level in &levels {
        for (path, name) in commands(level)? {
            walk.listing(&path, MemberKind::Command, Some(name));
        }
    }
    for level in &levels {
        for path in discover(level, <AgentValidator as SurfaceValidator>::GLOB)? {
            walk.listing(&path, MemberKind::Agent, None);
        }
    }

    let Walk {
        mut members,
        mut unmeasured,
        ..
    } = walk;
    members.sort_by(|a, b| b.chars.cmp(&a.chars).then_with(|| a.path.cmp(&b.path)));
    unmeasured.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    Ok(AlwaysLoaded {
        total_chars: members.iter().map(|m| m.chars).sum(),
        members,
        unmeasured,
        unread_excludes: settings.excludes.unread.clone(),
    })
}

/// `project` and each directory above it up to the top of the repository it
/// lies in, nearest first: where a session started in `project` reads its
/// harness from. The top is the nearest directory holding `.git` — the
/// directory, or the file a linked worktree or a submodule keeps — and outside
/// any repository `project` stands alone. A `.git` git itself would reject
/// still ends the walk, where git's own search goes on upward.
fn levels(project: &Path) -> Vec<&Path> {
    match project.ancestors().find(|dir| dir.join(".git").exists()) {
        Some(top) => project
            .ancestors()
            .take_while(|dir| dir.starts_with(top))
            .collect(),
        None => vec![project],
    }
}

/// The project memory files the runtime reads at launch: `CLAUDE.md` and
/// `.claude/CLAUDE.md`, or `AGENTS.md` and `.claude/AGENTS.md` where neither
/// exists.
pub(crate) fn memory_files(root: &Path) -> Vec<PathBuf> {
    let claude_md = present(root, &["CLAUDE.md", ".claude/CLAUDE.md"]);
    if claude_md.is_empty() {
        present(root, &["AGENTS.md", ".claude/AGENTS.md"])
    } else {
        claude_md
    }
}

/// The finding a set over `max_chars` is, located at its largest member.
pub fn over_budget(loaded: &AlwaysLoaded, max_chars: usize, root: &Path) -> Option<Finding> {
    if loaded.total_chars <= max_chars {
        return None;
    }
    let first = loaded.members.first()?;
    let largest: Vec<String> = loaded
        .members
        .iter()
        .take(5)
        .map(|m| format!("{} {}", m.path, m.chars))
        .collect();
    Some(Finding {
        slug: "always-loaded-over-budget".into(),
        severity: Severity::Major,
        location: Location::file(root.join(&first.path)),
        message: format!(
            "{} characters load into every session, over max_chars={max_chars}; largest of \
             {}: {}",
            loaded.total_chars,
            loaded.members.len(),
            largest.join(", ")
        ),
        hint: Some(
            "scope detail to the files it governs with `paths:`, or move it to a file read \
             on demand — every character here is paid on every turn. `harnex validate \
             always-loaded` lists each member."
                .into(),
        ),
        auto_fixable: false,
        fix_command: None,
    })
}

/// `claudeMdExcludes` as the runtime reads it: patterns matched against the
/// absolute path of a memory file, with the project root as the runtime sees
/// it — resolved through links — so a relative pattern matches nothing.
pub(crate) struct Excludes {
    root: PathBuf,
    canonical_root: PathBuf,
    patterns: Vec<ExcludeGlob>,
    /// Patterns written outside the dialect [`ExcludeGlob`] reads: each
    /// excludes nothing here, whatever it excludes in a session.
    unread: Vec<String>,
}

impl Excludes {
    /// The patterns the committed project settings declare.
    pub(crate) fn read(root: &Path) -> Result<Self> {
        Ok(ProjectSettings::read(root)?.excludes)
    }

    /// Whether the runtime skips the memory file at `path`, reached under the
    /// root this was read for. A rule is matched at its path under
    /// `.claude/rules/` and at its link target; any other memory file at the
    /// path it was reached by.
    pub(crate) fn matches(&self, path: &Path, kind: MemberKind) -> bool {
        let reached = match path.strip_prefix(&self.root) {
            Ok(relative) => self.canonical_root.join(relative),
            Err(_) => path.to_path_buf(),
        };
        let target = match kind {
            MemberKind::Rule => std::fs::canonicalize(path).ok(),
            _ => None,
        };
        std::iter::once(reached).chain(target).any(|candidate| {
            let candidate = candidate.to_string_lossy().replace('\\', "/");
            self.patterns.iter().any(|glob| glob.matches(&candidate))
        })
    }
}

/// One `claudeMdExcludes` pattern as the runtime reads it — picomatch with
/// `dot` (2.1.284) — which skips a file whose absolute path equals the
/// pattern or matches its glob.
///
/// harnex reads the patterns written in literal text, `*`, `**`, and `{…}`
/// groups of two or more non-empty alternatives holding no `**`, `..` or
/// group of their own, on which globset agrees with picomatch over the
/// canonical absolute paths a memory file is reached by
/// (`harnex_reads_its_dialect_as_picomatch_does`). A pattern holding anything
/// else — `?`, a bracket, a parenthesis, a leading `!`, another brace — is
/// left unread rather than applied.
struct ExcludeGlob {
    pattern: String,
    glob: globset::GlobMatcher,
}

impl ExcludeGlob {
    /// `None` for a pattern outside the dialect.
    fn read(pattern: &str) -> Option<Self> {
        if pattern.starts_with('!') {
            return None;
        }
        let mut group: Option<String> = None;
        for c in pattern.chars() {
            match (c, group.as_mut()) {
                ('?' | '[' | ']' | '(' | ')', _) | ('{', Some(_)) | ('}', None) => return None,
                ('{', None) => group = Some(String::new()),
                ('}', Some(alternatives)) => {
                    let split: Vec<&str> = alternatives.split(',').collect();
                    if split.len() < 2
                        || split.iter().any(|a| a.is_empty() || a.contains("**"))
                        || alternatives.contains("..")
                    {
                        return None;
                    }
                    group = None;
                }
                (c, Some(alternatives)) => alternatives.push(c),
                (_, None) => {}
            }
        }
        if group.is_some() {
            return None;
        }
        let glob = path_globs::compile_glob(pattern).ok()?.compile_matcher();
        Some(Self {
            pattern: pattern.to_string(),
            glob,
        })
    }

    fn matches(&self, path: &str) -> bool {
        path == self.pattern || self.glob.is_match(path)
    }
}

/// The `claudeMdExcludes` patterns `settings` declares, as the runtime
/// receives them: backslashes turned to slashes, empty ones dropped.
fn exclude_patterns(settings: &serde_json::Value) -> Vec<String> {
    settings
        .get("claudeMdExcludes")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .map(|pattern| pattern.replace('\\', "/"))
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

/// The `claudeMdExcludes` patterns `settings` declares that harnex leaves
/// unread ([`ExcludeGlob`]).
pub(crate) fn unread_excludes(settings: &serde_json::Value) -> Vec<String> {
    exclude_patterns(settings)
        .into_iter()
        .filter(|pattern| ExcludeGlob::read(pattern).is_none())
        .collect()
}

/// What the committed project settings select for every session.
struct ProjectSettings {
    output_style: Option<String>,
    listing_cap: usize,
    /// `skillOverrides`: a skill's or command's name to how it is offered.
    skill_overrides: serde_json::Map<String, serde_json::Value>,
    excludes: Excludes,
}

impl ProjectSettings {
    fn read(root: &Path) -> Result<Self> {
        // Settings that are absent or not JSON select nothing; the malformed
        // file is `validate.settings`' finding.
        let value = std::fs::read_to_string(root.join(".claude/settings.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let output_style = value
            .as_ref()
            .and_then(|v| v.get("outputStyle"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut patterns = Vec::new();
        let mut unread = Vec::new();
        for pattern in value.as_ref().map(exclude_patterns).unwrap_or_default() {
            for variant in with_resolved_prefix(&pattern) {
                match ExcludeGlob::read(&variant) {
                    Some(glob) => patterns.push(glob),
                    None => unread.push(variant),
                }
            }
        }
        let listing_cap = value
            .as_ref()
            .and_then(|v| v.get("skillListingMaxDescChars"))
            .and_then(|v| v.as_u64())
            .filter(|&cap| cap > 0)
            .map_or(LISTING_ENTRY_CAP, |cap| cap as usize);
        let skill_overrides = value
            .as_ref()
            .and_then(|v| v.get("skillOverrides"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            output_style,
            listing_cap,
            skill_overrides,
            excludes: Excludes {
                root: root.to_path_buf(),
                canonical_root: canonical_root(root)?,
                patterns,
                unread,
            },
        })
    }
}

/// `pattern`, and for an absolute one whose literal directory reaches through
/// a link, the same pattern over that link's target — the runtime matches
/// both.
fn with_resolved_prefix(pattern: &str) -> Vec<String> {
    let mut variants = vec![pattern.to_string()];
    if pattern.starts_with('/') {
        let literal = &pattern[..pattern.find(['*', '?', '{', '[']).unwrap_or(pattern.len())];
        if let Some(dir) = Path::new(literal).parent()
            && let Ok(resolved) = std::fs::canonicalize(dir)
            && resolved != dir
        {
            let rest = &pattern[dir.as_os_str().len()..];
            variants.push(format!("{}{rest}", resolved.display()));
        }
    }
    variants
}

fn canonical_root(root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(root).map_err(|source| Error::IoFailure {
        path: root.to_path_buf(),
        source,
    })
}

/// The walk a memory file was reached in. A rule's own `paths:` decides
/// whether it loads, and so does that of every file its imports reach; a file
/// reached from `CLAUDE.md` loads whatever its `paths:` says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tree {
    Memory,
    Rules,
}

struct Walk<'a> {
    /// The repository's top, canonical, so a member's content is the
    /// repository's exactly when its own canonical path starts with this.
    top: &'a Path,
    /// Canonical; a member's path is written relative to it.
    project: &'a Path,
    excludes: &'a Excludes,
    listing_cap: usize,
    overrides: &'a serde_json::Map<String, serde_json::Value>,
    /// Canonical paths already read: a file reached twice loads once.
    seen: HashSet<PathBuf>,
    /// Canonical paths of skills and commands already listed, the names
    /// skills and commands already hold, and agent names already registered:
    /// the runtime lists each once, from the nearest level that defines it.
    listed: HashSet<PathBuf>,
    names: HashSet<String>,
    agents: HashSet<String>,
    members: Vec<Member>,
    unmeasured: Vec<Unmeasured>,
}

impl Walk<'_> {
    /// `path` relative to the project, climbing out of it for a level above.
    fn relative(&self, path: &Path) -> String {
        let below = path
            .strip_prefix(self.top)
            .expect("every path the walk reads was found or kept under the top");
        let project = self
            .project
            .strip_prefix(self.top)
            .expect("the project is one of the levels below the top");
        let shared = below
            .components()
            .zip(project.components())
            .take_while(|(a, b)| a == b)
            .count();
        let mut relative = PathBuf::new();
        for _ in project.components().skip(shared) {
            relative.push(Component::ParentDir);
        }
        for part in below.components().skip(shared) {
            relative.push(part);
        }
        relative.to_string_lossy().into_owned()
    }

    fn unmeasured(&mut self, kind: MemberKind, name: String, reason: UnmeasuredReason) {
        self.unmeasured.push(Unmeasured { kind, name, reason });
    }

    /// Count `text` for the file reached at `path` whose content is at
    /// `target`, or name it where that content lies outside the project.
    fn admit(&mut self, kind: MemberKind, path: &Path, target: &Path, text: &str) -> bool {
        let name = self.relative(path);
        if !target.starts_with(self.top) {
            self.unmeasured(kind, name, UnmeasuredReason::OutsideProject);
            return false;
        }
        self.members.push(Member {
            kind,
            path: name,
            chars: text.chars().count(),
        });
        true
    }

    /// A memory file and what it imports. Its imports resolve against the
    /// directory its content lives in, and are followed whether or not the
    /// file itself loads.
    fn memory_file(&mut self, path: &Path, kind: MemberKind, depth: usize, tree: Tree) {
        if self.excludes.matches(path, kind) {
            return;
        }
        let Some((target, source)) = read(path, MEMORY_FILE_LIMIT) else {
            return;
        };
        if !self.seen.insert(target.clone()) {
            return;
        }
        let read = read_memory(&source.body);
        let text = read.text.trim();
        if text.is_empty() {
            return;
        }
        let loads = tree == Tree::Memory || !path_globs::declares_scope(source.fields.get("paths"));
        if loads {
            self.admit(kind, path, &target, text);
        }
        if !target.starts_with(self.top) || depth == MAX_IMPORT_HOPS {
            return;
        }
        let from = target.parent().unwrap_or(self.top).to_path_buf();
        for written in read.imports {
            self.import(&from, &written, depth + 1, tree);
        }
    }

    fn import(&mut self, from: &Path, written: &str, depth: usize, tree: Tree) {
        let Some(target) = import_target(written) else {
            return;
        };
        if !has_text_extension(Path::new(&target)) {
            return;
        }
        let path = if target.starts_with("~/") {
            None
        } else {
            let Some(path) = normalize(&from.join(&target)) else {
                return;
            };
            Some(path)
        };
        match path {
            Some(path) if path.starts_with(self.top) => {
                self.memory_file(&path, MemberKind::Import, depth, tree);
            }
            Some(path) if self.excludes.matches(&path, MemberKind::Import) => {}
            _ => self.unmeasured(
                MemberKind::Import,
                written.to_string(),
                UnmeasuredReason::OutsideProject,
            ),
        }
    }

    /// The style `selected` names, from the nearest level that defines it.
    fn output_style(&mut self, selected: &str, levels: &[&Path]) -> Result<()> {
        for level in levels {
            if self.output_style_in(level, selected)? {
                return Ok(());
            }
        }
        if !selected.eq_ignore_ascii_case("default") {
            self.unmeasured(
                MemberKind::OutputStyle,
                selected.to_string(),
                UnmeasuredReason::NotInProject,
            );
        }
        Ok(())
    }

    /// Admit the style `level` defines under `selected`: the file whose
    /// `name` it is, else one declaring no name whose file name it is.
    fn output_style_in(&mut self, level: &Path, selected: &str) -> Result<bool> {
        let mut by_stem = None;
        for path in discover(level, OUTPUT_STYLE_GLOB)? {
            let Some((target, source)) = read(&path, DEFINITION_FILE_LIMIT) else {
                continue;
            };
            match source.fields.get("name").and_then(js_string) {
                Some(name) if name == selected => {
                    self.admit(MemberKind::OutputStyle, &path, &target, source.body.trim());
                    return Ok(true);
                }
                None if by_stem.is_none() && path.file_stem().is_some_and(|s| s == selected) => {
                    by_stem = Some((path, target, source.body));
                }
                _ => {}
            }
        }
        Ok(match by_stem {
            Some((path, target, body)) => {
                self.admit(MemberKind::OutputStyle, &path, &target, body.trim());
                true
            }
            None => false,
        })
    }

    /// One listing entry: a skill or command offered under `name`, or an
    /// agent, which names itself.
    fn listing(&mut self, path: &Path, kind: MemberKind, name: Option<String>) {
        let limit = match kind {
            MemberKind::Skill => SKILL_FILE_LIMIT,
            _ => DEFINITION_FILE_LIMIT,
        };
        let Some((target, source)) = read(path, limit) else {
            return;
        };
        let entry = match kind {
            MemberKind::Agent => agent_entry(&source.fields)
                .filter(|(name, _)| self.agents.insert(name.clone()))
                .map(|(_, entry)| entry),
            _ => {
                if let Some(name) = &name
                    && !self.names.insert(name.clone())
                {
                    return;
                }
                let offered = name
                    .and_then(|name| self.overrides.get(&name))
                    .and_then(|mode| mode.as_str());
                let described =
                    !matches!(offered, Some("off" | "user-invocable-only" | "name-only"));
                (described && self.listed.insert(target.clone()))
                    .then(|| invocable_entry(&source, kind, self.listing_cap))
                    .flatten()
            }
        };
        if let Some(entry) = entry {
            self.admit(kind, path, &target, &entry);
        }
    }
}

/// A file's frontmatter keys and body, split as the runtime splits them.
struct Source {
    fields: Mapping,
    body: String,
}

impl Source {
    /// A leading byte-order mark is dropped: every reader of the text trims
    /// it away.
    fn parse(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        match FRONTMATTER.captures(text) {
            None => Self {
                fields: Mapping::new(),
                body: text.to_string(),
            },
            Some(block) => Self {
                fields: fields(&block[1]),
                body: text[block.get(0).expect("group 0 is the match").end()..].to_string(),
            },
        }
    }
}

/// The file at `path` as the runtime reads it — its content's canonical path
/// and text — or `None` where the runtime loads nothing: not a regular file,
/// over `limit` bytes, or unreadable.
fn read(path: &Path, limit: u64) -> Option<(PathBuf, Source)> {
    let target = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&target).ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let bytes = std::fs::read(&target).ok()?;
    Some((target, Source::parse(&String::from_utf8_lossy(&bytes))))
}

/// Frontmatter keys: the YAML, else the YAML after the runtime's repair, and
/// none where the one that parses is not a mapping.
fn fields(yaml: &str) -> Mapping {
    let parse = |text: &str| yaml_serde::from_str::<Value>(text).ok();
    match parse(yaml).or_else(|| parse(&repaired(yaml))) {
        Some(Value::Mapping(fields)) => fields,
        _ => Mapping::new(),
    }
}

/// YAML as the runtime repairs it after a failed parse: a `key: value` line
/// whose unquoted value holds an indicator is quoted, and each leading tab
/// becomes two spaces.
fn repaired(yaml: &str) -> String {
    let lines: Vec<String> = yaml
        .split('\n')
        .map(|line| {
            let line = match PLAIN_PAIR.captures(line) {
                Some(pair) if needs_quotes(&pair[2]) => format!(
                    "{}: \"{}\"",
                    &pair[1],
                    pair[2].replace('\\', "\\\\").replace('"', "\\\"")
                ),
                _ => line.to_string(),
            };
            let tabs = line.len() - line.trim_start_matches('\t').len();
            format!("{}{}", "  ".repeat(tabs), &line[tabs..])
        })
        .collect();
    lines.join("\n")
}

fn needs_quotes(value: &str) -> bool {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    let flow_list = value.starts_with('[')
        && value.ends_with(']')
        && matches!(yaml_serde::from_str::<Value>(value), Ok(Value::Sequence(_)));
    !quoted && !flow_list && INDICATOR.is_match(value)
}

/// A skill's or command's listing text, or `None` where it is not listed at
/// launch.
fn invocable_entry(source: &Source, kind: MemberKind, listing_cap: usize) -> Option<String> {
    let fields = &source.fields;
    if kind == MemberKind::Skill && path_globs::declares_scope(fields.get("paths")) {
        return None;
    }
    if truthy(fields.get("disable-model-invocation")) {
        return None;
    }
    Some(cap(&listed(source, kind), listing_cap, "\u{2026}"))
}

/// The text a `SKILL.md` lists before the runtime cuts it, whether or not the
/// skill is listed at launch.
pub(crate) fn skill_listing_text(text: &str) -> String {
    listed(&Source::parse(text), MemberKind::Skill)
}

/// The text a skill or command lists before the runtime cuts it: its
/// `description`, else its body's first line, then ` - when_to_use` unless
/// that is empty.
fn listed(source: &Source, kind: MemberKind) -> String {
    let unnamed = match kind {
        MemberKind::Skill => "Skill",
        _ => "Custom command",
    };
    let description = description(source.fields.get("description"))
        .unwrap_or_else(|| fallback_description(&source.body, unnamed));
    match source
        .fields
        .get("when_to_use")
        .and_then(js_string)
        .filter(|when| !when.is_empty())
    {
        Some(when) => format!("{description} - {when}"),
        None => description,
    }
}

/// An agent's name and listing text, its `description`, where the name is one
/// the runtime registers: not led by `-`, and holding no character NFKC folds
/// to a `:` (Unicode 16).
fn agent_entry(fields: &Mapping) -> Option<(String, String)> {
    const COLONS: [char; 5] = [':', '\u{2a74}', '\u{fe13}', '\u{fe55}', '\u{ff1a}'];
    let name = fields
        .get("name")?
        .as_str()
        .filter(|name| !name.is_empty())?;
    if name.starts_with('-') || name.contains(COLONS) {
        return None;
    }
    let description = fields
        .get("description")?
        .as_str()
        .filter(|description| !description.is_empty())?;
    Some((name.to_string(), description.replace("\\n", "\n")))
}

/// A `description` the runtime lists: text trimmed, a number or boolean
/// spelled out, and nothing for an empty one or any other shape.
fn description(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.trim())
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        scalar @ (Value::Bool(_) | Value::Number(_)) => js_string(scalar),
        _ => None,
    }
}

/// `String(value)` as the runtime spells a frontmatter value, `None` for null.
fn js_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Sequence(items) => Some(
            items
                .iter()
                .map(|item| js_string(item).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Mapping(_) => Some("[object Object]".to_string()),
        Value::Tagged(tagged) => js_string(&tagged.value),
    }
}

/// A flag the runtime reads as set: `true`, or `1`, `true`, `yes` or `on`
/// spelled as text or a number.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(set)) => *set,
        Some(spelled @ (Value::String(_) | Value::Number(_))) => {
            js_string(spelled).is_some_and(|s| {
                matches!(
                    s.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
        }
        _ => false,
    }
}

/// A memory file's text as the runtime injects it, and the imports it names
/// as written.
struct MemoryText {
    text: String,
    imports: Vec<String>,
}

fn read_memory(body: &str) -> MemoryText {
    // The runtime rebuilds a file that holds a comment from its lexer's
    // tokens, and that lexer reads every line break as `\n`.
    let normalized;
    let body = if body.contains("<!--") {
        normalized = body.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        body
    };
    let mut text = String::with_capacity(body.len());
    let mut copied = 0usize;
    let mut containers = 0usize;
    let mut in_code = false;
    let mut run = String::new();
    let mut imports = Vec::new();
    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        if let Event::Text(chunk) = &event
            && !in_code
        {
            run.push_str(chunk);
            continue;
        }
        scan_imports(&run, &mut imports);
        run.clear();
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::BlockQuote(_) | Tag::List(_)) => containers += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_)) => containers -= 1,
            Event::Start(Tag::HtmlBlock) => {
                // The runtime's block token runs through the line breaks that
                // follow it, so a comment removed whole takes its blank lines.
                let end = range.end
                    + body[range.end..]
                        .bytes()
                        .take_while(|&b| b == b'\n')
                        .count();
                if let Some(rest) = without_comments(&body[range.start..end]) {
                    let kept = !rest.trim().is_empty();
                    if kept {
                        scan_imports(&rest, &mut imports);
                    }
                    if containers == 0 {
                        text.push_str(&body[copied..range.start]);
                        if kept {
                            text.push_str(&rest);
                        }
                        copied = end;
                    }
                }
            }
            _ => {}
        }
    }
    scan_imports(&run, &mut imports);
    text.push_str(&body[copied..]);
    MemoryText { text, imports }
}
/// `raw` with every `<!-- … -->` removed, when it opens with a comment and
/// closes one; `None` for HTML the runtime keeps and does not read.
fn without_comments(raw: &str) -> Option<String> {
    if !(raw.trim_start().starts_with("<!--") && raw.contains("-->")) {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find("<!--") {
        let Some(close) = rest[open + 4..].find("-->") else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + 4 + close + 3..];
    }
    out.push_str(rest);
    Some(out)
}

/// Every `@token` that opens `run` or follows whitespace in it. A backslash
/// continues a token only before a space.
fn scan_imports(run: &str, out: &mut Vec<String>) {
    let mut previous: Option<char> = None;
    let mut at = 0;
    while let Some(c) = run[at..].chars().next() {
        if c == '@' && previous.is_none_or(char::is_whitespace) {
            let start = at + 1;
            let mut end = start;
            while let Some(d) = run[end..].chars().next() {
                if d == '\\' && run[end + 1..].starts_with(' ') {
                    end += 2;
                } else if d == '\\' || d.is_whitespace() {
                    break;
                } else {
                    end += d.len_utf8();
                }
            }
            out.push(run[start..end].to_string());
            previous = run[..end].chars().next_back();
            at = end;
            continue;
        }
        previous = Some(c);
        at += c.len_utf8();
    }
}

/// The path an import names, or `None` for a token the runtime does not read
/// as one.
fn import_target(written: &str) -> Option<String> {
    let path = written.split('#').next().unwrap_or_default();
    let path = path.replace("\\ ", " ");
    let first = path.chars().next()?;
    let accepted = first.is_ascii_alphanumeric()
        || matches!(first, '.' | '_' | '-')
        || path.starts_with("~/")
        || (first == '/' && path != "/");
    accepted.then_some(path)
}

fn has_text_extension(path: &Path) -> bool {
    path.extension().is_none_or(|ext| {
        let ext = format!(".{}", ext.to_string_lossy().to_lowercase());
        IMPORT_TEXT_EXTENSIONS.contains(&ext.as_str())
    })
}

/// `path` with `.` and `..` resolved lexically, or `None` above the root.
pub(crate) fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// The body's first non-empty line, a heading's text in place of the line, and
/// `unnamed` for a body with none.
fn fallback_description(body: &str, unnamed: &str) -> String {
    let Some(line) = body.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return unnamed.to_string();
    };
    let heading = line
        .strip_prefix('#')
        .map(|rest| rest.trim_start_matches('#'))
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim)
        .filter(|text| !text.is_empty());
    cap(heading.unwrap_or(line), FALLBACK_DESCRIPTION_CAP, "...")
}

/// A text's length as the runtime measures one it cuts: UTF-16 code units, so
/// a character outside the Basic Multilingual Plane counts twice.
pub(crate) fn runtime_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text` cut as the runtime cuts it: past `limit` ([`runtime_len`]), the code
/// units before `marker`, then `marker`. A cut through a surrogate pair leaves
/// half a character, which reaches the model as U+FFFD.
fn cap(text: &str, limit: usize, marker: &str) -> String {
    if runtime_len(text) <= limit {
        return text.to_string();
    }
    let keep = limit - runtime_len(marker);
    let mut out: String = char::decode_utf16(text.encode_utf16().take(keep))
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    out.push_str(marker);
    out
}

fn present(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.is_file())
        .collect()
}

/// The commands the runtime registers under `.claude/commands/`, each with the
/// name `skillOverrides` knows it by: its directories below that one joined by
/// `:`, then its file name without `.md`. A directory holding a `SKILL.md`
/// registers that file alone, named by the directory.
fn commands(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let base = root.join(".claude/commands");
    let namespaced = |dir: &Path, leaf: &str| {
        let namespace: Vec<String> = dir
            .strip_prefix(&base)
            .into_iter()
            .flat_map(Path::components)
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        if namespace.is_empty() {
            leaf.to_string()
        } else {
            format!("{}:{leaf}", namespace.join(":"))
        }
    };
    let mut by_dir: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in discover(root, COMMAND_GLOB)? {
        let dir = path.parent().unwrap_or(&base).to_path_buf();
        by_dir.entry(dir).or_default().push(path);
    }
    let mut out = Vec::new();
    for (dir, files) in by_dir {
        let skill = files.iter().find(|file| {
            file.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("skill.md"))
        });
        match skill {
            Some(skill) => {
                let leaf = dir.file_name().unwrap_or_default().to_string_lossy();
                out.push((
                    skill.clone(),
                    namespaced(dir.parent().unwrap_or(&dir), &leaf),
                ));
            }
            None => {
                for file in files {
                    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
                    let name = namespaced(&dir, &stem);
                    out.push((file, name));
                }
            }
        }
    }
    Ok(out)
}

fn discover(root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let rooted = crate::glob_root::rooted(root, pattern)?;
    let mut out = Vec::new();
    for entry in glob::glob(&rooted).map_err(|e| Error::IoFailure {
        path: root.join(pattern),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("glob: {e}")),
    })? {
        let path = entry.map_err(|e| Error::IoFailure {
            path: e.path().to_path_buf(),
            source: e.into(),
        })?;
        if path.is_file() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    /// A repository of its own, so no directory above the temp dir is read.
    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tree(files);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        dir
    }

    fn loaded(files: &[(&str, &str)]) -> AlwaysLoaded {
        let dir = project(files);
        resolve(dir.path()).unwrap()
    }

    fn paths(loaded: &AlwaysLoaded) -> Vec<&str> {
        let mut paths: Vec<&str> = loaded.members.iter().map(|m| m.path.as_str()).collect();
        paths.sort();
        paths
    }

    fn chars_of(loaded: &AlwaysLoaded, path: &str) -> usize {
        loaded
            .members
            .iter()
            .find(|m| m.path == path)
            .unwrap_or_else(|| panic!("{path} is not a member: {:?}", loaded.members))
            .chars
    }

    fn unmeasured(loaded: &AlwaysLoaded) -> Vec<(&str, UnmeasuredReason)> {
        loaded
            .unmeasured
            .iter()
            .map(|u| (u.name.as_str(), u.reason))
            .collect()
    }

    #[test]
    fn wire_strings_are_the_serialised_form() {
        for kind in MemberKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
        for reason in UnmeasuredReason::ALL {
            assert_eq!(serde_json::to_value(reason).unwrap(), reason.as_str());
        }
    }

    #[test]
    fn both_claude_md_locations_load_and_agents_md_only_without_them() {
        let with = loaded(&[
            ("CLAUDE.md", "root\n"),
            (".claude/CLAUDE.md", "dot\n"),
            ("AGENTS.md", "agents\n"),
        ]);
        assert_eq!(paths(&with), [".claude/CLAUDE.md", "CLAUDE.md"]);
        let without = loaded(&[("AGENTS.md", "agents\n"), (".claude/AGENTS.md", "dot\n")]);
        assert_eq!(paths(&without), [".claude/AGENTS.md", "AGENTS.md"]);
    }

    // What Claude Code 2.1.284 sent from a session started in a package: every
    // level up to the repository's top, nothing from a sibling, and above the
    // top memory files and rules, which are not the repository's to count.
    #[test]
    fn a_session_reads_its_directory_and_each_one_above_it_up_to_the_top() {
        let dir = tree(&[
            ("CLAUDE.md", "above\n"),
            (".claude/rules/above.md", "above\n"),
            ("repo/CLAUDE.md", "top\n"),
            ("repo/.claude/rules/top.md", "top\n"),
            (
                "repo/.claude/skills/top/SKILL.md",
                "---\ndescription: top\n---\n",
            ),
            (
                "repo/.claude/commands/top-cmd.md",
                "---\ndescription: top\n---\n",
            ),
            (
                "repo/.claude/agents/top.md",
                "---\nname: top\ndescription: top\n---\n",
            ),
            ("repo/packages/CLAUDE.md", "mid\n"),
            ("repo/packages/app/CLAUDE.md", "app\n"),
            ("repo/packages/app/.claude/rules/app.md", "app\n"),
            ("repo/packages/other/CLAUDE.md", "sibling\n"),
        ]);
        std::fs::create_dir(dir.path().join("repo/.git")).unwrap();
        let set = resolve(&dir.path().join("repo/packages/app")).unwrap();
        assert_eq!(
            paths(&set),
            [
                "../../.claude/agents/top.md",
                "../../.claude/commands/top-cmd.md",
                "../../.claude/rules/top.md",
                "../../.claude/skills/top/SKILL.md",
                "../../CLAUDE.md",
                "../CLAUDE.md",
                ".claude/rules/app.md",
                "CLAUDE.md",
            ]
        );
    }

    #[test]
    fn outside_any_repository_the_project_stands_alone() {
        let dir = tree(&[("CLAUDE.md", "above\n"), ("project/CLAUDE.md", "own\n")]);
        assert!(
            dir.path().ancestors().all(|d| !d.join(".git").exists()),
            "{} lies inside a repository; point TMPDIR outside one",
            dir.path().display()
        );
        let set = resolve(&dir.path().join("project")).unwrap();
        assert_eq!(paths(&set), ["CLAUDE.md"]);
    }

    // 2.1.284 read a package session's settings from the package alone: the
    // top's selected nothing and excluded nothing there, while the package's
    // reached every level.
    #[test]
    fn only_the_project_s_own_settings_apply_and_they_apply_at_every_level() {
        let dir = project(&[
            (
                ".claude/settings.json",
                r#"{"outputStyle": "top-style", "claudeMdExcludes": ["**/packages/CLAUDE.md"]}"#,
            ),
            (
                ".claude/output-styles/top.md",
                "---\nname: top-style\n---\ntop\n",
            ),
            ("CLAUDE.md", "top\n"),
            (
                ".claude/skills/top/SKILL.md",
                "---\ndescription: top\n---\n",
            ),
            ("packages/CLAUDE.md", "mid\n"),
        ]);
        let top = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/app/.claude")).unwrap();
        std::fs::write(
            dir.path().join("packages/app/.claude/settings.json"),
            serde_json::json!({
                "outputStyle": "top-style",
                "skillOverrides": {"top": "off"},
                "claudeMdExcludes": [top.join("CLAUDE.md")],
            })
            .to_string(),
        )
        .unwrap();
        let app = resolve(&dir.path().join("packages/app")).unwrap();
        assert_eq!(
            paths(&app),
            ["../../.claude/output-styles/top.md", "../CLAUDE.md"]
        );
        let at_top = resolve(dir.path()).unwrap();
        assert_eq!(
            paths(&at_top),
            [
                ".claude/output-styles/top.md",
                ".claude/skills/top/SKILL.md",
                "CLAUDE.md"
            ]
        );
    }

    // 2.1.284 kept one definition per name, the nearest level's, and listed a
    // skill over a command of its name whichever level held either.
    #[test]
    fn a_name_is_the_nearest_level_s_and_a_skill_s_name_hides_a_command() {
        let skill = |d: &str| format!("---\ndescription: {d}\n---\n");
        let agent = |d: &str| format!("---\nname: shared\ndescription: {d}\n---\n");
        let style = |d: &str| format!("---\nname: shared\n---\n{d}\n");
        let files = [
            (".claude/output-styles/shared.md", style("top")),
            (".claude/agents/shared.md", agent("top")),
            (".claude/skills/shared/SKILL.md", skill("top")),
            (".claude/commands/cmd.md", skill("top")),
            (".claude/skills/dup1/SKILL.md", skill("top")),
            (".claude/commands/dup3.md", skill("top")),
            ("packages/.claude/commands/cmd.md", skill("mid")),
            (
                "packages/app/.claude/settings.json",
                r#"{"outputStyle": "shared"}"#.into(),
            ),
            ("packages/app/.claude/output-styles/shared.md", style("app")),
            ("packages/app/.claude/agents/shared.md", agent("app")),
            ("packages/app/.claude/skills/shared/SKILL.md", skill("app")),
            ("packages/app/.claude/commands/dup1.md", skill("app")),
            ("packages/app/.claude/skills/dup2/SKILL.md", skill("app")),
            ("packages/app/.claude/commands/dup2.md", skill("app")),
            ("packages/app/.claude/skills/dup3/SKILL.md", skill("app")),
        ];
        let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
        let dir = project(&files);
        let set = resolve(&dir.path().join("packages/app")).unwrap();
        assert_eq!(
            paths(&set),
            [
                "../../.claude/skills/dup1/SKILL.md",
                "../.claude/commands/cmd.md",
                ".claude/agents/shared.md",
                ".claude/output-styles/shared.md",
                ".claude/skills/dup2/SKILL.md",
                ".claude/skills/dup3/SKILL.md",
                ".claude/skills/shared/SKILL.md",
            ]
        );
    }

    #[test]
    fn an_import_into_another_directory_of_the_repository_is_counted() {
        let dir = project(&[
            ("shared/conventions.md", "shared\n"),
            ("packages/app/CLAUDE.md", "@../../shared/conventions.md\n"),
        ]);
        let set = resolve(&dir.path().join("packages/app")).unwrap();
        assert_eq!(paths(&set), ["../../shared/conventions.md", "CLAUDE.md"]);
        assert!(set.unmeasured.is_empty(), "{:?}", set.unmeasured);
    }

    /// Verdicts of picomatch 4.0.7 with `{ dot: true }` — the matcher Claude
    /// Code 2.1.284 compiles `claudeMdExcludes` with — on the patterns after
    /// its backslash-to-slash turn, over canonical absolute paths.
    const PICOMATCH_PATHS: &[&str] = &[
        "/r/CLAUDE.md",
        "/r/a/CLAUDE.md",
        "/r/a/b/CLAUDE.md",
        "/r/.claude/rules/x.md",
        "/r/.claude/rules/sub/y.md",
        "/r/.hidden/CLAUDE.md",
        "/r/docs/x.md",
        "/r/한 글/CLAUDE.md",
        "/r/My Drive (Work)/CLAUDE.md",
        "/r/{a,b}/CLAUDE.md",
        "/r/[slug]/CLAUDE.md",
        "/r/a+b@c/CLAUDE.md",
        "/r/claude.md",
        "/x/CLAUDE.md",
        "/r/x.txt",
        "/r/ab.md",
    ];
    const PICOMATCH_VERDICTS: &[(&str, &[&str])] = &[
        ("/r/CLAUDE.md", &["/r/CLAUDE.md"]),
        (
            "/r/*/CLAUDE.md",
            &[
                "/r/a/CLAUDE.md",
                "/r/.hidden/CLAUDE.md",
                "/r/한 글/CLAUDE.md",
                "/r/My Drive (Work)/CLAUDE.md",
                "/r/{a,b}/CLAUDE.md",
                "/r/[slug]/CLAUDE.md",
                "/r/a+b@c/CLAUDE.md",
            ],
        ),
        (
            "/r/**/CLAUDE.md",
            &[
                "/r/CLAUDE.md",
                "/r/a/CLAUDE.md",
                "/r/a/b/CLAUDE.md",
                "/r/.hidden/CLAUDE.md",
                "/r/한 글/CLAUDE.md",
                "/r/My Drive (Work)/CLAUDE.md",
                "/r/{a,b}/CLAUDE.md",
                "/r/[slug]/CLAUDE.md",
                "/r/a+b@c/CLAUDE.md",
            ],
        ),
        (
            "**/CLAUDE.md",
            &[
                "/r/CLAUDE.md",
                "/r/a/CLAUDE.md",
                "/r/a/b/CLAUDE.md",
                "/r/.hidden/CLAUDE.md",
                "/r/한 글/CLAUDE.md",
                "/r/My Drive (Work)/CLAUDE.md",
                "/r/{a,b}/CLAUDE.md",
                "/r/[slug]/CLAUDE.md",
                "/r/a+b@c/CLAUDE.md",
                "/x/CLAUDE.md",
            ],
        ),
        (
            "**/.claude/rules/**",
            &["/r/.claude/rules/x.md", "/r/.claude/rules/sub/y.md"],
        ),
        ("/r/.claude/rules/*.md", &["/r/.claude/rules/x.md"]),
        (
            "/r/**",
            &[
                "/r/CLAUDE.md",
                "/r/a/CLAUDE.md",
                "/r/a/b/CLAUDE.md",
                "/r/.claude/rules/x.md",
                "/r/.claude/rules/sub/y.md",
                "/r/.hidden/CLAUDE.md",
                "/r/docs/x.md",
                "/r/한 글/CLAUDE.md",
                "/r/My Drive (Work)/CLAUDE.md",
                "/r/{a,b}/CLAUDE.md",
                "/r/[slug]/CLAUDE.md",
                "/r/a+b@c/CLAUDE.md",
                "/r/claude.md",
                "/r/x.txt",
                "/r/ab.md",
            ],
        ),
        (
            "/r/*",
            &["/r/CLAUDE.md", "/r/claude.md", "/r/x.txt", "/r/ab.md"],
        ),
        (
            "/r/a**/CLAUDE.md",
            &["/r/a/CLAUDE.md", "/r/a+b@c/CLAUDE.md"],
        ),
        ("*CLAUDE.md", &[]),
        ("CLAUDE.md", &[]),
        (
            "/r/{a,docs}/**",
            &["/r/a/CLAUDE.md", "/r/a/b/CLAUDE.md", "/r/docs/x.md"],
        ),
        (
            "/r/*.{md,txt}",
            &["/r/CLAUDE.md", "/r/claude.md", "/r/x.txt", "/r/ab.md"],
        ),
        ("/r/{a,b}{.md,b.md}", &["/r/ab.md"]),
        (
            "/r/{a,b}/CLAUDE.md",
            &["/r/a/CLAUDE.md", "/r/{a,b}/CLAUDE.md"],
        ),
        ("/r/한 글/CLAUDE.md", &["/r/한 글/CLAUDE.md"]),
        ("/r/a+b@c/CLAUDE.md", &["/r/a+b@c/CLAUDE.md"]),
        ("/r/a\\b/CLAUDE.md", &["/r/a/b/CLAUDE.md"]),
    ];

    #[test]
    fn harnex_reads_its_dialect_as_picomatch_does() {
        let written: Vec<&str> = PICOMATCH_VERDICTS.iter().map(|(p, _)| *p).collect();
        let patterns = exclude_patterns(&serde_json::json!({ "claudeMdExcludes": written }));
        assert_eq!(patterns.len(), PICOMATCH_VERDICTS.len());
        for ((written, expected), pattern) in PICOMATCH_VERDICTS.iter().zip(&patterns) {
            let glob = ExcludeGlob::read(pattern)
                .unwrap_or_else(|| panic!("`{written}` is written in the dialect harnex reads"));
            let matched: Vec<&str> = PICOMATCH_PATHS
                .iter()
                .copied()
                .filter(|path| glob.matches(path))
                .collect();
            assert_eq!(&matched, expected, "`{written}`");
        }
        for unread in [
            "/r/?/CLAUDE.md",
            "/r/[unclosed.md",
            "/r/[slug]/CLAUDE.md",
            "/r/{a}/CLAUDE.md",
            "/r/{a/CLAUDE.md",
            "/r/}x/CLAUDE.md",
            "/r/{a,{b,c}}/CLAUDE.md",
            "/r/c{1..2}/CLAUDE.md",
            "/r/{,a}/CLAUDE.md",
            "/r/{a,**}/CLAUDE.md",
            "/r/My Drive (Work)/CLAUDE.md",
            "/r/@(a|b)/CLAUDE.md",
            "!/r/CLAUDE.md",
        ] {
            assert!(ExcludeGlob::read(unread).is_none(), "`{unread}`");
        }
    }

    // 2.1.284 read an unclosed bracket as a literal one and applied every
    // other pattern in the list; harnex names that one instead of refusing
    // the settings.
    #[test]
    fn a_pattern_outside_the_dialect_is_named_and_the_rest_still_apply() {
        let dir = project(&[
            ("CLAUDE.md", "memory\n"),
            (".claude/rules/[unclosed.md", "bracket\n"),
            (".claude/rules/plain.md", "plain\n"),
        ]);
        let top = std::fs::canonicalize(dir.path()).unwrap();
        let bracket = format!("{}/.claude/rules/[unclosed.md", top.display());
        let backslashed = format!("{}\\.claude\\rules\\plain.md", top.display());
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            serde_json::json!({ "claudeMdExcludes": [&bracket, backslashed] }).to_string(),
        )
        .unwrap();
        let set = resolve(dir.path()).unwrap();
        assert_eq!(paths(&set), [".claude/rules/[unclosed.md", "CLAUDE.md"]);
        assert_eq!(set.unread_excludes, [bracket]);
    }

    // 2.1.284 skipped a rule named `a\b.md` for `**/.claude/rules/a/b.md`, and
    // loaded it with no exclude: the path's backslashes turn too.
    #[cfg(unix)]
    #[test]
    fn a_backslash_in_the_path_reads_as_a_slash() {
        let set = loaded(&[
            (
                ".claude/settings.json",
                r#"{"claudeMdExcludes": ["**/.claude/rules/a/b.md"]}"#,
            ),
            (".claude/rules/a\\b.md", "rule\n"),
        ]);
        assert!(set.members.is_empty(), "{:?}", set.members);
    }

    #[test]
    fn the_counted_text_is_the_text_the_runtime_injects() {
        // A removed comment block takes the line breaks after it, and the
        // whole text is trimmed — what 2.1.283 sent for this file.
        let set = loaded(&[("CLAUDE.md", "\nfirst\n\n<!-- note -->\n\nsecond\n\n")]);
        assert_eq!(chars_of(&set, "CLAUDE.md"), "first\n\nsecond".len());
        // A file holding a comment reaches the model with `\n` line breaks;
        // one without keeps its `\r\n` — 2.1.283 sent 29 and 32 here.
        let crlf = loaded(&[
            (
                "CLAUDE.md",
                "MARK_CRLF_A\r\n\r\n<!-- c -->\r\n\r\nMARK_CRLF_B\r\nline\r\n",
            ),
            (
                ".claude/CLAUDE.md",
                "MARK_CRLF2_A\r\nline\r\nMARK_CRLF2_B\r\n",
            ),
        ]);
        assert_eq!(chars_of(&crlf, "CLAUDE.md"), 29);
        assert_eq!(chars_of(&crlf, ".claude/CLAUDE.md"), 32);
    }

    #[test]
    fn a_member_counts_characters_not_bytes() {
        let set = loaded(&[("CLAUDE.md", "한국어 문장\n")]);
        assert_eq!(set.total_chars, "한국어 문장".chars().count());
    }

    #[test]
    fn frontmatter_and_top_level_comment_blocks_leave_the_text() {
        let text = read_memory(
            "a\n\n<!-- gone -->\n\n<!--\nalso gone\n-->\n\n<!-- gone --> kept\n\nb <!-- inline stays -->\n\n- item\n\n  <!-- listed stays -->\n\n> <!-- quoted stays -->\n\n<!-- gone after the list -->\n",
        )
        .text;
        assert!(!text.contains("gone"), "{text:?}");
        assert!(text.contains(" kept"), "{text:?}");
        assert!(text.contains("inline stays"), "{text:?}");
        assert!(text.contains("listed stays"), "{text:?}");
        assert!(text.contains("quoted stays"), "{text:?}");
        let set = loaded(&[("CLAUDE.md", "---\ntitle: x\n---\nbody\n")]);
        assert_eq!(chars_of(&set, "CLAUDE.md"), "body".len());
    }

    #[test]
    fn frontmatter_is_split_and_repaired_as_the_runtime_does() {
        let unclosed = "---\nA rule under a thematic break.\n\nbody\n";
        let set = loaded(&[
            ("CLAUDE.md", unclosed),
            (".claude/CLAUDE.md", "---\ntitle: a---\nbody\n"),
            (
                ".claude/rules/repaired.md",
                "---\npaths: **/*.ts\n---\nscoped once repaired\n",
            ),
            (
                ".claude/rules/listed.md",
                "---\npaths:\n  - **/*.ts\n---\nbeyond repair\n",
            ),
            (
                ".claude/rules/tabbed.md",
                "---\npaths:\n\t- \"src/**\"\n---\nscoped once untabbed\n",
            ),
        ]);
        assert_eq!(chars_of(&set, "CLAUDE.md"), unclosed.trim().len());
        assert_eq!(chars_of(&set, ".claude/CLAUDE.md"), "body".len());
        assert_eq!(
            paths(&set),
            [".claude/CLAUDE.md", ".claude/rules/listed.md", "CLAUDE.md"]
        );
        assert_eq!(
            chars_of(&set, ".claude/rules/listed.md"),
            "beyond repair".len()
        );
    }

    #[test]
    fn a_file_the_runtime_skips_loads_nothing_and_bad_bytes_are_replaced() {
        let oversized = "x".repeat(MEMORY_FILE_LIMIT as usize + 1);
        let at_limit = "y".repeat(SKILL_FILE_LIMIT as usize - "---\ndescription: z\n---\n".len());
        let dir = project(&[
            ("CLAUDE.md", "@big.md\n"),
            ("big.md", &oversized),
            (
                ".claude/skills/edge/SKILL.md",
                &format!("---\ndescription: z\n---\n{at_limit}"),
            ),
            (
                ".claude/skills/over/SKILL.md",
                &format!("---\ndescription: z\n---\n{at_limit}!"),
            ),
        ]);
        std::fs::write(dir.path().join(".claude/CLAUDE.md"), b"ok \xff\n").unwrap();
        let set = resolve(dir.path()).unwrap();
        assert_eq!(
            paths(&set),
            [
                ".claude/CLAUDE.md",
                ".claude/skills/edge/SKILL.md",
                "CLAUDE.md"
            ]
        );
        assert_eq!(
            chars_of(&set, ".claude/CLAUDE.md"),
            "ok \u{fffd}".chars().count()
        );
    }

    #[test]
    fn an_import_is_a_token_the_runtime_reads_as_one() {
        let set = loaded(&[
            (
                "CLAUDE.md",
                "@docs/start.md\n\nsee @docs/space.md here\n\n# @docs/heading.md\n\n\
                 See @docs/dot.md. later\n\n`@docs/span.md`\n\n```\n@docs/fence.md\n```\n\n    \
                 @docs/indented.md\n\nmail a@docs/mail.md\n\n(@docs/paren.md)\n\n\
                 <!-- @docs/comment.md -->\n\nsee @docs/sp\\ ace.md here\n\n@docs/missing.md\n\n\
                 @docs\n\nline one\n@docs/wrapped.md\n\n<div> @docs/html.md --></div>\n",
            ),
            ("docs/start.md", "x"),
            ("docs/space.md", "x"),
            ("docs/heading.md", "x"),
            ("docs/dot.md", "x"),
            ("docs/span.md", "x"),
            ("docs/fence.md", "x"),
            ("docs/indented.md", "x"),
            ("docs/mail.md", "x"),
            ("docs/paren.md", "x"),
            ("docs/comment.md", "x"),
            ("docs/sp ace.md", "x"),
            ("docs/wrapped.md", "x"),
            ("docs/html.md", "x"),
        ]);
        assert_eq!(
            paths(&set),
            [
                "CLAUDE.md",
                "docs/heading.md",
                "docs/sp ace.md",
                "docs/space.md",
                "docs/start.md",
                "docs/wrapped.md"
            ]
        );
    }

    #[test]
    fn an_import_loads_a_text_extension_or_none() {
        let set = loaded(&[
            (
                "CLAUDE.md",
                "@a.MD\n\n@Makefile\n\n@a.toml\n\n@a.weirdext\n\n@a.png\n\n@a.md,\n\n@~/pic.png\n",
            ),
            ("a.MD", "x"),
            ("Makefile", "x"),
            ("a.toml", "x"),
            ("a.weirdext", "x"),
            ("a.png", "x"),
            ("a.md,", "x"),
        ]);
        assert_eq!(paths(&set), ["CLAUDE.md", "Makefile", "a.MD", "a.toml"]);
        assert!(set.unmeasured.is_empty(), "{:?}", set.unmeasured);
    }

    #[test]
    fn imports_reach_four_hops_and_load_a_file_once() {
        let set = loaded(&[
            ("CLAUDE.md", "@d/1.md\n\n@d/shared.md\n\n@d/empty.md\n"),
            (".claude/CLAUDE.md", "@../d/shared.md\n"),
            ("d/1.md", "@2.md\n"),
            ("d/2.md", "@3.md\n"),
            ("d/3.md", "@4.md\n"),
            ("d/4.md", "@5.md\n"),
            ("d/5.md", "five\n"),
            ("d/shared.md", "shared\n"),
            ("d/empty.md", "\n<!-- nothing -->\n"),
        ]);
        assert_eq!(
            paths(&set),
            [
                ".claude/CLAUDE.md",
                "CLAUDE.md",
                "d/1.md",
                "d/2.md",
                "d/3.md",
                "d/4.md",
                "d/shared.md"
            ]
        );
    }

    #[test]
    fn a_rule_loads_when_its_paths_scope_nothing_and_brings_its_imports() {
        let set = loaded(&[
            (".claude/rules/plain.md", "@../../docs/r.md\n"),
            (".claude/rules/sub/nested.md", "nested\n"),
            (".claude/rules/star.md", "---\npaths: \"**\"\n---\nstar\n"),
            (
                ".claude/rules/scoped.md",
                "---\npaths: \"src/**\"\n---\nscoped\n",
            ),
            (
                ".claude/rules/governed.md",
                "---\npaths: \"src/**\"\ngoverns: not a harnex shape\n---\nscoped\n",
            ),
            ("docs/r.md", "imported\n@ts-only.md\n"),
            (
                ".claude/rules/scoped-importer.md",
                "---\npaths: \"src/**\"\n---\n@../../docs/unscoped.md\n",
            ),
            ("docs/unscoped.md", "loads though its importer waits\n"),
            ("docs/ts-only.md", "---\npaths: \"**/*.ts\"\n---\nwaits\n"),
            ("CLAUDE.md", "@docs/memory-import.md\n"),
            (
                "docs/memory-import.md",
                "---\npaths: \"src/**\"\n---\nloads from CLAUDE.md\n",
            ),
        ]);
        // What 2.1.283 sent: in a rule's tree every file answers to its own
        // `paths:`, and a file `CLAUDE.md` imports loads whatever it says.
        assert_eq!(
            paths(&set),
            [
                ".claude/rules/plain.md",
                ".claude/rules/star.md",
                ".claude/rules/sub/nested.md",
                "CLAUDE.md",
                "docs/memory-import.md",
                "docs/r.md",
                "docs/unscoped.md"
            ]
        );
        assert!(set.members.iter().any(|m| m.path == ".claude/rules/star.md"
            && m.kind == MemberKind::Rule
            && m.chars == "star".len()));
    }

    #[test]
    fn a_rule_imported_first_is_counted_once_as_the_import() {
        let set = loaded(&[
            ("CLAUDE.md", "@.claude/rules/r.md\n"),
            (".claude/rules/r.md", "rule\n"),
        ]);
        let rule: Vec<&Member> = set
            .members
            .iter()
            .filter(|m| m.path == ".claude/rules/r.md")
            .collect();
        assert_eq!(rule.len(), 1);
        assert_eq!(rule[0].kind, MemberKind::Import);
    }

    #[test]
    fn an_exclude_matches_the_absolute_path_and_a_relative_one_matches_nothing() {
        let dir = project(&[
            (
                "CLAUDE.md",
                "@docs/rel.md\n\n@docs/star.md\n\n@docs/spelled.md\n",
            ),
            ("docs/rel.md", "x"),
            ("docs/star.md", "x"),
            ("docs/spelled.md", "x"),
            (".claude/rules/abs.md", "x"),
            (
                ".claude/settings.local.json",
                r#"{"claudeMdExcludes": ["**/CLAUDE.md"]}"#,
            ),
        ]);
        let absolute = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            serde_json::json!({ "claudeMdExcludes": [
                "docs/rel.md",
                "**/docs/star.md",
                format!("{}/.claude/rules/*.md", absolute.display()),
                format!("{}/docs/spelled.md", dir.path().display()),
            ]})
            .to_string(),
        )
        .unwrap();
        let set = resolve(dir.path()).unwrap();
        assert_eq!(paths(&set), ["CLAUDE.md", "docs/rel.md"]);
    }

    #[test]
    fn an_import_outside_the_project_is_named_and_not_counted() {
        let set = loaded(&[
            (
                "CLAUDE.md",
                "@~/.claude/mine.md\n\n@../../elsewhere.md\n\n@../../skipped.md\n\n\
                 @/etc/elsewhere.md\n\n@/\n",
            ),
            (
                ".claude/settings.json",
                r#"{"claudeMdExcludes": ["**/skipped.md"]}"#,
            ),
        ]);
        assert_eq!(paths(&set), ["CLAUDE.md"]);
        assert_eq!(
            unmeasured(&set),
            [
                ("../../elsewhere.md", UnmeasuredReason::OutsideProject),
                ("/etc/elsewhere.md", UnmeasuredReason::OutsideProject),
                ("~/.claude/mine.md", UnmeasuredReason::OutsideProject)
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_counted_where_its_content_lives() {
        use std::os::unix::fs::symlink;
        let rules = project(&[("shared.md", "shared rule\n")]);
        let file = project(&[("secret.md", "secret\n")]);
        let dir = project(&[
            ("CLAUDE.md", "@docs/import.md\n"),
            ("docs/local.md", "local\n"),
        ]);
        let root = dir.path();
        symlink(file.path().join("secret.md"), root.join("docs/import.md")).unwrap();
        std::fs::create_dir_all(root.join(".claude/rules")).unwrap();
        symlink(rules.path(), root.join(".claude/rules/shared")).unwrap();
        symlink("../../docs/local.md", root.join(".claude/rules/local.md")).unwrap();
        symlink("../CLAUDE.md", root.join(".claude/CLAUDE.md")).unwrap();
        let set = resolve(root).unwrap();
        assert_eq!(paths(&set), [".claude/rules/local.md", "CLAUDE.md"]);
        assert_eq!(
            unmeasured(&set),
            [
                (
                    ".claude/rules/shared/shared.md",
                    UnmeasuredReason::OutsideProject
                ),
                ("docs/import.md", UnmeasuredReason::OutsideProject),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_rule_is_excluded_by_its_rules_path_or_its_target() {
        use std::os::unix::fs::symlink;
        for (pattern, loads) in [
            ("**/.claude/rules/linked/*.md", 0),
            ("**/docs/target.md", 0),
            ("**/docs/elsewhere.md", 1),
        ] {
            let dir = project(&[
                ("docs/target.md", "rule\n"),
                (
                    ".claude/settings.json",
                    &serde_json::json!({ "claudeMdExcludes": [pattern] }).to_string(),
                ),
            ]);
            std::fs::create_dir_all(dir.path().join(".claude/rules")).unwrap();
            symlink("../../docs", dir.path().join(".claude/rules/linked")).unwrap();
            let set = resolve(dir.path()).unwrap();
            assert_eq!(set.members.len(), loads, "{pattern}: {:?}", set.members);
        }
    }

    #[test]
    fn the_selected_style_is_found_by_name_before_file_name() {
        let set = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Terse"}"#),
            (
                ".claude/output-styles/x.md",
                "---\nname: Terse\n---\nby name <!-- kept -->\n",
            ),
            (".claude/output-styles/Terse.md", "by file name\n"),
        ]);
        assert_eq!(paths(&set), [".claude/output-styles/x.md"]);
        assert_eq!(
            chars_of(&set, ".claude/output-styles/x.md"),
            "by name <!-- kept -->".len()
        );

        let by_stem = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Terse"}"#),
            (".claude/output-styles/other.md", "another style\n"),
            (".claude/output-styles/team/Terse.md", "by file name\n"),
        ]);
        assert_eq!(paths(&by_stem), [".claude/output-styles/team/Terse.md"]);

        let local_is_theirs = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Terse"}"#),
            (
                ".claude/settings.local.json",
                r#"{"outputStyle": "Explanatory"}"#,
            ),
            (".claude/output-styles/Terse.md", "by file name\n"),
        ]);
        assert_eq!(paths(&local_is_theirs), [".claude/output-styles/Terse.md"]);

        let renamed = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Terse"}"#),
            (
                ".claude/output-styles/Terse.md",
                "---\nname: Concise\n---\nbody\n",
            ),
        ]);
        assert!(renamed.members.is_empty(), "{:?}", renamed.members);

        let unshipped = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Explanatory"}"#),
            (".claude/output-styles/other.md", "another style\n"),
        ]);
        assert_eq!(
            unmeasured(&unshipped),
            [("Explanatory", UnmeasuredReason::NotInProject)]
        );
        let built_in = loaded(&[(".claude/settings.json", r#"{"outputStyle": "default"}"#)]);
        assert!(built_in.members.is_empty() && built_in.unmeasured.is_empty());
    }

    #[test]
    fn a_listing_entry_counts_the_text_the_runtime_lists() {
        let long = "x".repeat(2000);
        let set = loaded(&[
            (
                ".claude/skills/both/SKILL.md",
                "---\nname: both\ndescription: \"  Deploy  \"\nwhen_to_use: on release\n---\nbody\n",
            ),
            (
                ".claude/skills/numbered/SKILL.md",
                "---\ndescription: 42\n---\nbody line\n",
            ),
            (
                ".claude/skills/hidden/SKILL.md",
                "---\nname: hidden\ndescription: x\ndisable-model-invocation: \"yes\"\n---\nbody\n",
            ),
            (
                ".claude/skills/conditional/SKILL.md",
                "---\ndescription: x\npaths: \"src/**\"\n---\nbody\n",
            ),
            (
                ".claude/skills/bare/SKILL.md",
                "---\nname: bare\n---\n\n# Heading text\n\nmore\n",
            ),
            (".claude/skills/empty/SKILL.md", "---\nname: empty\n---\n"),
            (
                ".claude/skills/listed/SKILL.md",
                "---\ndescription: Ship\nwhen_to_use:\n  - tagging\n  - releasing\n---\n",
            ),
            (
                ".claude/skills/long/SKILL.md",
                &format!("---\nname: long\ndescription: {long}\n---\nbody\n"),
            ),
            (
                ".claude/skills/emoji/SKILL.md",
                &format!("---\ndescription: {}\n---\nbody\n", "😀".repeat(1000)),
            ),
            (".claude/commands/sub/run.md", "first line\nsecond\n"),
            (".claude/commands/blank.md", ""),
            (
                ".claude/agents/named.md",
                "---\nname: named\ndescription: Reviews\\n\\nthings\n---\nbody\n",
            ),
            (
                ".claude/agents/nameless.md",
                "---\ndescription: Unlisted\n---\nbody\n",
            ),
            (
                ".claude/agents/dashed.md",
                "---\nname: -dashed\ndescription: Unlisted\n---\nbody\n",
            ),
        ]);
        assert_eq!(
            chars_of(&set, ".claude/skills/both/SKILL.md"),
            "Deploy - on release".len()
        );
        assert_eq!(
            chars_of(&set, ".claude/skills/bare/SKILL.md"),
            "Heading text".len()
        );
        assert_eq!(
            chars_of(&set, ".claude/skills/empty/SKILL.md"),
            "Skill".len()
        );
        assert_eq!(chars_of(&set, ".claude/skills/numbered/SKILL.md"), 2);
        assert_eq!(
            chars_of(&set, ".claude/skills/listed/SKILL.md"),
            "Ship - tagging,releasing".len()
        );
        assert_eq!(
            chars_of(&set, ".claude/skills/long/SKILL.md"),
            LISTING_ENTRY_CAP
        );
        // 1,535 code units keep 767 emoji and half of the next.
        assert_eq!(chars_of(&set, ".claude/skills/emoji/SKILL.md"), 767 + 2);
        assert_eq!(
            chars_of(&set, ".claude/commands/sub/run.md"),
            "first line".len()
        );
        assert_eq!(
            chars_of(&set, ".claude/commands/blank.md"),
            "Custom command".len()
        );
        assert_eq!(
            chars_of(&set, ".claude/agents/named.md"),
            "Reviews\n\nthings".len()
        );
        for absent in [
            ".claude/skills/hidden/SKILL.md",
            ".claude/skills/conditional/SKILL.md",
            ".claude/agents/nameless.md",
            ".claude/agents/dashed.md",
        ] {
            assert!(!paths(&set).contains(&absent), "{absent}");
        }
    }

    #[test]
    fn the_committed_listing_cap_replaces_the_default() {
        let entry = |settings: &str| {
            let set = loaded(&[
                (".claude/settings.json", settings),
                (
                    ".claude/skills/s/SKILL.md",
                    "---\ndescription: abcdefghijklmnop\n---\n",
                ),
            ]);
            chars_of(&set, ".claude/skills/s/SKILL.md")
        };
        assert_eq!(entry(r#"{"skillListingMaxDescChars": 10}"#), 10);
        assert_eq!(entry(r#"{"skillListingMaxDescChars": 0}"#), 16);
        assert_eq!(entry("{}"), 16);
    }

    #[test]
    fn a_listing_entry_follows_the_committed_overrides_and_is_listed_once() {
        // What 2.1.283 sent: `off` and `user-invocable-only` leave the listing,
        // `name-only` lists no description, a command is overridden by its
        // `:`-joined name, and one agent name registers once.
        let set = loaded(&[
            (
                ".claude/settings.json",
                r#"{"skillOverrides": {"big": "off", "terse": "name-only",
                    "uio": "user-invocable-only", "sub:run": "off", "kept": "on"}}"#,
            ),
            (
                ".claude/skills/big/SKILL.md",
                "---\ndescription: big\n---\n",
            ),
            (
                ".claude/skills/terse/SKILL.md",
                "---\ndescription: terse\n---\n",
            ),
            (
                ".claude/skills/uio/SKILL.md",
                "---\ndescription: uio\n---\n",
            ),
            (
                ".claude/skills/kept/SKILL.md",
                "---\ndescription: kept\n---\n",
            ),
            (
                ".claude/commands/sub/run.md",
                "---\ndescription: run\n---\n",
            ),
            (".claude/commands/run.md", "---\ndescription: top\n---\n"),
            (
                ".claude/commands/packed/SKILL.md",
                "---\ndescription: packed\n---\n",
            ),
            (
                ".claude/commands/packed/extra.md",
                "---\ndescription: extra\n---\n",
            ),
            (
                ".claude/agents/a.md",
                "---\nname: dup\ndescription: first\n---\n",
            ),
            (
                ".claude/agents/b.md",
                "---\nname: dup\ndescription: second, longer\n---\n",
            ),
            (
                ".claude/agents/colon.md",
                "---\nname: 리뷰어\u{ff1a}코드\ndescription: unregistered\n---\n",
            ),
        ]);
        assert_eq!(
            paths(&set),
            [
                ".claude/agents/a.md",
                ".claude/commands/packed/SKILL.md",
                ".claude/commands/run.md",
                ".claude/skills/kept/SKILL.md"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_reached_through_a_link_is_read_where_it_lives() {
        use std::os::unix::fs::symlink;
        let dir = project(&[
            ("shared/rules/common.md", "common\n\n@./style.md\n"),
            ("shared/rules/style.md", "style\n"),
            (
                ".claude/skills/deploy/SKILL.md",
                "---\ndescription: deploy\n---\n",
            ),
        ]);
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude/rules")).unwrap();
        symlink(
            "../../shared/rules/common.md",
            root.join(".claude/rules/common.md"),
        )
        .unwrap();
        symlink("deploy", root.join(".claude/skills/ship")).unwrap();
        let set = resolve(root).unwrap();
        assert_eq!(
            paths(&set),
            [
                ".claude/rules/common.md",
                ".claude/skills/deploy/SKILL.md",
                "shared/rules/style.md"
            ]
        );
    }

    #[test]
    fn a_description_from_the_body_is_its_first_line_cut_to_a_hundred() {
        assert_eq!(fallback_description("\n\n## Title\nnext", "Skill"), "Title");
        assert_eq!(fallback_description("#tag line", "Skill"), "#tag line");
        assert_eq!(fallback_description("", "Skill"), "Skill");
        let cut = fallback_description(&"y".repeat(150), "Skill");
        assert_eq!(cut.chars().count(), FALLBACK_DESCRIPTION_CAP);
        assert!(cut.ends_with("..."));
        let emoji = fallback_description(&"😀".repeat(60), "Skill");
        assert_eq!(emoji, format!("{}\u{fffd}...", "😀".repeat(48)));
    }

    #[test]
    fn the_repair_quotes_only_an_unquoted_value_holding_an_indicator() {
        // `broken` fails the first parse; every other line is one the repair
        // must leave as it is, or quote, for the whole block to parse right.
        let fields = fields(
            "broken: a: \"b\"\nsingle: a: 'b'\nquoted: 'q: r'\nlist: [\"src/**\", x]\n\
             alias: [*x]\ncommented: [a, b] # c\nclosing: - a ]\nplain: text",
        );
        let text = |key: &str| fields.get(key).and_then(|v| v.as_str());
        assert_eq!(text("broken"), Some("a: \"b\""));
        assert_eq!(text("single"), Some("a: 'b'"));
        assert_eq!(text("quoted"), Some("q: r"));
        assert!(
            fields.get("list").is_some_and(Value::is_sequence),
            "{fields:?}"
        );
        assert_eq!(text("alias"), Some("[*x]"));
        assert_eq!(text("commented"), Some("[a, b] # c"));
        assert_eq!(text("closing"), Some("- a ]"));
        assert_eq!(text("plain"), Some("text"));
    }

    #[test]
    fn a_flag_is_set_the_ways_the_runtime_spells_it() {
        let flag = |yaml: &str| truthy(fields(yaml).get("f"));
        for set in ["f: true", "f: \"true\"", "f: \"Yes\"", "f: \"on\"", "f: 1"] {
            assert!(flag(set), "{set}");
        }
        for unset in ["f: false", "f: \"no\"", "f: 0", "f: [true]", "g: true"] {
            assert!(!flag(unset), "{unset}");
        }
    }

    #[test]
    fn the_budget_holds_at_its_limit_and_names_the_largest_member_past_it() {
        let dir = project(&[("CLAUDE.md", "12345\n"), (".claude/CLAUDE.md", "1\n")]);
        let set = resolve(dir.path()).unwrap();
        assert_eq!(set.total_chars, 6);
        assert!(over_budget(&set, 6, dir.path()).is_none());
        let finding = over_budget(&set, 5, dir.path()).unwrap();
        assert_eq!(finding.slug, "always-loaded-over-budget");
        assert_eq!(finding.location.path, dir.path().join("CLAUDE.md"));
    }
}
