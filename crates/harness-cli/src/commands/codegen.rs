use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::codegen::SentinelSyncer;
use harness_core::envelope::ListResponse;
use harness_core::error::{Error, Result};

use super::{config_dir, load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum CodegenCommand {
    /// Apply every sentinel-block sync; write target files when content changes
    Sync,
    /// Report would-change drift without writing
    Check,
}

pub fn run<W: Write>(cmd: CodegenCommand, out: &mut W) -> Result<ExitCode> {
    let (config, config_path, working_dir) = load_config()?;
    let cg = config
        .codegen
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [codegen] section in harness.toml".into(),
            location: None,
        })?;
    let root = config_dir(&config_path, &working_dir);
    let sync = SentinelSyncer::new(cg, &root);

    let (outcomes, exit) = match cmd {
        CodegenCommand::Sync => (sync.sync()?, ExitCode::SUCCESS),
        CodegenCommand::Check => {
            let oc = sync.check()?;
            let drifted = oc.iter().any(|o| o.changed);
            (
                oc,
                if drifted {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                },
            )
        }
    };

    write_envelope_success(out, ListResponse::new(outcomes))?;
    Ok(exit)
}
