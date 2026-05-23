//! # init — scaffold a Claude Code project for harness-toolkit
//!
//! Creates `harness.toml`, `CLAUDE.md`, `README.md`, `.claude/rules/constitution.md`,
//! `.claude/settings.json`, and (optionally) a `hooks/` directory with
//! fail-open hook scripts under the target directory. Existing files are
//! skipped unless `force` is set. If `AGENTS.md` is present at the target
//! root, the generated `CLAUDE.md` includes an `@AGENTS.md` import line.
//!
//! ## What this module refuses to do
//!
//! - Never overwrite a non-empty existing file unless `force=true`.
//! - Never write outside the target directory.
//! - Never spawn external commands. Initialisation is pure file I/O.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::PermissionsPolicy;
use crate::error::{Error, Result};
use crate::path_guard;
use crate::policy::PermissionGenerator;

const TEMPLATE_HARNESS_TOML: &str = include_str!("../../templates/harness.toml");
const TEMPLATE_CLAUDE_MD: &str = include_str!("../../templates/CLAUDE.md");
const TEMPLATE_README_MD: &str = include_str!("../../templates/README.md");
const TEMPLATE_CONSTITUTION_MD: &str = include_str!("../../templates/constitution.md");

const TEMPLATE_HOOK_RUNNER: &str = include_str!("../../templates/hooks/_runner.sh");
const TEMPLATE_HOOK_STOP_RUNNER: &str = include_str!("../../templates/hooks/_stop_runner.sh");
const TEMPLATE_HOOK_POST_FORMAT: &str = include_str!("../../templates/hooks/post-format.sh");
const TEMPLATE_HOOK_SESSION_START: &str = include_str!("../../templates/hooks/session-start.sh");
const TEMPLATE_HOOK_CHECK_ON_STOP: &str = include_str!("../../templates/hooks/check-on-stop.sh");

