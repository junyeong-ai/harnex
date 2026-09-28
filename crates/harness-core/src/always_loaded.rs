//! # always_loaded — the text a repository puts into every Claude Code session
//!
//! Part of every session's context is assembled from files a repository
//! commits: its memory files and what they import, the rules that carry no
//! scope, the output style its settings select, and one listing entry per
//! skill, command and agent. [`resolve`] reads that set the way the runtime
//! does and counts what each member contributes, so a budget can be held over
//! the whole set rather than over one rule file at a time ([`over_budget`]).
//!
//! The reading is the runtime's, measured on Claude Code 2.1.283 from the
//! request it sends and the loader it ships:
//!
//! - `CLAUDE.md` and `.claude/CLAUDE.md` both load; `AGENTS.md` and
//!   `.claude/AGENTS.md` load in their place when neither exists.
//! - A rule under `.claude/rules/` loads when its `paths:` scopes nothing
//!   (`validate::path_globs::declares_scope`).
//! - A memory file loses its frontmatter and each top-level HTML block that
//!   opens with a comment. An inline comment, and one inside a list or a
//!   quote, stays.
//! - `@path` imports a file when it opens a text run or follows whitespace
//!   outside code and comments, begins with a letter, a digit, `.`, `_`, `-`,
//!   `~/` or `/`, and names a file whose extension is in
//!   [`IMPORT_TEXT_EXTENSIONS`] or that has none. It resolves against the
//!   importing file, reaches [`MAX_IMPORT_HOPS`] deep, and loads a file once.
//! - `claudeMdExcludes` in either project settings file removes a memory file
//!   whose absolute path it matches; a relative pattern matches nothing.
//! - The output style is the body of the file whose `name`, or else whose file
//!   name, is `outputStyle`, comments included.
//! - A skill or command lists `description` — else its body's first non-empty
//!   line, a heading's text, cut to [`FALLBACK_DESCRIPTION_CAP`] — and
//!   ` - when_to_use`, cut to [`LISTING_ENTRY_CAP`], unless it sets
//!   `disable-model-invocation: true`. An agent lists its `description`.
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
//!   user-level or ancestor memory file and auto memory are each developer's
//!   own; an import reaching outside the project and an output style the
//!   project does not ship are [`Unmeasured`], named rather than guessed.
//! - Never guesses at a file it cannot read. An unreadable file, or
//!   frontmatter that does not parse, is [`Unmeasured`]: whether the runtime
//!   loads it at all is unknown.
//! - Never counts tokens. That needs the model's tokenizer, which is not
//!   available offline, and characters are exact.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::validate::{
    AgentValidator, OutputStyleValidator, RuleValidator, SkillValidator, SurfaceValidator,
    frontmatter, path_globs,
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

/// Characters a listing entry's text is cut to, the last one an ellipsis.
pub const LISTING_ENTRY_CAP: usize = 1536;

/// Characters a description taken from a body line is cut to.
pub const FALLBACK_DESCRIPTION_CAP: usize = 100;

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
        /// An import resolving outside the project, `~/` included.
        OutsideProject => "outside-project",
        /// An output style the project does not ship: built in, or a user's.
        NotInProject => "not-in-project",
        /// A file, or its frontmatter, that cannot be read.
        Unreadable => "unreadable",
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
}

