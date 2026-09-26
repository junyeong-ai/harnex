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
//! - Never read outside the harness surface, or outside what the project
//!   owns. A marker in the project's own source is the project's business;
//!   only `CLAUDE.md` and `.claude/` are places harnex writes. Which files
//!   under those are this project's is [`crate::git::owned_files`]' answer,
//!   because `.claude/` is also where Claude Code puts a linked worktree:
//!   walking it would read another checkout's copy of the harness and report
//!   a placeholder in a file nobody here wrote.
//! - Never rank one marker above another. Every one is the same defect —
//!   a file that reads finished and is not.

use std::path::{Path, PathBuf};

use crate::audit::AuditFindingSlug;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::git;
use crate::sentinel;

/// Where harnex writes markdown, as git pathspecs. A marker anywhere else
/// belongs to the project, and the auditor has no standing to judge it.
/// `:(glob)` is the magic that makes `*` stop at a path separator and `**`
/// cross one, which is the reading the surface is written in.
const HARNESS_SURFACE: &[&str] = &[":(glob)CLAUDE.md", ":(glob).claude/**/*.md"];

#[derive(Debug)]
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
            let body = match std::fs::read_to_string(&path) {
                Ok(body) => body,
                // git lists a tracked file whether or not it is still on
                // disk, and one that is gone carries no marker. Any other
                // failure is a file left unread, which must not report as a
                // file read clean.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(Error::IoFailure { path, source }),
            };
            files_scanned += 1;
            for marker in sentinel::fill_markers(&body) {
                findings.push(Finding {
                    slug: AuditFindingSlug::FillMarkerUnresolved.as_str().into(),
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
    git::owned_files(project_root, HARNESS_SURFACE)
        .map_err(|git::Failure(message)| Error::AuditGitFailure { message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// A git command answering about `root` and nothing else: no global or
    /// system configuration, so a signing or hooks setting on this machine
    /// cannot reach the repository a test builds.
    fn git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }

    /// A repository holding no commit — the state a scaffold leaves behind,
    /// and the one every case below but the worktree runs in.
    fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        dir
    }

    #[test]
    fn flags_a_marker_the_generating_step_left_behind() {
        let dir = repo();
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
        let dir = repo();
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
        let dir = repo();
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
        let dir = repo();
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
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        write(&root, "CLAUDE.md", "<!-- harnex-fill: the name -->\n");
        let outcome = FillMarkerAuditor::new().audit(&root).unwrap();
        assert_eq!(outcome.findings.len(), 1);
    }

    #[test]
    fn a_marker_in_a_linked_worktree_belongs_to_that_checkout() {
        // Claude Code puts a worktree under `.claude/`, so the harness
        // surface glob reaches a second checkout of this same harness. Both
        // copies carry the marker here; only this project's is the finding.
        let dir = repo();
        let root = dir.path();
        write(
            root,
            "CLAUDE.md",
            "<!-- harnex-fill: the project name -->\n",
        );
        git(root, &["add", "-A"]);
        git(
            root,
            &[
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "initial",
            ],
        );
        git(
            root,
            &[
                "worktree",
                "add",
                "-q",
                ".claude/worktrees/agent",
                "-b",
                "agent",
            ],
        );

        let outcome = FillMarkerAuditor::new().audit(root).unwrap();
        assert_eq!(outcome.files_scanned, 1);
        assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
        assert_eq!(outcome.findings[0].location.path, root.join("CLAUDE.md"));
    }

    #[test]
    fn an_ignored_path_under_dot_claude_is_not_this_projects() {
        let dir = repo();
        let root = dir.path();
        write(root, ".gitignore", ".claude/scratch/\n");
        write(
            root,
            ".claude/scratch/notes.md",
            "<!-- harnex-fill: nope -->\n",
        );
        write(
            root,
            ".claude/rules/python-conventions.md",
            "<!-- harnex-fill: the type checker -->\n",
        );

        let outcome = FillMarkerAuditor::new().audit(root).unwrap();
        assert_eq!(outcome.files_scanned, 1);
        assert_eq!(outcome.findings.len(), 1, "{:?}", outcome.findings);
        assert!(
            outcome.findings[0].message.contains("the type checker"),
            "{}",
            outcome.findings[0].message
        );
    }

    #[test]
    fn a_harness_below_the_repository_root_answers_for_its_own_directory() {
        // A package carrying its own `harness.toml` audits from there, so the
        // pathspecs are read against that directory rather than the
        // repository root.
        let dir = repo();
        let root = dir.path();
        write(root, "CLAUDE.md", "<!-- harnex-fill: the repository -->\n");
        let package = root.join("packages/app");
        write(&package, "CLAUDE.md", "<!-- harnex-fill: the package -->\n");

        let outcome = FillMarkerAuditor::new().audit(&package).unwrap();
        assert_eq!(outcome.files_scanned, 1);
        assert!(
            outcome.findings[0].message.contains("the package"),
            "{}",
            outcome.findings[0].message
        );
    }

    #[test]
    fn a_tree_git_cannot_read_is_a_failure_not_a_clean_gate() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "CLAUDE.md",
            "<!-- harnex-fill: the project name -->\n",
        );
        let error = FillMarkerAuditor::new().audit(dir.path()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AuditGitFailure);
    }

    #[test]
    fn a_tracked_file_gone_from_disk_carries_no_marker() {
        let dir = repo();
        write(
            dir.path(),
            "CLAUDE.md",
            "<!-- harnex-fill: the project name -->\n",
        );
        git(dir.path(), &["add", "CLAUDE.md"]);
        std::fs::remove_file(dir.path().join("CLAUDE.md")).unwrap();

        let outcome = FillMarkerAuditor::new().audit(dir.path()).unwrap();
        assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
        assert_eq!(outcome.files_scanned, 0);
    }

    #[test]
    fn a_file_that_cannot_be_read_fails_the_audit_rather_than_passing_it() {
        let dir = repo();
        std::fs::write(dir.path().join("CLAUDE.md"), [0xff, 0xfe, 0x00]).unwrap();

        let error = FillMarkerAuditor::new().audit(dir.path()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::IoFailure);
    }
}
