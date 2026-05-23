use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::config::Config;
use harness_core::envelope::{ListResponse, Severity};
use harness_core::error::{Error, Result};
use harness_core::evidence::EvidenceVerifier;

use super::write_envelope_success;

#[derive(Subcommand)]
pub enum EvidenceCommand {
    /// Verify provenance markers in one or more markdown files
    Verify {
        /// Paths to markdown files
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

pub fn run<W: Write>(cmd: EvidenceCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        EvidenceCommand::Verify { paths } => verify(paths, out),
    }
}

fn verify<W: Write>(paths: Vec<PathBuf>, out: &mut W) -> Result<ExitCode> {
    let working_dir = std::env::current_dir().map_err(|e| Error::IoFailure {
        path: PathBuf::from("."),
        source: e,
    })?;
    let (config, _config_path) = Config::load(&working_dir)?;
    let evidence_cfg = config
        .evidence
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [evidence] section in harness.toml".into(),
            location: None,
        })?;
    let verifier = EvidenceVerifier::new(evidence_cfg)?;

    let mut findings = Vec::new();
    for p in paths {
        let mut fs = verifier.verify_file(&p, &working_dir)?;
        findings.append(&mut fs);
    }

    let has_blocker = findings.iter().any(|f| f.severity == Severity::Blocker);

    write_envelope_success(out, ListResponse::new(findings))?;

    Ok(if has_blocker {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
