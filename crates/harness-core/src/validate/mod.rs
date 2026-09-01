//! # validate — frontmatter and structural checks for Claude Code surfaces
//!
//! Six sub-validators scoped to Claude Code + git surfaces:
//! - [`rules`] — `.claude/rules/*.md`: `paths:` frontmatter + line budget
//!   keyed on load scope.
//! - [`skills`] — `.claude/skills/*/SKILL.md`: full SKILL.md frontmatter
//!   contract per <https://code.claude.com/docs/en/skills>.
//! - [`agents`] — `.claude/agents/*.md`: subagent frontmatter contract per
//!   <https://code.claude.com/docs/en/sub-agents>.
//! - [`output_styles`] — `.claude/output-styles/*.md`: the two booleans that
//!   decide what a style does to the system prompt.
//! - [`settings`] — `.claude/settings.json`: hook event name typo
//!   detection (per /en/hooks), permission tier shape, project-scope
//!   no-op key detection, `defaultMode` closed-enum check.
//! - [`commit_msg`] — git commit messages: closed-enum trailer values
//!   and required-trailer presence per `[validate.commit_msg]` config.
//!
//! ## What this module refuses to do
//!
//! - Never read rule / skill / commit BODY semantics. Frontmatter +
//!   structural only (commit_msg checks trailers, not the message body).
//! - Never modify input files. Findings only — fixing is callers' job.

pub mod agents;
pub mod commit_msg;
pub mod frontmatter;
pub mod output_styles;
pub mod routines;
pub mod rules;
pub mod settings;
pub mod skills;

use std::path::Path;

use crate::config::Config;
use crate::envelope::Finding;
use crate::error::Result;

/// A validator over one glob of Claude Code surface files.
///
/// Every such validator answers the same four questions — which config
/// section enables it, which files it covers, what slug it reports under,
/// and how it reads one file — so [`check`](crate::check) drives them
/// through this trait instead of carrying a near-identical method per
/// artifact class. Adding an artifact class is then an impl plus one line
/// in the gate, and the gate's skipped-vs-ran contract cannot diverge
/// between classes because there is only one copy of it.
/// What a [`SurfaceValidator::GLOB`] means, as [`glob::glob`] reads it.
///
/// `glob::glob` forces `require_literal_separator` on regardless of what it is
/// handed; the other two are its defaults. Stated once so the discovery walk
/// and [`SurfaceValidator::covers`] cannot answer differently.
const GLOB_MATCH: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

pub trait SurfaceValidator<'p>: Sized {
    /// The `[validate.<section>]` policy that enables this validator.
    type Policy: 'p;

    /// Slug reported in `run` / `skipped` and documented in `check.md`.
    const SLUG: &'static str;

    /// Glob, relative to the project root, of the files this validator covers.
    ///
    /// Read through [`Self::covers`] rather than matched directly — the string
    /// alone does not carry the semantics discovery gives it.
    const GLOB: &'static str;

    /// Whether a project-relative path is one [`Self::GLOB`] discovers.
    ///
    /// Discovery is [`glob::glob`] over the project tree, which walks directory
    /// by directory: a `*` never crosses a separator and `**` is what spans
    /// depth. [`glob::Pattern::matches`] defaults the opposite way, so matching
    /// `GLOB` with it reports coverage of paths no validator ever reads —
    /// `.claude/skills/*/SKILL.md` claiming `.claude/skills/a/b/SKILL.md`,
    /// which Claude Code loads no skill from. The options are pinned here so
    /// the meaning of a `GLOB` travels with its declaration.
    fn covers(path: &str) -> bool {
        glob::Pattern::new(Self::GLOB)
            .expect("SurfaceValidator::GLOB is a well-formed glob")
            .matches_with(path, GLOB_MATCH)
    }

    fn policy(config: &'p Config) -> Option<&'p Self::Policy>;

    fn build(policy: &'p Self::Policy) -> Self;

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>>;
}

pub use agents::{AgentValidator, KNOWN_AGENT_KEYS};
pub use commit_msg::CommitMsgValidator;
pub use output_styles::{KNOWN_OUTPUT_STYLE_KEYS, OutputStyleValidator};
pub use routines::RoutineValidator;
pub use rules::RuleValidator;
pub use settings::{
    KNOWN_DEFAULT_MODE_VALUES, KNOWN_HOOK_EVENTS, KNOWN_PROJECT_SCOPE_NOOP_KEYS,
    KNOWN_SESSION_START_SOURCES, KNOWN_SKILL_OVERRIDE_VALUES, SettingsScope, SettingsValidator,
};
pub use skills::{KNOWN_SKILL_KEYS, SkillValidator};

#[cfg(test)]
mod glob_tests {
    use super::*;

    fn agrees<'p, V: SurfaceValidator<'p>>(root: &Path, files: &[&str]) {
        let pattern = crate::glob_root::rooted(root, V::GLOB).expect("rooted pattern");
        let mut walked: Vec<String> = glob::glob(&pattern)
            .expect("glob pattern parses")
            .filter_map(std::result::Result::ok)
            .map(|p| {
                p.strip_prefix(root)
                    .expect("match lies under the root")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        walked.sort();
        let mut claimed: Vec<String> = files
            .iter()
            .filter(|f| V::covers(f))
            .map(|f| (*f).to_string())
            .collect();
        claimed.sort();
        assert_eq!(
            claimed,
            walked,
            "{}: covers() and the discovery walk read {} differently",
            V::SLUG,
            V::GLOB
        );
    }

    /// `covers` answers what the discovery walk finds, for every surface.
    ///
    /// The two read one `GLOB` through different machinery — a directory walk
    /// and a string match — and the string match defaults to letting a `*`
    /// cross a separator. The walk is the truth here because the walk is what
    /// `check` runs; this fails if a caller ever reintroduces that default.
    #[test]
    fn covers_agrees_with_the_discovery_walk() {
        let root = tempfile::tempdir().expect("tempdir");
        // Each surface gets a file its glob covers and one just past the depth
        // it covers, which is where the two readings part.
        let files = [
            ".claude/rules/flat.md",
            ".claude/rules/nested/deep.md",
            ".claude/agents/reviewer.md",
            ".claude/agents/team/reviewer.md",
            ".claude/skills/spec/SKILL.md",
            ".claude/skills/spec/gates.md",
            ".claude/skills/spec/sub/SKILL.md",
            ".claude/output-styles/terse.md",
            ".claude/output-styles/nested/terse.md",
            ".claude/routines/curate.md",
            ".claude/routines/nested/curate.md",
        ];
        for file in files {
            let path = root.path().join(file);
            std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create dirs");
            std::fs::write(&path, "---\n---\n").expect("write file");
        }
        agrees::<RuleValidator>(root.path(), &files);
        agrees::<AgentValidator>(root.path(), &files);
        agrees::<SkillValidator>(root.path(), &files);
        agrees::<OutputStyleValidator>(root.path(), &files);
        agrees::<RoutineValidator>(root.path(), &files);
    }
}
