pub mod audit;
pub mod check;
pub mod codegen;
pub mod completions;
pub mod evidence;
pub mod export;
pub mod graph;
pub mod guard;
pub mod lifecycle;
pub mod policy;
pub mod session;
pub mod telemetry;
pub mod validate;

use std::io::Write;
use std::path::PathBuf;

use harness_core::config::Config;
use harness_core::error::{Error, Result};

pub fn load_config() -> Result<(Config, PathBuf, PathBuf)> {
    let working_dir = std::env::current_dir().map_err(|e| Error::IoFailure {
        path: PathBuf::from("."),
        source: e,
    })?;
    let (config, config_path) = Config::load(&working_dir)?;
    Ok((config, config_path, working_dir))
}

pub fn config_dir(config_path: &std::path::Path, working_dir: &std::path::Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| working_dir.to_path_buf())
}

/// Emit the success envelope, carrying whatever the binary must disclose
/// about itself. Spec-vocabulary staleness rides here rather than any
/// command's findings: it describes this build's knowledge, not the project
/// the command was pointed at, and every consumer sees it on every call.
pub fn write_envelope_success<T: serde::Serialize, W: Write>(out: &mut W, data: T) -> Result<()> {
    let warnings = harness_core::spec::stale_warnings_now();
    harness_core::envelope::write_success(out, data, &warnings).map_err(|e| Error::IoFailure {
        path: PathBuf::from("(stdout)"),
        source: e,
    })
}
