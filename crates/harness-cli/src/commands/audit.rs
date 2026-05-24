use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use harness_core::audit::ProjectAuditor;
use harness_core::envelope::Severity;
use harness_core::error::Result;

use super::write_envelope_success;

#[derive(Args)]
pub struct AuditArgs {
    /// Plugin root containing `templates/managed-files.toml`. When supplied,
    /// the managed-region auditor compares generated artifacts against the
    /// canonical templates. Without it, only spec-drift auditors run.
    #[arg(long)]
    pub plugin_root: Option<PathBuf>,
}

pub fn run<W: Write>(args: AuditArgs, out: &mut W) -> Result<ExitCode> {
    let working_dir = std::env::current_dir().map_err(|e| {
        harness_core::error::Error::IoFailure {
            path: PathBuf::from("."),
            source: e,
        }
    })?;
    let mut auditor = ProjectAuditor::new(&working_dir);
    if let Some(plugin_root) = args.plugin_root {
        auditor = auditor.with_plugin_root(plugin_root);
    }
    let outcome = auditor.run()?;
    let has_blocker = outcome
        .findings
        .iter()
        .any(|f| f.severity == Severity::Blocker);
    write_envelope_success(out, outcome)?;
    Ok(if has_blocker {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
