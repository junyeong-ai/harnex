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
        /// The committed baseline of the spec: its decision-log bullets must
        /// stand verbatim as a prefix of `--spec`'s
        #[arg(long)]
        baseline_spec: Option<PathBuf>,
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
        baseline_spec,
    } = cmd;

    // An absent plan is a judged state when anything else anchors the audit —
    // a deletion the baseline witnesses, or a spec-only spec whose log is
    // still held. A bare --plan pointing at nothing stays a runtime failure:
    // auditing nothing against nothing would read as a pass.
    let plan_text = match read_lossy(&plan) {
        Ok(text) => Some(text),
        Err(Error::IoFailure { ref source, .. })
            if source.kind() == std::io::ErrorKind::NotFound
                && (baseline.is_some() || spec.is_some()) =>
        {
            None
        }
        Err(e) => return Err(e),
    };
    let spec_text = spec.as_deref().map(read_lossy).transpose()?;
    let baseline_text = baseline
        .as_deref()
        .map(|p| {
            if p == Path::new("-") {
                let mut bytes = Vec::new();
                std::io::stdin()
                    .read_to_end(&mut bytes)
                    .map_err(|e| io_failure(p, e))?;
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            } else {
                read_lossy(p)
            }
        })
        .transpose()?;

    let baseline_spec_text = baseline_spec.as_deref().map(read_lossy).transpose()?;

    let spec_input = spec.as_deref().zip(spec_text.as_deref());
    let findings = PlanAuditor::new(
        &plan,
        plan_text.as_deref(),
        spec_input,
        baseline_text.as_deref(),
        baseline_spec_text.as_deref(),
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

/// Lossy on purpose: one invalid byte in a staged artifact must degrade to a
/// replacement character the parser still reads, never to a runtime failure
/// the fail-open hook turns into a skipped gate.
fn read_lossy(path: &Path) -> Result<String> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| io_failure(path, e))
}

fn io_failure(path: &Path, source: std::io::Error) -> Error {
    Error::IoFailure {
        path: path.to_path_buf(),
        source,
    }
}
