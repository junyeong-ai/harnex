//! Validator for `.claude/routines/*.md`.
//!
//! Shape only: the closed frontmatter grammar `harness_core::routines`
//! owns. Where a routine STANDS — produced, overdue, unscheduled — is the
//! query's answer, deliberately not a finding: a gate whose result moves
//! with the clock alone gives a different verdict every day over an
//! unchanged tree.
//!
//! ## What this module refuses to do
//!
//! - Never read the calendar. Shape is the only question here.
//! - Never modify input files.

use std::path::Path;

use crate::config::RoutinesPolicy;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::routines::RoutineDecl;

pub struct RoutineValidator;

impl RoutineValidator {
    pub fn new(_policy: &RoutinesPolicy) -> Self {
        Self
    }

    pub fn validate_file(&self, path: &Path) -> Result<Vec<Finding>> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(self.validate_text(&contents, path))
    }

    pub fn validate_text(&self, content: &str, path: &Path) -> Vec<Finding> {
        match RoutineDecl::from_file(content, path) {
            Ok(_) => Vec::new(),
            Err(e) => vec![Finding {
                slug: "routine-invalid".into(),
                severity: Severity::Major,
                location: Location::line(path.to_path_buf(), 1),
                message: e.to_string(),
                hint: Some(
                    "a routine declares `cadence`, `owner`, `prompt`, and — once scheduled — \
                     `when` (YYYY-MM-DD) and `produces`; `status: superseded` retires it in \
                     place"
                        .into(),
                ),
                auto_fixable: false,
                fix_command: None,
            }],
        }
    }
}

impl<'p> crate::validate::SurfaceValidator<'p> for RoutineValidator {
    type Policy = RoutinesPolicy;
    const SLUG: &'static str = "validate.routines";
    const GLOB: &'static str = ".claude/routines/*.md";

    fn policy(config: &'p crate::config::Config) -> Option<&'p Self::Policy> {
        config.validate.as_ref()?.routines.as_ref()
    }

    fn build(policy: &'p Self::Policy) -> Self {
        Self::new(policy)
    }

    fn validate_path(&self, path: &Path) -> Result<Vec<Finding>> {
        self.validate_file(path)
    }
}
