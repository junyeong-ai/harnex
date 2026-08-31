use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

use harness_core::envelope::{ListResponse, Warning};
use harness_core::error::Result;
use harness_core::governs;

use super::{config_dir, load_config, write_envelope_success_warned};

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
    let (_config, config_path, working_dir) = load_config()?;
    let root = config_dir(&config_path, &working_dir);
    match cmd {
        GovernsCommand::Resolve { paths } => {
            let outcome = governs::load(&root)?;
            let queries = paths
                .iter()
                .map(|p| governs::normalize_query(&root, p))
                .collect::<Result<Vec<_>>>()?;
            let resolutions = governs::resolve(&outcome.rules, &queries);
            // A defective declaration is excluded from the answer, and an
            // exclusion the caller cannot see is silent scope shrinkage —
            // the result is correct and cannot yet be trusted whole.
            let warnings = outcome
                .defects
                .iter()
                .map(|d| Warning {
                    code: "governs-declaration-excluded".into(),
                    message: format!(
                        "{}: {} — excluded from resolution; `harnex check` reports it",
                        d.rule, d.error
                    ),
                })
                .collect();
            write_envelope_success_warned(out, ListResponse::new(resolutions), warnings)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
