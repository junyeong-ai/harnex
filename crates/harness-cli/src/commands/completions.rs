use std::io::Write;
use std::process::ExitCode;

use clap::{Args, CommandFactory, ValueEnum};
use clap_complete::{Shell, generate};
use serde::Serialize;

use harness_core::error::{Error, Result};

use super::write_envelope_success;

#[derive(Args)]
pub struct CompletionsArgs {
    /// Target shell (bash | zsh | fish | powershell | elvish)
    pub shell: String,
    /// Emit the raw completion script directly to stdout instead of
    /// wrapping it in a JSON envelope. Use this when piping to a file
    /// the shell will `source`:
    ///
    ///     harness completions bash --raw > ~/.bashrc.d/harness
    #[arg(long, default_value_t = false)]
    pub raw: bool,
}

#[derive(Serialize)]
struct CompletionsPayload {
    shell: String,
    script: String,
}

pub fn run<W: Write>(args: CompletionsArgs, out: &mut W) -> Result<ExitCode> {
    let shell = parse_shell(&args.shell)?;
    let mut cmd = crate::Cli::command();
    let bin_name = cmd.get_name().to_string();

    let mut buf: Vec<u8> = Vec::new();
    generate(shell, &mut cmd, &bin_name, &mut buf);
    let script = String::from_utf8(buf).map_err(|e| Error::IoFailure {
        path: std::path::PathBuf::from("(completions)"),
        source: std::io::Error::other(format!("non-utf8 completion output: {e}")),
    })?;

    if args.raw {
        out.write_all(script.as_bytes())
            .map_err(|e| Error::IoFailure {
                path: std::path::PathBuf::from("(stdout)"),
                source: e,
            })?;
    } else {
        write_envelope_success(
            out,
            CompletionsPayload {
                shell: args.shell,
                script,
            },
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_shell(s: &str) -> Result<Shell> {
    Shell::from_str(s, true).map_err(|_| Error::ConfigInvalid {
        message: format!(
            "unknown shell '{s}' (supported: {})",
            Shell::value_variants()
                .iter()
                .map(|v| v.to_possible_value().unwrap().get_name().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        location: None,
    })
}
