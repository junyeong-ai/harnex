use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use serde::Serialize;

use harness_core::always_loaded::{self, AlwaysLoaded};
use harness_core::envelope::{Finding, ListResponse};
use harness_core::error::{Error, Result};
use harness_core::validate::{
    AgentValidator, CommitMsgValidator, OutputStyleValidator, RuleValidator, SettingsScope,
    SettingsValidator, SkillValidator,
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
    /// Validate `.claude/agents/*.md` subagent definitions
    Agents {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Validate `.claude/output-styles/*.md`
    OutputStyles {
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
    /// Measure what this repository puts into every session, member by
    /// member. Held to `[validate.always_loaded] max_chars` when the section
    /// is declared; measured either way, so a budget can be chosen from it.
    AlwaysLoaded,
}

/// The set, the budget it was held to, and what holding it found.
#[derive(Serialize)]
struct AlwaysLoadedReport {
    #[serde(flatten)]
    loaded: AlwaysLoaded,
    max_chars: Option<usize>,
    findings: Vec<Finding>,
}

pub fn run<W: Write>(cmd: ValidateCommand, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = load_config()?;

    let mut findings = Vec::new();
    match cmd {
        ValidateCommand::AlwaysLoaded => {
            let root = super::config_dir(&config_path, &working_dir);
            let loaded = always_loaded::resolve(&root)?;
            let max_chars = config
                .validate
                .as_ref()
                .and_then(|v| v.always_loaded.as_ref())
                .map(|policy| policy.max_chars);
            let findings: Vec<Finding> = max_chars
                .and_then(|max| always_loaded::over_budget(&loaded, max, &root))
                .into_iter()
                .collect();
            let has_gating_finding = findings.iter().any(|f| f.severity.fails_gate());
            write_envelope_success(
                out,
                AlwaysLoadedReport {
                    loaded,
                    max_chars,
                    findings,
                },
            )?;
            return Ok(if has_gating_finding {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            });
        }
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
                findings.extend(v.validate_file(&p));
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
                findings.extend(v.validate_file(&p));
            }
        }
        ValidateCommand::Agents { paths } => {
            let policy = config
                .validate
                .as_ref()
                .and_then(|v| v.agents.as_ref())
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [validate.agents] section in harness.toml".into(),
                    location: None,
                })?;
            let v = AgentValidator::new(policy);
            for p in paths {
                findings.extend(v.validate_file(&p));
            }
        }
        ValidateCommand::OutputStyles { paths } => {
            let policy = config
                .validate
                .as_ref()
                .and_then(|v| v.output_styles.as_ref())
                .ok_or_else(|| Error::ConfigInvalid {
                    message: "no [validate.output_styles] section in harness.toml".into(),
                    location: None,
                })?;
            let v = OutputStyleValidator::new(policy);
            for p in paths {
                findings.extend(v.validate_file(&p));
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
            findings.extend(v.validate_file(&path, scope));
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

    let has_gating_finding = findings.iter().any(|f| f.severity.fails_gate());
    write_envelope_success(out, ListResponse::new(findings))?;
    Ok(if has_gating_finding {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
