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
//! - Never repair. The finding names the template to copy across; which side
//!   is right is the operator's call, because a deliberate fork of a runner is
//!   a decision this auditor cannot see the reason for.

use std::path::Path;

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
                if body == canonical {
                    continue;
                }
                findings.push(Finding {
                    slug: "audit-copy-drift".into(),
                    severity: Severity::Major,
                    location: Location::file(landed),
                    message: format!(
                        "'{}' differs from the template that emits it ('{template}'), and the \
                         manifest declares this artifact byte-identical — whatever is wired at \
                         that path is not what harnex generated",
                        destination.display()
                    ),
                    hint: Some(format!(
                        "re-copy '{template}' over it, or move the project's own version to a \
                         path the scaffold does not claim and repoint the hooks that name it"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        Ok(findings)
    }
}
