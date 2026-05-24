//! Managed-region drift auditor.
//!
//! Generated markdown artifacts (`CLAUDE.md`, `.claude/rules/constitution.md`,
//! …) carry sentinel-delimited regions:
//!
//! ```text
//! <!-- harnex-managed:start <slug> -->
//! ... harnex-owned content ...
//! <!-- harnex-managed:end <slug> -->
//! ```
//!
//! The block between the sentinels mirrors a region of a plugin template.
//! Which template owns which project file is declared in
//! `templates/managed-files.toml` shipped with the plugin — Constitution
//! VII forbids encoding such project-domain paths in Rust source. This
//! auditor reads the manifest, then compares the project's bytes against
//! the template's for every declared pair.
//!
//! ## What this module refuses to do
//!
//! - Never own settings.json — that JSON file has no comment syntax for
//!   sentinels; its managed-region contract is by-top-level-key
//!   (`permissions`, `hooks` are managed), enforced in the scaffold /
//!   regenerate skill flow rather than here.
//! - Never write. Findings only — restoration is the regenerate flow's job.
//! - Never interpret prose. Bytes-equal-modulo-line-ending is the entire
//!   semantic; anything richer (semantic diff, paraphrase tolerance) is
//!   heuristic territory forbidden by `keep-soften-cut`.
//! - Never silently succeed on a missing manifest. The whole auditor fails
//!   loudly so a wrong `--plugin-root` cannot masquerade as a clean audit.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::sentinel;

const MANIFEST_FILENAME: &str = "managed-files.toml";

/// Parsed shape of `templates/managed-files.toml`.
#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    managed: Vec<ManagedFile>,
}

#[derive(Debug, Deserialize)]
struct ManagedFile {
    /// Relative path of the template within the templates directory.
    template: PathBuf,
    /// Relative path of the artifact within the target project.
    project: PathBuf,
}

/// Aggregated outcome of a managed-region audit pass.
#[derive(Debug)]
pub(crate) struct ManagedRegionOutcome {
    pub findings: Vec<Finding>,
    pub files_scanned: usize,
}

pub(crate) struct ManagedRegionAuditor<'a> {
    plugin_root: &'a Path,
}

impl<'a> ManagedRegionAuditor<'a> {
    /// `plugin_root` is the plugin directory containing the `templates/`
    /// subdirectory (e.g., `plugins/harnex` in this repo).
    pub(crate) fn new(plugin_root: &'a Path) -> Self {
        Self { plugin_root }
    }

