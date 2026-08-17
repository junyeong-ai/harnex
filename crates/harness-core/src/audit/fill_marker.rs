//! Unresolved fill-marker auditor.
//!
//! A template ships gaps its generating step is meant to close with a project
//! observation. `SKILL.md` states the rule outright — a placeholder that ships
//! is the blank-page problem in disguise — and until this auditor existed the
//! rule had no enforcer, which is precisely the enforced-vs-advisory mistake
//! harnex is built to stop a project from making.
//!
//! What makes the check exact is that the marker is harnex's own reserved
//! token (`sentinel::fill_markers`). Finding one is a fact about a file harnex
//! wrote, not a guess about prose: a scan for `<PROJECT_NAME>`-shaped text or
//! angle brackets would flag a generic parameter, an HTML snippet, or a
//! placeholder a project authored for itself.
//!
//! ## What this module refuses to do
//!
//! - Never guess which value belongs there. The finding carries what the
//!   template asked for and stops; filling it needs the project, and a
//!   default invented here would be the free-generation the templates exist
//!   to prevent.
//! - Never read outside the harness surface. A marker in the project's own
//!   source is the project's business; only `CLAUDE.md` and `.claude/` are
//!   places harnex writes.
//! - Never rank one marker above another. Every one is the same defect —
//!   a file that reads finished and is not.

use std::path::{Path, PathBuf};

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::sentinel;

/// Where harnex writes markdown. A marker anywhere else belongs to the
/// project, and the auditor has no standing to judge it.
const HARNESS_SURFACE: &[&str] = &["CLAUDE.md", ".claude/**/*.md"];

pub(crate) struct FillMarkerOutcome {
    pub findings: Vec<Finding>,
    pub files_scanned: usize,
}

pub(crate) struct FillMarkerAuditor;

impl FillMarkerAuditor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn audit(&self, project_root: &Path) -> Result<FillMarkerOutcome> {
        let mut findings = Vec::new();
        let mut files_scanned = 0usize;

        for path in surface_files(project_root)? {
            let Ok(body) = std::fs::read_to_string(&path) else {
                continue;
            };
            files_scanned += 1;
            for marker in sentinel::fill_markers(&body) {
                findings.push(Finding {
                    slug: "audit-fill-marker-unresolved".into(),
                    severity: Severity::Major,
                    location: Location::line(path.clone(), marker.line as u32),
                    message: format!(
                        "unresolved fill marker: the template asked for {} and the generated \
                         file still carries the placeholder",
                        marker.wanted
                    ),
                    hint: Some(format!(
                        "replace the marker with what this project actually does, or with an \
                         explicit \"none observed yet\" note — {} is what it was asked to record",
                        marker.wanted
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        Ok(FillMarkerOutcome {
            findings,
            files_scanned,
        })
    }
}

fn surface_files(project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for pattern in HARNESS_SURFACE {
        let rooted = crate::glob_root::rooted(project_root, pattern)?;
        let entries = glob::glob(&rooted).map_err(|e| Error::IoFailure {
            path: project_root.join(pattern),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("glob: {e}")),
        })?;
        for entry in entries {
            // A traversal error is not "no marker here" — surfacing it keeps a
            // permission-denied directory from reading as a clean file.
            let path = entry.map_err(|e| Error::IoFailure {
                path: e.path().to_path_buf(),
                source: e.into(),
            })?;
            if path.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn flags_a_marker_the_generating_step_left_behind() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "CLAUDE.md",
            "# <!-- harnex-fill: the project name -->\n\nbody\n",
        );
        let outcome = FillMarkerAuditor::new().audit(dir.path()).unwrap();
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(outcome.findings[0].slug, "audit-fill-marker-unresolved");
        assert!(
            outcome.findings[0].message.contains("the project name"),
            "the finding must carry what was asked for: {}",
            outcome.findings[0].message
        );
        assert_eq!(outcome.findings[0].location.line, Some(1));
    }

    #[test]
    fn a_finished_harness_is_silent() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "CLAUDE.md", "# realproject\n\nreal content\n");
        write(
            dir.path(),
            ".claude/rules/python-conventions.md",
            "# Python\n\nType checker: ty, in the gate.\n",
        );
        let outcome = FillMarkerAuditor::new().audit(dir.path()).unwrap();
        assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
        assert_eq!(outcome.files_scanned, 2);
    }

    #[test]
    fn prose_that_merely_looks_like_a_placeholder_is_not_one() {
        // The reason the marker is a reserved token. Every line below is
        // ordinary content in a real rule file, and a shape-matching scan
        // would flag all of them.
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            ".claude/rules/typescript-conventions.md",
            "Use `Array<T>` over `T[]`.\n\
             Set `<PROJECT_NAME>` in the deploy manifest.\n\
             Observed: <none yet>\n\
             <!-- a plain comment -->\n\
             Render `<Button />` from the design system.\n",
        );
        let outcome = FillMarkerAuditor::new().audit(dir.path()).unwrap();
        assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
    }

    #[test]
    fn a_marker_in_the_projects_own_source_is_not_this_auditors_business() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "src/main.rs", "// <!-- harnex-fill: nope -->\n");
        write(dir.path(), "docs/guide.md", "<!-- harnex-fill: nope -->\n");
        let outcome = FillMarkerAuditor::new().audit(dir.path()).unwrap();
        assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
        assert_eq!(outcome.files_scanned, 0);
    }

    #[test]
    fn a_project_path_carrying_glob_syntax_is_still_scanned() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("repo [backup]");
        write(&root, "CLAUDE.md", "<!-- harnex-fill: the name -->\n");
        let outcome = FillMarkerAuditor::new().audit(&root).unwrap();
        assert_eq!(outcome.findings.len(), 1);
    }
}
