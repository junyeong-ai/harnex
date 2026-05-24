use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::{ListResponse, Severity};
use harness_core::error::{Error, Result};
use harness_core::validate::{
    CommitMsgValidator, RuleValidator, SettingsScope, SettingsValidator, SkillValidator,
};

use super::{load_config, write_envelope_success};

/// Closed-set value parser for clap, sourced from the enum's `ALL` (single
/// source of truth — drift impossible).
fn settings_scope_values() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(
        SettingsScope::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
    )
}

#[derive(Subcommand)]
pub enum ValidateCommand {
    /// Validate `.claude/rules/*.md` files
    Rules {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Validate `.claude/skills/*/SKILL.md` files
    Skills {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Validate `.claude/settings.json`
    Settings {
        #[arg(default_value = ".claude/settings.json")]
        path: PathBuf,
        /// Scope of the settings file. Affects scope-restricted checks
        /// (`auto` defaultMode, `autoMemoryDirectory`, …). Default: project.
        #[arg(long, value_parser = settings_scope_values(), default_value = "project")]
        scope: String,
    },
    /// Validate a git commit message against `[validate.commit_msg]` trailers
    /// (e.g., `.git/COMMIT_EDITMSG` from a commit-msg hook)
    CommitMsg { path: PathBuf },
}

pub fn run<W: Write>(cmd: ValidateCommand, out: &mut W) -> Result<ExitCode> {
    let (config, _config_path, _working_dir) = load_config()?;

    let mut findings = Vec::new();
    match cmd {
        ValidateCommand::Rules { paths } => {
            let policy = config
                .validate
                .as_ref()
                .and_then(|v| v.rules.as_ref())
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [validate.rules] section in harness.toml".into(),
                    location: None,
                })?;
            let v = RuleValidator::new(policy);
            for p in paths {
                findings.extend(v.validate_file(&p)?);
            }
        }
        ValidateCommand::Skills { paths } => {
            let policy = config
                .validate
                .as_ref()
                .and_then(|v| v.skills.as_ref())
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [validate.skills] section in harness.toml".into(),
                    location: None,
                })?;
            let v = SkillValidator::new(policy);
            for p in paths {
                findings.extend(v.validate_file(&p)?);
            }
        }
        ValidateCommand::Settings { path, scope } => {
            let scope = SettingsScope::from_str(&scope).ok_or_else(|| Error::ConfigInvalid {
                message: format!(
                    "unknown settings scope '{scope}' (use: {})",
                    SettingsScope::ALL
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                location: None,
            })?;
            let v = SettingsValidator::new();
            findings.extend(v.validate_file(&path, scope)?);
        }
        ValidateCommand::CommitMsg { path } => {
            let policy = config
                .validate
                .as_ref()
                .and_then(|v| v.commit_msg.as_ref())
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [validate.commit_msg] section in harness.toml".into(),
                    location: None,
                })?;
            let v = CommitMsgValidator::new(policy);
            findings.extend(v.validate_file(&path)?);
        }
    }

    let has_blocker = findings.iter().any(|f| f.severity == Severity::Blocker);
    write_envelope_success(out, ListResponse::new(findings))?;
    Ok(if has_blocker {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