/// Read the set rooted at `root`, the directory `harness.toml` lives in.
pub fn resolve(root: &Path) -> Result<AlwaysLoaded> {
    let settings = ProjectSettings::read(root)?;
    let mut walk = Walk {
        root,
        excludes: &settings.excludes,
        seen: HashSet::new(),
        members: Vec::new(),
        unmeasured: Vec::new(),
    };

    let claude_md = present(root, &["CLAUDE.md", ".claude/CLAUDE.md"]);
    let memory = if claude_md.is_empty() {
        present(root, &["AGENTS.md", ".claude/AGENTS.md"])
    } else {
        claude_md
    };
    for path in memory {
        walk.memory_file(&path, MemberKind::Memory, 0);
    }
    for path in discover(root, <RuleValidator as SurfaceValidator>::GLOB)? {
        walk.rule(&path);
    }
    if let Some(name) = &settings.output_style {
        walk.output_style(name)?;
    }
    for path in discover(root, <SkillValidator as SurfaceValidator>::GLOB)? {
        walk.listing(&path, MemberKind::Skill);
    }
    for path in discover(root, ".claude/commands/**/*.md")? {
        walk.listing(&path, MemberKind::Command);
    }
    for path in discover(root, <AgentValidator as SurfaceValidator>::GLOB)? {
        walk.listing(&path, MemberKind::Agent);
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
    })
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

/// `claudeMdExcludes` as the runtime reads it: glob patterns matched against
/// the canonical absolute path of a memory file — `CLAUDE.md`, a rule or an
/// import — so a relative pattern matches nothing.
pub(crate) struct Excludes(Vec<globset::GlobMatcher>);

impl Excludes {
    /// The patterns in both project settings files.
    pub(crate) fn read(root: &Path) -> Result<Self> {
        Ok(ProjectSettings::read(root)?.excludes)
    }

    /// Whether the runtime skips the memory file at `path`, which exists.
    pub(crate) fn matches(&self, path: &Path) -> bool {
        std::fs::canonicalize(path)
            .is_ok_and(|absolute| self.0.iter().any(|glob| glob.is_match(&absolute)))
    }
}

/// The project settings the set depends on, merged as the runtime merges the
/// two project scopes: the local file wins a key, and lists concatenate.
struct ProjectSettings {
    output_style: Option<String>,
    excludes: Excludes,
}

impl ProjectSettings {
    fn read(root: &Path) -> Result<Self> {
        let mut output_style = None;
        let mut excludes = Vec::new();
        for scope in [".claude/settings.json", ".claude/settings.local.json"] {
            // A settings file that is absent or not JSON contributes nothing;
            // the malformed one is `validate.settings`' finding.
            let Some(value) = std::fs::read_to_string(root.join(scope))
                .ok()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            else {
                continue;
            };
            if let Some(style) = value.get("outputStyle").and_then(|v| v.as_str()) {
                output_style = Some(style.to_string());
            }
            for pattern in value
                .get("claudeMdExcludes")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
            {
                let glob = path_globs::compile_glob(pattern).map_err(|e| Error::ConfigInvalid {
                    message: format!(
                        "{scope}: claudeMdExcludes pattern '{pattern}' is invalid: {e}"
                    ),
                    location: None,
                })?;
                excludes.push(glob.compile_matcher());
            }
        }
        Ok(Self {
            output_style,
            excludes: Excludes(excludes),
        })
    }
}

struct Walk<'a> {
    root: &'a Path,
    excludes: &'a Excludes,
    seen: HashSet<PathBuf>,
    members: Vec<Member>,
    unmeasured: Vec<Unmeasured>,
}

