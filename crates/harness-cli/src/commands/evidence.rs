use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
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
    /// Record a declared advisory's measurement as its evidence baseline,
    /// digesting the declared inputs and engine as of now
    Record {
        /// The [[evidence.advisories]] id to record
        #[arg(long)]
        id: String,
        /// JSON file holding the measurement payload (`-` reads stdin);
        /// omitted records a freshness-only baseline
        #[arg(long)]
        payload: Option<PathBuf>,
    },
}

pub fn run<W: Write>(cmd: EvidenceCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        EvidenceCommand::Verify { paths } => verify(paths, out),
        EvidenceCommand::Record { id, payload } => record(&id, payload, out),
    }
}

fn record<W: Write>(id: &str, payload: Option<PathBuf>, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = super::load_config()?;
    let root = super::config_dir(&config_path, &working_dir);
    let evidence_cfg = config
        .evidence
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [evidence] section in harness.toml".into(),
            location: None,
        })?;
    let payload_value = match payload {
        None => serde_json::Value::Null,
        Some(p) => {
            let raw = if p.as_os_str() == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|e| {
                    Error::IoFailure {
                        path: PathBuf::from("-"),
                        source: e,
                    }
                })?;
                buf
            } else {
                std::fs::read_to_string(&p).map_err(|e| Error::IoFailure {
                    path: p.clone(),
                    source: e,
                })?
            };
            serde_json::from_str(&raw).map_err(|e| Error::ConfigInvalid {
                message: format!("payload is not JSON: {e}"),
                location: None,
            })?
        }
    };
    let evidence =
        harness_core::evidence::advisory::record(&root, evidence_cfg, id, payload_value)?;
    write_envelope_success(out, evidence)?;
    Ok(ExitCode::SUCCESS)
}

fn verify<W: Write>(paths: Vec<PathBuf>, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = super::load_config()?;
    let root = super::config_dir(&config_path, &working_dir);
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
        let mut fs = verifier.verify_file(&p, &root)?;
        findings.append(&mut fs);
    }

    let has_gating_finding = findings.iter().any(|f| f.severity.fails_gate());

    write_envelope_success(out, ListResponse::new(findings))?;

    Ok(if has_gating_finding {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