    pub(crate) fn audit(&self, project_root: &Path) -> Result<ManagedRegionOutcome> {
        let templates_root = self.plugin_root.join("templates");
        let manifest_path = templates_root.join(MANIFEST_FILENAME);
        let manifest = load_manifest(&manifest_path)?;

        let mut findings = Vec::new();
        let mut files_scanned: usize = 0;
        for entry in &manifest.managed {
            let template_path = templates_root.join(&entry.template);
            if !template_path.is_file() {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "managed-files.toml lists '{}' but the template is missing at {}",
                        entry.template.display(),
                        template_path.display()
                    ),
                    location: Some(Location::file(manifest_path.clone())),
                });
            }
            let project_path = project_root.join(&entry.project);
            if !project_path.is_file() {
                continue;
            }
            files_scanned += 1;
            self.audit_one(&template_path, &project_path, &mut findings)?;
        }
        Ok(ManagedRegionOutcome {
            findings,
            files_scanned,
        })
    }

    fn audit_one(
        &self,
        template_path: &Path,
        project_path: &Path,
        findings: &mut Vec<Finding>,
    ) -> Result<()> {
        let project_content =
            std::fs::read_to_string(project_path).map_err(|e| Error::IoFailure {
                path: project_path.to_path_buf(),
                source: e,
            })?;
        let template_content =
            std::fs::read_to_string(template_path).map_err(|e| Error::IoFailure {
                path: template_path.to_path_buf(),
                source: e,
            })?;
        let project_regions = sentinel::extract_regions(&project_content);
        // A file with zero harnex-managed sentinels is fully project-authored.
        // Possibly never scaffolded by harnex. The "missing region" finding
        // only carries signal when SOME sentinels are present (partial drift).
        if project_regions.is_empty() {
            return Ok(());
        }
        let template_regions = sentinel::extract_regions(&template_content);
        for (slug, template_body) in &template_regions {
            let Some(project_body) = project_regions.get(slug) else {
                findings.push(Finding {
                    slug: "audit-managed-region-missing".into(),
                    severity: Severity::Major,
                    location: Location::file(project_path.to_path_buf()),
                    message: format!(
                        "managed region '{slug}' is missing from a file that carries other \
                         harnex-managed sentinels — partial drift"
                    ),
                    hint: Some(format!(
                        "run `harnex regenerate` or restore the sentinels:\n\
                         <!-- harnex-managed:start {slug} --> ... <!-- harnex-managed:end {slug} -->"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                continue;
            };
            if normalize(project_body) != normalize(template_body) {
                findings.push(Finding {
                    slug: "audit-managed-region-edited".into(),
                    severity: Severity::Info,
                    location: Location::file(project_path.to_path_buf()),
                    message: format!(
                        "managed region '{slug}' diverges from the canonical template — \
                         either the operator edited inside the sentinels or the template \
                         drifted upstream"
                    ),
                    hint: Some(format!(
                        "compare against {} and re-run `harnex regenerate` to restore",
                        template_path.display()
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        Ok(())
    }
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    let raw = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;
    let parsed: Manifest = toml::from_str(&raw).map_err(|e| Error::ConfigInvalid {
        message: format!("parse {MANIFEST_FILENAME}: {e}"),
        location: Some(Location::file(path.to_path_buf())),
    })?;
    if parsed.managed.is_empty() {
        return Err(Error::ConfigInvalid {
            message: format!(
                "{MANIFEST_FILENAME} has no [[managed]] entries — at least one is required"
            ),
            location: Some(Location::file(path.to_path_buf())),
        });
    }
    // Reject duplicate project paths — two entries pointing at the same file
    // would produce duplicate findings and mask which template is canonical.
    let mut seen = std::collections::BTreeSet::new();
    for entry in &parsed.managed {
        if !seen.insert(&entry.project) {
            return Err(Error::ConfigInvalid {
                message: format!(
                    "{MANIFEST_FILENAME} has duplicate project path: {}",
                    entry.project.display()
                ),
                location: Some(Location::file(path.to_path_buf())),
            });
        }
    }
    Ok(parsed)
}

/// Normalize a region body for comparison: trim, collapse internal CR/LF.
fn normalize(body: &str) -> String {
    body.replace("\r\n", "\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        p
    }

    const CANONICAL_CLAUDE_MD: &str = "\
intro
<!-- harnex-managed:start enforcement-summary -->
canonical body
<!-- harnex-managed:end enforcement-summary -->
outside
";

    /// Build a minimal plugin directory layout (templates/ + manifest) the
    /// auditor can read. Returns the plugin root.
    fn seed_plugin(plugin_root: &Path) {
        write(
            plugin_root,
            "templates/common/CLAUDE.md",
            CANONICAL_CLAUDE_MD,
        );
        write(
            plugin_root,
            "templates/common/constitution.md",
            "<!-- harnex-managed:start constitution-articles -->\nbody\n<!-- harnex-managed:end constitution-articles -->\n",
        );
        write(
            plugin_root,
            "templates/managed-files.toml",
            r#"
[[managed]]
template = "common/CLAUDE.md"
project = "CLAUDE.md"

[[managed]]
template = "common/constitution.md"
project = ".claude/rules/constitution.md"
"#,
        );
    }

    #[test]
    fn detects_edited_managed_region() {
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed_plugin(plugin.path());
        write(
            proj.path(),
            "CLAUDE.md",
            "intro\n<!-- harnex-managed:start enforcement-summary -->\nEDITED body\n<!-- harnex-managed:end enforcement-summary -->\nouts",
        );
        let outcome = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap();
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.slug == "audit-managed-region-edited"),
            "expected edited finding: {:?}",
            outcome.findings
        );
    }

    #[test]
    fn ignores_edits_outside_managed_region() {
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed_plugin(plugin.path());
        write(
            proj.path(),
            "CLAUDE.md",
            "TOTALLY different intro\n<!-- harnex-managed:start enforcement-summary -->\ncanonical body\n<!-- harnex-managed:end enforcement-summary -->\nproject-authored notes here\n",
        );
        let outcome = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap();
        assert!(
            !outcome
                .findings
                .iter()
                .any(|f| f.slug == "audit-managed-region-edited"),
            "outside-sentinel edits must be ignored: {:?}",
            outcome.findings
        );
    }

    #[test]
    fn flags_missing_region_only_on_partial_drift() {
        // A file with SOME harnex-managed sentinels but missing one expected
        // by the template signals partial drift — fire the finding.
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed_plugin(plugin.path());
        write(
            proj.path(),
            "CLAUDE.md",
            "intro\n<!-- harnex-managed:start unrelated -->\nx\n<!-- harnex-managed:end unrelated -->\nouts",
        );
        let outcome = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap();
        assert!(
            outcome
                .findings
                .iter()
                .any(|f| f.slug == "audit-managed-region-missing"),
            "partial-drift must surface missing region: {:?}",
            outcome.findings
        );
    }

    #[test]
    fn silent_when_file_has_no_sentinels_at_all() {
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed_plugin(plugin.path());
        write(proj.path(), "CLAUDE.md", "hand-written, no sentinels\n");
        let outcome = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap();
        assert!(
            outcome.findings.is_empty(),
            "fully-project-authored file must produce no findings: {:?}",
            outcome.findings
        );
    }

    #[test]
    fn missing_manifest_is_a_hard_error_not_silent_zero() {
        // CRIT7: a wrong --plugin-root must NOT masquerade as a clean audit.
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        // No managed-files.toml written.
        let err = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap_err();
        assert!(
            matches!(err.code(), crate::error::ErrorCode::IoFailure),
            "expected IoFailure for missing manifest, got {:?}",
            err.code()
        );
    }

    #[test]
    fn empty_manifest_is_rejected() {
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        write(
            plugin.path(),
            "templates/managed-files.toml",
            "managed = []\n",
        );
        let err = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap_err();
        assert!(
            matches!(err.code(), crate::error::ErrorCode::ConfigInvalid),
            "empty manifest must error, got {:?}",
            err.code()
        );
    }

    #[test]
    fn manifest_referencing_missing_template_is_rejected() {
        let plugin = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        write(
            plugin.path(),
            "templates/managed-files.toml",
            r#"[[managed]]
template = "common/does-not-exist.md"
project = "x.md"
"#,
        );
        let err = ManagedRegionAuditor::new(plugin.path())
            .audit(proj.path())
            .unwrap_err();
        assert!(matches!(err.code(), crate::error::ErrorCode::ConfigInvalid));
    }
}