impl Walk<'_> {
    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.root)
            .expect("every path the walk reads was found or kept under the root")
            .to_string_lossy()
            .into_owned()
    }

    fn count(&mut self, kind: MemberKind, path: &Path, text: &str) {
        let path = self.relative(path);
        self.members.push(Member {
            kind,
            path,
            chars: text.chars().count(),
        });
    }

    fn unmeasured(&mut self, kind: MemberKind, name: String, reason: UnmeasuredReason) {
        self.unmeasured.push(Unmeasured { kind, name, reason });
    }

    fn rule(&mut self, path: &Path) {
        let Some((yaml, body)) = self.split(path, MemberKind::Rule) else {
            return;
        };
        let paths = match yaml {
            None => None,
            Some(yaml) => {
                match yaml_serde::from_str::<crate::validate::rules::RuleFrontmatter>(&yaml) {
                    Ok(parsed) => parsed.paths,
                    Err(_) => {
                        let name = self.relative(path);
                        self.unmeasured(MemberKind::Rule, name, UnmeasuredReason::Unreadable);
                        return;
                    }
                }
            }
        };
        if !path_globs::declares_scope(paths.as_ref()) {
            self.load_memory(path, MemberKind::Rule, &body, 0);
        }
    }

    fn memory_file(&mut self, path: &Path, kind: MemberKind, depth: usize) {
        if let Some((_, body)) = self.split(path, kind) {
            self.load_memory(path, kind, &body, depth);
        }
    }

    fn load_memory(&mut self, path: &Path, kind: MemberKind, body: &str, depth: usize) {
        if !self.seen.insert(path.to_path_buf()) || self.excludes.matches(path) {
            return;
        }
        let read = read_memory(body);
        self.count(kind, path, read.text.trim());
        if depth == MAX_IMPORT_HOPS {
            return;
        }
        let from = path.parent().unwrap_or(self.root);
        for written in read.imports {
            self.import(from, &written, depth + 1);
        }
    }

    fn import(&mut self, from: &Path, written: &str, depth: usize) {
        let Some(target) = import_target(written) else {
            return;
        };
        if target.starts_with("~/") {
            self.unmeasured(
                MemberKind::Import,
                written.to_string(),
                UnmeasuredReason::OutsideProject,
            );
            return;
        }
        let Some(path) = normalize(&from.join(&target)) else {
            return;
        };
        if !path.starts_with(self.root) {
            self.unmeasured(
                MemberKind::Import,
                written.to_string(),
                UnmeasuredReason::OutsideProject,
            );
            return;
        }
        if !path.is_file() || !has_text_extension(&path) {
            return;
        }
        self.memory_file(&path, MemberKind::Import, depth);
    }

    /// A style file that cannot be read may be the selected one, so it turns
    /// a style the project does not seem to ship into one it cannot measure.
    fn output_style(&mut self, name: &str) -> Result<()> {
        let mut by_stem = None;
        let mut any_unreadable = false;
        for path in discover(self.root, <OutputStyleValidator as SurfaceValidator>::GLOB)? {
            let Ok((yaml, body)) = split(&path) else {
                any_unreadable = true;
                continue;
            };
            let named = yaml
                .as_deref()
                .and_then(|yaml| parse_mapping(yaml).ok())
                .and_then(|mapping| mapping.get("name")?.as_str().map(str::to_string));
            if named.as_deref() == Some(name) {
                self.count(MemberKind::OutputStyle, &path, body.trim());
                return Ok(());
            }
            if by_stem.is_none() && path.file_stem().is_some_and(|stem| stem == name) {
                by_stem = Some((path, body));
            }
        }
        let reason = match by_stem {
            Some((path, body)) => {
                self.count(MemberKind::OutputStyle, &path, body.trim());
                return Ok(());
            }
            None if any_unreadable => UnmeasuredReason::Unreadable,
            None if name.eq_ignore_ascii_case("default") => return Ok(()),
            None => UnmeasuredReason::NotInProject,
        };
        self.unmeasured(MemberKind::OutputStyle, name.to_string(), reason);
        Ok(())
    }

    fn listing(&mut self, path: &Path, kind: MemberKind) {
        let Some((yaml, body)) = self.split(path, kind) else {
            return;
        };
        let mapping = match yaml.as_deref().map(parse_mapping) {
            None => yaml_serde::Mapping::new(),
            Some(Ok(mapping)) => mapping,
            Some(Err(())) => {
                let name = self.relative(path);
                self.unmeasured(kind, name, UnmeasuredReason::Unreadable);
                return;
            }
        };
        let text = |key: &str| {
            mapping
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let entry = if kind == MemberKind::Agent {
            if text("name").is_none() {
                return;
            }
            let Some(description) = text("description") else {
                return;
            };
            description
        } else {
            if mapping
                .get("disable-model-invocation")
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                return;
            }
            let description = text("description").unwrap_or_else(|| fallback_description(&body));
            let entry = match text("when_to_use") {
                Some(when) => format!("{description} - {when}"),
                None => description,
            };
            cap(&entry, LISTING_ENTRY_CAP, "\u{2026}")
        };
        self.count(kind, path, &entry);
    }

    /// [`split`], with a file that cannot be read recorded as unmeasured.
    fn split(&mut self, path: &Path, kind: MemberKind) -> Option<(Option<String>, String)> {
        let split = split(path).ok();
        if split.is_none() {
            let name = self.relative(path);
            self.unmeasured(kind, name, UnmeasuredReason::Unreadable);
        }
        split
    }
}

