use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::ListResponse;
use harness_core::error::Result;
use harness_core::governs;

use super::{load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum GovernsCommand {
    /// The rules whose `governs.live_truth` covers each given path
    Resolve {
        /// Project-relative paths to resolve (files or directories)
        #[arg(required = true)]
        paths: Vec<String>,
    },
}

pub fn run<W: Write>(cmd: GovernsCommand, out: &mut W) -> Result<ExitCode> {
    let (_config, _config_path, working_dir) = load_config()?;
    match cmd {
        GovernsCommand::Resolve { paths } => {
            let rules = governs::load(&working_dir)?;
            let resolutions = governs::resolve(&rules, &paths);
            write_envelope_success(out, ListResponse::new(resolutions))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
