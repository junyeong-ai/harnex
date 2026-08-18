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
pub trait SurfaceValidator<'p>: Sized {
    /// The `[validate.<section>]` policy that enables this validator.
    type Policy: 'p;

    /// Slug reported in `run` / `skipped` and documented in `check.md`.
    const SLUG: &'static str;

    /// Glob, relative to the project root, of the files this validator covers.
    const GLOB: &'static str;

    fn policy(config: &'p Config) -> Option<&'p Self::Policy>;

    fn build(policy: &'p Self::Policy) -> Self;

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>>;
}

pub use agents::{AgentValidator, KNOWN_AGENT_KEYS};
pub use commit_msg::CommitMsgValidator;
pub use output_styles::{KNOWN_OUTPUT_STYLE_KEYS, OutputStyleValidator};
pub use rules::RuleValidator;
pub use settings::{
    KNOWN_DEFAULT_MODE_VALUES, KNOWN_HOOK_EVENTS, KNOWN_PROJECT_SCOPE_NOOP_KEYS,
    KNOWN_SESSION_START_SOURCES, KNOWN_SKILL_OVERRIDE_VALUES, SettingsScope, SettingsValidator,
};
pub use skills::{KNOWN_SKILL_KEYS, SkillValidator};