/// A file's frontmatter text and its body, or `Err` for a file that cannot be
/// read or whose frontmatter is not closed.
fn split(path: &Path) -> std::result::Result<(Option<String>, String), ()> {
    let content = std::fs::read_to_string(path).map_err(|_| ())?;
    match frontmatter::parse(&content, path).map_err(|_| ())? {
        None => Ok((None, content)),
        Some(fm) => {
            let body = content
                .split_inclusive('\n')
                .skip(fm.end_line as usize)
                .collect();
            Ok((Some(fm.yaml_text), body))
        }
    }
}

/// A memory file's text as the runtime injects it, and the imports it names
/// as written.
struct MemoryText {
    text: String,
    imports: Vec<String>,
}

fn read_memory(body: &str) -> MemoryText {
    let mut text = String::with_capacity(body.len());
    let mut copied = 0usize;
    let mut containers = 0usize;
    let mut in_code = false;
    let mut run = String::new();
    let mut imports = Vec::new();
    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        match &event {
            Event::Text(chunk) if !in_code => {
                run.push_str(chunk);
                continue;
            }
            Event::SoftBreak => {
                run.push('\n');
                continue;
            }
            _ => {}
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
            Event::InlineHtml(html) => {
                if let Some(rest) = without_comments(&html) {
                    scan_imports(&rest, &mut imports);
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
            if end > start {
                out.push(run[start..end].to_string());
            }
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
fn normalize(path: &Path) -> Option<PathBuf> {
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

/// The body's first non-empty line, a heading's text in place of the line.
fn fallback_description(body: &str) -> String {
    let Some(line) = body.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return "Custom item".to_string();
    };
    let heading = line
        .strip_prefix('#')
        .map(|rest| rest.trim_start_matches('#'))
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim)
        .filter(|text| !text.is_empty());
    cap(heading.unwrap_or(line), FALLBACK_DESCRIPTION_CAP, "...")
}

/// `text` cut to `limit` characters, `marker` ending the cut.
fn cap(text: &str, limit: usize, marker: &str) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let keep = limit - marker.chars().count();
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(marker);
    out
}

fn parse_mapping(yaml: &str) -> std::result::Result<yaml_serde::Mapping, ()> {
    if yaml.trim().is_empty() {
        return Ok(yaml_serde::Mapping::new());
    }
    yaml_serde::from_str(yaml).map_err(|_| ())
}

fn present(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.is_file())
        .collect()
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

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
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

    #[test]
    fn the_counted_text_is_the_text_the_runtime_injects() {
        // A removed comment block takes the line breaks after it, and the
        // whole text is trimmed — what 2.1.283 sent for this file.
        let set = loaded(&[("CLAUDE.md", "\nfirst\n\n<!-- note -->\n\nsecond\n\n")]);
        assert_eq!(chars_of(&set, "CLAUDE.md"), "first\n\nsecond".len());
    }

    #[test]
    fn a_member_counts_characters_not_bytes() {
        let set = loaded(&[("CLAUDE.md", "한국어 문장\n")]);
        assert_eq!(set.total_chars, "한국어 문장".chars().count());
    }

    #[test]
    fn frontmatter_and_top_level_comment_blocks_leave_the_text() {
        let text = read_memory(
            "a\n\n<!-- gone -->\n\n<!--\nalso gone\n-->\n\n<!-- gone --> kept\n\nb <!-- inline stays -->\n\n- item\n\n  <!-- listed stays -->\n",
        )
        .text;
        assert!(!text.contains("gone"), "{text:?}");
        assert!(text.contains(" kept"), "{text:?}");
        assert!(text.contains("inline stays"), "{text:?}");
        assert!(text.contains("listed stays"), "{text:?}");
        let set = loaded(&[("CLAUDE.md", "---\ntitle: x\n---\nbody\n")]);
        assert_eq!(chars_of(&set, "CLAUDE.md"), "body".len());
    }

    #[test]
    fn an_import_is_a_token_the_runtime_reads_as_one() {
        let set = loaded(&[
            (
                "CLAUDE.md",
                "@docs/start.md\n\nsee @docs/space.md here\n\n# @docs/heading.md\n\n\
                 See @docs/dot.md. later\n\n`@docs/span.md`\n\n```\n@docs/fence.md\n```\n\n    \
                 @docs/indented.md\n\nmail a@docs/mail.md\n\n(@docs/paren.md)\n\n\
                 <!-- @docs/comment.md -->\n\n@docs/sp\\ ace.md\n\n@docs/missing.md\n\n@docs\n",
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
        ]);
        assert_eq!(
            paths(&set),
            [
                "CLAUDE.md",
                "docs/heading.md",
                "docs/sp ace.md",
                "docs/space.md",
                "docs/start.md"
            ]
        );
    }

    #[test]
    fn an_import_loads_a_text_extension_or_none() {
        let set = loaded(&[
            (
                "CLAUDE.md",
                "@a.MD\n\n@Makefile\n\n@a.toml\n\n@a.weirdext\n\n@a.png\n\n@a.md,\n",
            ),
            ("a.MD", "x"),
            ("Makefile", "x"),
            ("a.toml", "x"),
            ("a.weirdext", "x"),
            ("a.png", "x"),
            ("a.md,", "x"),
        ]);
        assert_eq!(paths(&set), ["CLAUDE.md", "Makefile", "a.MD", "a.toml"]);
    }

    #[test]
    fn imports_reach_four_hops_and_load_a_file_once() {
        let set = loaded(&[
            ("CLAUDE.md", "@d/1.md\n\n@d/shared.md\n"),
            (".claude/CLAUDE.md", "@../d/shared.md\n"),
            ("d/1.md", "@2.md\n"),
            ("d/2.md", "@3.md\n"),
            ("d/3.md", "@4.md\n"),
            ("d/4.md", "@5.md\n"),
            ("d/5.md", "five\n"),
            ("d/shared.md", "shared\n"),
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
            ("docs/r.md", "imported\n"),
        ]);
        assert_eq!(
            paths(&set),
            [
                ".claude/rules/plain.md",
                ".claude/rules/star.md",
                ".claude/rules/sub/nested.md",
                "docs/r.md"
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
            ("CLAUDE.md", "@docs/rel.md\n\n@docs/star.md\n"),
            ("docs/rel.md", "x"),
            ("docs/star.md", "x"),
            (".claude/rules/abs.md", "x"),
        ]);
        let absolute = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            serde_json::json!({ "claudeMdExcludes": [
                "docs/rel.md",
                "**/docs/star.md",
                format!("{}/.claude/rules/*.md", absolute.display()),
            ]})
            .to_string(),
        )
        .unwrap();
        let set = resolve(dir.path()).unwrap();
        assert_eq!(paths(&set), ["CLAUDE.md", "docs/rel.md"]);
    }

    #[test]
    fn an_import_outside_the_project_is_named_and_not_counted() {
        let set = loaded(&[("CLAUDE.md", "@~/.claude/mine.md\n\n@../../elsewhere.md\n")]);
        assert_eq!(paths(&set), ["CLAUDE.md"]);
        let names: Vec<(&str, UnmeasuredReason)> = set
            .unmeasured
            .iter()
            .map(|u| (u.name.as_str(), u.reason))
            .collect();
        assert_eq!(
            names,
            [
                ("../../elsewhere.md", UnmeasuredReason::OutsideProject),
                ("~/.claude/mine.md", UnmeasuredReason::OutsideProject)
            ]
        );
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
            (".claude/output-styles/Terse.md", "by file name\n"),
        ]);
        assert_eq!(paths(&by_stem), [".claude/output-styles/Terse.md"]);

        let local_wins = loaded(&[
            (".claude/settings.json", r#"{"outputStyle": "Terse"}"#),
            (
                ".claude/settings.local.json",
                r#"{"outputStyle": "Explanatory"}"#,
            ),
            (".claude/output-styles/Terse.md", "by file name\n"),
        ]);
        assert!(local_wins.members.is_empty());
        assert_eq!(
            local_wins.unmeasured[0].reason,
            UnmeasuredReason::NotInProject
        );
    }

    #[test]
    fn a_listing_entry_counts_the_text_the_runtime_lists() {
        let long = "x".repeat(2000);
        let set = loaded(&[
            (
                ".claude/skills/both/SKILL.md",
                "---\nname: both\ndescription: Deploy\nwhen_to_use: on release\n---\nbody\n",
            ),
            (
                ".claude/skills/hidden/SKILL.md",
                "---\nname: hidden\ndescription: x\ndisable-model-invocation: true\n---\nbody\n",
            ),
            (
                ".claude/skills/bare/SKILL.md",
                "---\nname: bare\n---\n\n# Heading text\n\nmore\n",
            ),
            (
                ".claude/skills/long/SKILL.md",
                &format!("---\nname: long\ndescription: {long}\n---\nbody\n"),
            ),
            (".claude/commands/sub/run.md", "first line\nsecond\n"),
            (
                ".claude/agents/named.md",
                "---\nname: named\ndescription: Reviews\n---\nbody\n",
            ),
            (
                ".claude/agents/nameless.md",
                "---\ndescription: Unlisted\n---\nbody\n",
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
            chars_of(&set, ".claude/skills/long/SKILL.md"),
            LISTING_ENTRY_CAP
        );
        assert_eq!(
            chars_of(&set, ".claude/commands/sub/run.md"),
            "first line".len()
        );
        assert_eq!(chars_of(&set, ".claude/agents/named.md"), "Reviews".len());
        assert!(!paths(&set).contains(&".claude/skills/hidden/SKILL.md"));
        assert!(!paths(&set).contains(&".claude/agents/nameless.md"));
    }

    #[test]
    fn a_description_from_the_body_is_its_first_line_cut_to_a_hundred() {
        assert_eq!(fallback_description("\n\n## Title\nnext"), "Title");
        assert_eq!(fallback_description("#tag line"), "#tag line");
        assert_eq!(fallback_description(""), "Custom item");
        let cut = fallback_description(&"y".repeat(150));
        assert_eq!(cut.chars().count(), FALLBACK_DESCRIPTION_CAP);
        assert!(cut.ends_with("..."));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_unmeasured_rather_than_guessed() {
        let set = loaded(&[
            (
                ".claude/rules/broken.md",
                "---\npaths: [unclosed\n---\nbody\n",
            ),
            (".claude/skills/open/SKILL.md", "---\nname: open\n"),
        ]);
        assert!(set.members.is_empty(), "{:?}", set.members);
        let reasons: Vec<UnmeasuredReason> = set.unmeasured.iter().map(|u| u.reason).collect();
        assert_eq!(
            reasons,
            [UnmeasuredReason::Unreadable, UnmeasuredReason::Unreadable]
        );
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
