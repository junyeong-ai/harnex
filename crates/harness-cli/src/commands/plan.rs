use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
use harness_core::error::{Error, Result};
use harness_core::plan::PlanAuditor;

use super::write_envelope_success;

#[derive(Subcommand)]
pub enum PlanCommand {
    /// Audit a spec's plan — and optionally its decision log — against the
    /// review grammar the spec-workflow templates write
    Audit {
        /// The plan carrying `## Outstanding issues`. May not exist when
        /// `--baseline` is given: a deleted plan is judged by what the
        /// baseline still holds open
        #[arg(long)]
        plan: PathBuf,
        /// The spec carrying `## Decision log`, for the convergence checks
        #[arg(long)]
        spec: Option<PathBuf>,
        /// The committed baseline of the plan (`-` reads stdin). Open rows it
        /// holds must survive in `--plan` or carry a terminal disposition
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
}

/// No `harness.toml` is loaded: the grammar is harness vocabulary, the files
/// are named by the caller, and a gate that must run in any repo the pattern
/// is installed in cannot require configuration to exist first.
pub fn run<W: Write>(cmd: PlanCommand, out: &mut W) -> Result<ExitCode> {
    let PlanCommand::Audit {
        plan,
        spec,
        baseline,
    } = cmd;

    let plan_text = match std::fs::read_to_string(&plan) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && baseline.is_some() => None,
        Err(e) => return Err(io_failure(&plan, e)),
    };
    let spec_text = spec
        .as_deref()
        .map(|p| std::fs::read_to_string(p).map_err(|e| io_failure(p, e)))
        .transpose()?;
    let baseline_text = baseline
        .as_deref()
        .map(|p| {
            if p == Path::new("-") {
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| io_failure(p, e))?;
                Ok(text)
            } else {
                std::fs::read_to_string(p).map_err(|e| io_failure(p, e))
            }
        })
        .transpose()?;

    let spec_input = spec.as_deref().zip(spec_text.as_deref());
    let findings = PlanAuditor::new(
        &plan,
        plan_text.as_deref(),
        spec_input,
        baseline_text.as_deref(),
    )
    .audit();

    let has_gating_finding = findings.iter().any(|f| f.severity.fails_gate());
    write_envelope_success(out, ListResponse::new(findings))?;
    Ok(if has_gating_finding {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn io_failure(path: &Path, source: std::io::Error) -> Error {
    Error::IoFailure {
        path: path.to_path_buf(),
        source,
    }
}
