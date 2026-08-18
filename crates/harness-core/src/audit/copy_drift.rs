//! Byte-drift auditor for `copy` scaffold artifacts.
//!
//! `copy` means the template is the only statement of what the artifact does,
//! so the manifest calls any difference a defect. Nothing enforced that in a
//! target project: the managed-region auditor judges what sentinels bound, and
//! a `copy` artifact carries none.
//!
//! What that let through is worse than an edited script. A scaffold keeps a
//! file already at a `copy` destination — the project owns it — and still
//! merges the hook fragments that reference it, because ownership is decided
//! per artifact while the wiring lives in another one. A repository with its
//! own `hooks/_runner.sh` therefore ends up with two Claude Code events
//! dispatching to a script harnex did not write, passing harnex's verifier
//! names as its first argument, and every other check reports the harness
//! clean. Comparing bytes is what makes that visible, and it is exact — no
//! threshold, no pattern, no inference about what the difference means.
//!
//! ## What this module refuses to do
//!
//! - Never judge a destination the manifest does not declare `copy`. `seed` is
//!   handed to the project outright and `managed` owns only its sentinels, so
//!   a difference in either is the intended use rather than drift.
//! - Never judge an absent file. Absence is coverage's answer, and a hook
//!   pointing at one is `hook-wiring`'s; reporting it here would say the same
//!   thing a third time with a worse name.
//! - Never repair, and never rank the reasons. Three states produce the same
//!   bytes — the project's own file kept by the collision rule, an edit to
//!   harnex's copy, and a harness generated at an older plugin version — and
//!   the file cannot tell them apart. That is why the finding is `Minor`
//!   rather than gating: the collision rule instructs the scaffold to keep an
//!   incumbent, so a gating finding would make the manifest contradict itself
//!   and leave a correct project permanently red with no suppression. The
//!   message names all three and the operator picks.
//! - Never read a line ending as a difference. Comparison goes through
//!   [`crate::audit::normalize`], the same helper the managed-region auditor
//!   uses, so a Windows checkout does not report every shell hook as drifted.

use std::path::Path;

use crate::audit::{AuditFindingSlug, normalize};
use crate::envelope::{Finding, Location, Severity};
use crate::error::Result;
use crate::scaffold::{Content, ScaffoldManifest};

pub(crate) struct CopyDriftAuditor;

impl CopyDriftAuditor {
    pub(crate) fn audit(&self, project_root: &Path, plugin_root: &Path) -> Result<Vec<Finding>> {
        let templates = plugin_root.join("templates");
        let manifest = ScaffoldManifest::load(&templates)?;
        let mut findings = Vec::new();

        for artifact in manifest.artifacts() {
            if artifact.content != Content::Copy {
                continue;
            }
            // Paired, not crossed: the destination carries the language, so the
            // template that emits it is known exactly. Matching any shipped
            // template would call a Rust project holding the Python formatter
            // undrifted.
            for (template, destination) in artifact.resolved_pairs() {
                let landed = project_root.join(&destination);
                let Ok(body) = std::fs::read_to_string(&landed) else {
                    continue;
                };
                let Ok(canonical) = std::fs::read_to_string(templates.join(&template)) else {
                    continue;
                };
                if normalize(&body) == normalize(&canonical) {
                    continue;
                }
                findings.push(Finding {
                    slug: AuditFindingSlug::CopyDrift.as_str().into(),
                    severity: Severity::Minor,
                    location: Location::file(landed),
                    message: format!(
                        "'{}' is not what '{template}' says it should be. Three ways to reach \
                         this and the file cannot tell them apart: the project already had its \
                         own file there and the scaffold kept it, someone edited harnex's copy, \
                         or the harness was generated at an older plugin version",
                        destination.display()
                    ),
                    hint: Some(format!(
                        "re-copy '{template}' to take harnex's version; keep yours and move it \
                         to a path the scaffold does not claim if the hooks should reach your \
                         own script; or ignore this if the difference is a version you have not \
                         regenerated to yet"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        Ok(findings)
    }
}
