use std::io::Write;
use std::process::ExitCode;

use clap::Args;

use harness_core::check::ProjectChecker;
use harness_core::error::Result;

use super::{load_config, write_envelope_success};

#[derive(Args)]
pub struct CheckArgs {
    /// Restrict scanning to files changed since this git ref
    /// (e.g. `--since main`, `--since HEAD~5`).
    #[arg(long)]
    pub since: Option<String>,
    /// Execute every auto_fixable finding via the safe-fix registry,
    /// then re-run check. Exit code reflects the post-fix findings.
    #[arg(long, default_value_t = false)]
    pub fix: bool,
}

pub fn run<W: Write>(args: CheckArgs, out: &mut W) -> Result<ExitCode> {
    let (config, _config_path, working_dir) = load_config()?;
    let mut check = ProjectChecker::new(&config, &working_dir);
    if let Some(since) = args.since.as_deref() {
        check = check.with_since(since);
    }
    let gating_in = |findings: &[harness_core::envelope::Finding]| {
        findings.iter().any(|f| f.severity.fails_gate())
    };
    if args.fix {
        let outcome = check.fix()?;
        let has_gating_finding = gating_in(&outcome.after.findings);
        write_envelope_success(out, outcome)?;
        Ok(if has_gating_finding {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        })
    } else {
        let outcome = check.run()?;
        let has_gating_finding = gating_in(&outcome.findings);
        write_envelope_success(out, outcome)?;
        Ok(if has_gating_finding {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        })
    }
}
