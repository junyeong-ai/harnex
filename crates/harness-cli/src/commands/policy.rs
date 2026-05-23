use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
use harness_core::error::{Error, Result};
use harness_core::policy::{
    PermissionAuditor, PermissionGenerator, PermissionsBlock, VersionChecker,
};

use super::{load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum PolicyCommand {
    /// Permission profile generator / auditor
    Permissions {
        #[command(subcommand)]
        cmd: PermissionsCommand,
    },
    /// Tool version pin checker
    Versions {
        #[command(subcommand)]
        cmd: VersionsCommand,
    },
}

#[derive(Subcommand)]
pub enum PermissionsCommand {
    /// Emit a `.claude/settings.json`-shaped permission block from the
    /// configured profiles + extras
    Generate,
    /// Audit a `.claude/settings.json` against the configured policy
    Audit {
        #[arg(long, default_value = ".claude/settings.json")]
        path: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
pub enum VersionsCommand {
    /// Show the declared version pins
    Show,
    /// Check one installed version against the declared pin
    Check {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        installed: String,
    },
}

pub fn run<W: Write>(cmd: PolicyCommand, out: &mut W) -> Result<ExitCode> {
    let (config, _config_path, _working_dir) = load_config()?;
    let policy = config.policy.as_ref().ok_or_else(|| Error::ConfigInvalid {
        message: "no [policy] section in harness.toml".into(),
        location: None,
    })?;

    match cmd {
        PolicyCommand::Permissions { cmd } => run_permissions(cmd, policy, out),
        PolicyCommand::Versions { cmd } => run_versions(cmd, policy, out),
    }
}

fn run_permissions<W: Write>(
    cmd: PermissionsCommand,
    policy: &harness_core::config::PolicyConfig,
    out: &mut W,
) -> Result<ExitCode> {
    let perms_policy = policy
        .permissions
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [policy.permissions] section in harness.toml".into(),
            location: None,
        })?;

    match cmd {
        PermissionsCommand::Generate => {
            let block: PermissionsBlock = PermissionGenerator::new(perms_policy)?.generate();
            write_envelope_success(out, block)?;
            Ok(ExitCode::SUCCESS)
        }
        PermissionsCommand::Audit { path } => {
            let raw = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            let v: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| Error::ConfigInvalid {
                    message: format!("settings.json parse: {e}"),
                    location: None,
                })?;
            let allow: Vec<String> = v
                .pointer("/permissions/allow")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let ask: Vec<String> = v
                .pointer("/permissions/ask")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let deny: Vec<String> = v
                .pointer("/permissions/deny")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let findings = PermissionAuditor::new(perms_policy, &allow, &ask, &deny).audit();
            let has_issues = !findings.is_empty();
            write_envelope_success(out, ListResponse::new(findings))?;
            Ok(if has_issues {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

fn run_versions<W: Write>(
    cmd: VersionsCommand,
    policy: &harness_core::config::PolicyConfig,
    out: &mut W,
) -> Result<ExitCode> {
    let checker = VersionChecker::new(&policy.versions);
    match cmd {
        VersionsCommand::Show => {
            write_envelope_success(out, checker.show().to_vec())?;
            Ok(ExitCode::SUCCESS)
        }
        VersionsCommand::Check { tool, installed } => {
            let verdict = checker.check_installed(&tool, &installed)?;
            let exit = if verdict.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            };
            write_envelope_success(out, verdict)?;
            Ok(exit)
        }
    }
}
