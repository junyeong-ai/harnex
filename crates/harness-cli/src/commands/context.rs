use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;
use harness_core::{context, error::Result};

use super::{config_dir, load_config, write_envelope_success};

#[derive(Subcommand)]
pub enum ContextCommand {
    /// Required Claude-owned instructions for repository-relative file targets;
    /// no targets returns the always-loaded foundation
    Resolve { paths: Vec<String> },
}

pub fn run<W: Write>(cmd: ContextCommand, out: &mut W) -> Result<ExitCode> {
    let (_, config_path, working_dir) = load_config()?;
    let root = config_dir(&config_path, &working_dir);
    match cmd {
        ContextCommand::Resolve { paths } => {
            write_envelope_success(out, context::resolve(&root, &paths)?)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
