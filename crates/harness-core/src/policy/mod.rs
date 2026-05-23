//! # policy — permission profiles + version pins
//!
//! Two surfaces:
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
pub mod versions;

pub use permissions::{
    PermissionAuditor, PermissionFinding, PermissionFindingKind, PermissionGenerator,
    PermissionsBlock,
};
pub use profiles::PermissionProfile;
pub use versions::{VersionChecker, VersionCheckOutcome};