/// All hook templates in the order they are written to `hooks/`.
const HOOK_TEMPLATES: &[(&str, &str)] = &[
    ("hooks/_runner.sh", TEMPLATE_HOOK_RUNNER),
    ("hooks/_stop_runner.sh", TEMPLATE_HOOK_STOP_RUNNER),
    ("hooks/post-format.sh", TEMPLATE_HOOK_POST_FORMAT),
    ("hooks/session-start.sh", TEMPLATE_HOOK_SESSION_START),
    ("hooks/check-on-stop.sh", TEMPLATE_HOOK_CHECK_ON_STOP),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FileAction {
    Created,
    Skipped,
    Overwritten,
    Planned,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct InitFileOutcome {
    pub path: PathBuf,
    pub action: FileAction,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct InitOutcome {
    pub target_dir: PathBuf,
    pub project_name: String,
    pub agents_md_imported: bool,
    pub files: Vec<InitFileOutcome>,
    /// Ordered first-run commands. AI agents and operators consume this
    /// to know "what to run next" without parsing prose. Each entry is a
    /// shell-ready command line, NOT a description.
    pub next_steps: Vec<&'static str>,
}

pub struct ProjectInitializer<'a> {
    target_dir: &'a Path,
    project_name: String,
    force: bool,
    hooks: bool,
}

impl<'a> ProjectInitializer<'a> {
    pub fn new(target_dir: &'a Path, project_name: impl Into<String>, force: bool) -> Self {
        Self {
            target_dir,
            project_name: project_name.into(),
            force,
            hooks: true,
        }
    }

    /// Disable hook script generation.
    pub fn with_hooks(mut self, hooks: bool) -> Self {
        self.hooks = hooks;
        self
    }

    /// Dry-run: report what `apply` would do without writing anything.
    pub fn plan(&self) -> Result<InitOutcome> {
        self.run(false)
    }

    /// Materialise the project. Existing files are skipped unless `force`.
    pub fn apply(&self) -> Result<InitOutcome> {
        self.run(true)
    }

    fn run(&self, apply: bool) -> Result<InitOutcome> {
        path_guard::reject_traversal(self.target_dir)?;
        if apply {
            std::fs::create_dir_all(self.target_dir).map_err(|e| Error::IoFailure {
                path: self.target_dir.to_path_buf(),
                source: e,
            })?;
        }

        let agents_md_path = self.target_dir.join("AGENTS.md");
        let agents_md_imported = agents_md_path.is_file();

        let claude_md = self.render_claude_md(agents_md_imported);
        let readme_md = self.render_readme_md();
        let settings_json = render_settings_json(self.hooks)?;

        let mut plans: Vec<(&str, String)> = vec![
            ("harness.toml", TEMPLATE_HARNESS_TOML.to_string()),
            ("CLAUDE.md", claude_md),
            ("README.md", readme_md),
            (
                ".claude/rules/constitution.md",
                TEMPLATE_CONSTITUTION_MD.to_string(),
            ),
            (".claude/settings.json", settings_json),
        ];

        if self.hooks {
            for &(rel, content) in HOOK_TEMPLATES {
                plans.push((rel, content.to_string()));
            }
        }

        let mut files = Vec::with_capacity(plans.len());
        for (rel, contents) in &plans {
            files.push(self.process_one(rel, contents, apply)?);
        }

        if apply && self.hooks {
            set_hook_permissions(self.target_dir)?;
        }

        Ok(InitOutcome {
            target_dir: self.target_dir.to_path_buf(),
            project_name: self.project_name.clone(),
            agents_md_imported,
            files,
            next_steps: Self::FIRST_RUN_STEPS.to_vec(),
        })
    }

    /// Canonical first-run sequence emitted in [`InitOutcome::next_steps`].
    /// Order matters: each step is the natural follow-up after the previous.
    const FIRST_RUN_STEPS: &'static [&'static str] = &[
        "harness check",
        "harness policy permissions audit",
        "harness export schema config --raw > schemas/harness.schema.json",
    ];

    fn process_one(&self, rel: &str, contents: &str, apply: bool) -> Result<InitFileOutcome> {
        let path = self.target_dir.join(rel);
        let exists = path.exists();
        let action = if exists && !self.force {
            FileAction::Skipped
        } else if !apply {
            FileAction::Planned
        } else if exists {
            FileAction::Overwritten
        } else {
            FileAction::Created
        };
        if apply && action != FileAction::Skipped {
            path_guard::write_atomic(&path, contents.as_bytes())?;
        }
        Ok(InitFileOutcome {
            path,
            action,
            bytes: contents.len(),
        })
    }

    fn render_claude_md(&self, agents_md_imported: bool) -> String {
        let mut body = TEMPLATE_CLAUDE_MD.replace("<PROJECT_NAME>", &self.project_name);
        if agents_md_imported {
            body.push_str("\n## Imported instructions\n\n@AGENTS.md\n");
        }
        body
    }

    fn render_readme_md(&self) -> String {
        TEMPLATE_README_MD.replace("<PROJECT_NAME>", &self.project_name)
    }
}

fn render_settings_json(include_hooks: bool) -> Result<String> {
    let policy = PermissionsPolicy {
        profiles: vec!["baseline".to_string()],
        ..Default::default()
    };
    let block = PermissionGenerator::new(&policy)?.generate();
    let mut value = serde_json::json!({
        "permissions": {
            "allow": block.allow,
            "ask": block.ask,
            "deny": block.deny,
        }
    });
    if include_hooks {
        value["hooks"] = serde_json::json!({
            "SessionStart": [
                {
                    "matcher": "startup|resume",
                    "hooks": [{"type": "command", "command": "hooks/_runner.sh session-start.sh", "timeout": 10000}]
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Edit|Write",
                    "hooks": [{"type": "command", "command": "hooks/_runner.sh post-format.sh", "timeout": 15000}]
                }
            ],
            "Stop": [
                {
                    "hooks": [{"type": "command", "command": "hooks/_stop_runner.sh check-on-stop.sh", "timeout": 30000}]
                }
            ]
        });
    }
    Ok(serde_json::to_string_pretty(&value).expect("serialize is infallible") + "\n")
}

/// Set executable permissions on all hook scripts (Unix only).
fn set_hook_permissions(target_dir: &Path) -> Result<()> {
    for &(rel, _) in HOOK_TEMPLATES {
        let path = target_dir.join(rel);
        if path.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(
                    |e| Error::IoFailure {
                        path: path.clone(),
                        source: e,
                    },
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn plan_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let init = ProjectInitializer::new(tmp.path(), "test-proj", false);
        let outcome = init.plan().unwrap();
        for f in &outcome.files {
            assert_eq!(f.action, FileAction::Planned);
            assert!(!f.path.exists(), "plan must not write {:?}", f.path);
        }
    }

    #[test]
    fn apply_creates_all_files() {
        let tmp = TempDir::new().unwrap();
        let init = ProjectInitializer::new(tmp.path(), "test-proj", false);
        let outcome = init.apply().unwrap();
        for f in &outcome.files {
            assert_eq!(f.action, FileAction::Created);
            assert!(f.path.exists(), "{:?} not created", f.path);
        }
        // Verify content substitution
        let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert!(claude.contains("test-proj"));
    }

    #[test]
    fn second_apply_skips_existing() {
        let tmp = TempDir::new().unwrap();
        let init = ProjectInitializer::new(tmp.path(), "test-proj", false);
        init.apply().unwrap();
        let outcome2 = init.apply().unwrap();
        for f in &outcome2.files {
            assert_eq!(f.action, FileAction::Skipped, "{:?}", f.path);
        }
    }

    #[test]
    fn force_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();
        let outcome = ProjectInitializer::new(tmp.path(), "test-proj", true)
            .apply()
            .unwrap();
        for f in &outcome.files {
            assert_eq!(f.action, FileAction::Overwritten, "{:?}", f.path);
        }
    }

    #[test]
    fn agents_md_triggers_import_line() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# agents content").unwrap();
        let outcome = ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();
        assert!(outcome.agents_md_imported);
        let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert!(claude.contains("@AGENTS.md"));
    }

    #[test]
    fn settings_json_includes_baseline_denies() {
        let tmp = TempDir::new().unwrap();
        ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();
        let settings = std::fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        assert!(settings.contains("sudo"));
        assert!(settings.contains("rm -rf"));
    }

    #[test]
    fn generated_harness_toml_loads_and_validates() {
        let tmp = TempDir::new().unwrap();
        ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();
        let (_cfg, _path) = crate::config::Config::load(tmp.path()).unwrap();
    }

    #[test]
    fn hooks_created_by_default() {
        let tmp = TempDir::new().unwrap();
        let outcome = ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();

        let hook_files = [
            "hooks/_runner.sh",
            "hooks/_stop_runner.sh",
            "hooks/post-format.sh",
            "hooks/session-start.sh",
            "hooks/check-on-stop.sh",
        ];

        for rel in &hook_files {
            let path = tmp.path().join(rel);
            assert!(path.exists(), "{rel} not created");
        }

        // Verify hook entries appear in outcome
        let hook_outcomes: Vec<_> = outcome
            .files
            .iter()
            .filter(|f| f.path.to_string_lossy().contains("hooks/"))
            .collect();
        assert_eq!(hook_outcomes.len(), 5);
        for ho in &hook_outcomes {
            assert_eq!(ho.action, FileAction::Created);
        }

        // Verify settings.json contains hooks configuration
        let settings = std::fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        assert!(settings.contains("SessionStart"));
        assert!(settings.contains("PostToolUse"));
        assert!(settings.contains("Stop"));
        assert!(settings.contains("hooks/_runner.sh session-start.sh"));
        assert!(settings.contains("hooks/_runner.sh post-format.sh"));
        assert!(settings.contains("hooks/_stop_runner.sh check-on-stop.sh"));
    }

    #[test]
    fn hooks_not_created_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let outcome = ProjectInitializer::new(tmp.path(), "test-proj", false)
            .with_hooks(false)
            .apply()
            .unwrap();

        assert!(
            !tmp.path().join("hooks").exists(),
            "hooks/ directory should not exist when hooks disabled"
        );

        let hook_outcomes: Vec<_> = outcome
            .files
            .iter()
            .filter(|f| f.path.to_string_lossy().contains("hooks/"))
            .collect();
        assert!(hook_outcomes.is_empty());

        // settings.json should NOT contain hooks section
        let settings = std::fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        assert!(!settings.contains("SessionStart"));
        assert!(!settings.contains("PostToolUse"));
    }

    #[test]
    fn hook_files_have_correct_content() {
        let tmp = TempDir::new().unwrap();
        ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();

        let runner = std::fs::read_to_string(tmp.path().join("hooks/_runner.sh")).unwrap();
        assert!(runner.starts_with("#!/usr/bin/env bash"));
        assert!(runner.contains("set -euo pipefail"));
        assert!(runner.contains("git rev-parse --show-toplevel"));

        let stop_runner =
            std::fs::read_to_string(tmp.path().join("hooks/_stop_runner.sh")).unwrap();
        assert!(stop_runner.contains("exit 0"));

        let post_format = std::fs::read_to_string(tmp.path().join("hooks/post-format.sh")).unwrap();
        assert!(post_format.contains("cargo fmt"));

        let session_start =
            std::fs::read_to_string(tmp.path().join("hooks/session-start.sh")).unwrap();
        assert!(session_start.contains("additionalContext"));

        let check_on_stop =
            std::fs::read_to_string(tmp.path().join("hooks/check-on-stop.sh")).unwrap();
        assert!(check_on_stop.contains("uncommitted files"));
    }

    #[cfg(unix)]
    #[test]
    fn hook_files_are_executable() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        ProjectInitializer::new(tmp.path(), "test-proj", false)
            .apply()
            .unwrap();

        let hook_files = [
            "hooks/_runner.sh",
            "hooks/_stop_runner.sh",
            "hooks/post-format.sh",
            "hooks/session-start.sh",
            "hooks/check-on-stop.sh",
        ];

        for rel in &hook_files {
            let path = tmp.path().join(rel);
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o111,
                0o111,
                "{rel} should be executable (mode: {mode:#o})"
            );
        }
    }
}
