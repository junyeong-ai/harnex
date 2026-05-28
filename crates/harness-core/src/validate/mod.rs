//! # validate — frontmatter and structural checks for Claude Code surfaces
//!
//! Four sub-validators scoped to Claude Code + git surfaces:
//! - [`rules`] — `.claude/rules/*.md`: `paths:` frontmatter + max-lines.
//! - [`skills`] — `.claude/skills/*/SKILL.md`: full SKILL.md frontmatter
//!   contract per <https://code.claude.com/docs/en/skills>.
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

pub mod commit_msg;
pub mod frontmatter;
pub mod rules;
pub mod settings;
pub mod skills;

pub use commit_msg::CommitMsgValidator;
pub use rules::RuleValidator;
pub use settings::{
    KNOWN_DEFAULT_MODE_VALUES, KNOWN_HOOK_EVENTS, KNOWN_PROJECT_SCOPE_NOOP_KEYS,
    KNOWN_SKILL_OVERRIDE_VALUES, SettingsScope, SettingsValidator,
};
pub use skills::{KNOWN_SKILL_KEYS, SkillValidator};
