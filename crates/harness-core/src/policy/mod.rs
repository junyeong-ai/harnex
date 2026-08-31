//! # policy — permission rules, profiles + version pins
//!
//! Three surfaces:
//! - [`rule`] owns the permission rule grammar and answers the one question
//!   every other surface asks of a rule string: does a permission check read
//!   it, or does Claude Code accept it and never consult it.
//! - [`permissions`] composes canonical deny/ask/allow rules for
//!   `.claude/settings.json` from named built-in profiles plus
//!   project-local extras.
//! - [`versions`] checks declared tool pins against installed versions
//!   under four strategies (`exact` / `minor` / `major` / `rolling`).
//!
//! ## What this module refuses to do
//!
//! - Never modify `.claude/settings.json` directly. Generators emit JSON;
//!   the caller writes it.
//! - Never spawn arbitrary tool `--version` subprocesses. Callers run
//!   the tool and pipe its version string to `VersionChecker::check_installed`.

pub mod permissions;
pub mod profiles;
pub mod rule;
pub mod versions;

pub use permissions::{
    PermissionAuditor, PermissionFinding, PermissionFindingKind, PermissionGenerator,
    PermissionsBlock,
};
pub use profiles::PermissionProfile;
pub use rule::{
    InertReason, InertRule, MisleadingReason, MisleadingRule, PermissionRule, RuleDirection,
    RuleEffect,
};
pub use versions::{VersionCheckOutcome, VersionChecker};
