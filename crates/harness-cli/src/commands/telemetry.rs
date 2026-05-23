use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use jiff::Timestamp;
use serde::Serialize;

use harness_core::config::Config;
use harness_core::error::{Error, Result};

use super::write_envelope_success;
use harness_core::telemetry::{
    DEFAULT_REPORT_WINDOWS, JsonlStorage, TelemetryAppender, TelemetryQuery,
};

#[derive(Subcommand)]
pub enum TelemetryCommand {
    /// Append a typed event to the ledger
    Append {
        /// Kind name (must be declared in [[telemetry.kinds]])
        #[arg(long)]
        kind: String,
        /// JSON payload matching the kind's payload_schema
        #[arg(long)]
        payload: String,
    },
    /// Count events of a given kind
    Count {
        #[arg(long)]
        kind: String,
        /// Restrict to events at or after this RFC 3339 timestamp
        #[arg(long)]
        since: Option<Timestamp>,
    },
    /// Per-Kind aggregate (total + first/last + trailing-window counts)
    Report {
        /// Optional Kind filter; omit to report on every declared Kind
        #[arg(long)]
        kind: Option<String>,
        /// Trailing-day windows (e.g., 1 7 30 90). Default: 1 7 30 90.
        #[arg(long, num_args = 1.., value_delimiter = ',')]
        window: Option<Vec<u32>>,
    },
}

pub fn run<W: Write>(cmd: TelemetryCommand, out: &mut W) -> Result<ExitCode> {
    let working_dir = std::env::current_dir().map_err(|e| Error::IoFailure {
        path: PathBuf::from("."),
        source: e,
    })?;
    let (config, config_path) = Config::load(&working_dir)?;
    let tcfg = config
        .telemetry
        .as_ref()
        .ok_or_else(|| Error::ConfigInvalid {
            message: "no [telemetry] section in harness.toml".into(),
            location: None,
        })?;

    let config_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| working_dir.clone());
    let storage_dir = if tcfg.storage_dir.is_absolute() {
        tcfg.storage_dir.clone()
    } else {
        config_dir.join(&tcfg.storage_dir)
    };

    match cmd {
        TelemetryCommand::Append { kind, payload } => {
            let storage = JsonlStorage::new(storage_dir, tcfg.rotate_at_mb);
            let mut appender = TelemetryAppender::new(tcfg, storage)?;
            let payload_value: serde_json::Value =
                serde_json::from_str(&payload).map_err(|e| Error::TelemetryPayloadInvalid {
                    message: format!("payload is not valid JSON: {e}"),
                })?;
            let event = appender.append(&kind, payload_value)?;
            write_envelope_success(out, event)?;
            Ok(ExitCode::SUCCESS)
        }
        TelemetryCommand::Count { kind, since } => {
            let storage = JsonlStorage::new(storage_dir, tcfg.rotate_at_mb);
            let query = TelemetryQuery::new(storage);
            let count = query.count(&kind, since)?;

            #[derive(Serialize)]
            struct CountResponse {
                kind: String,
                count: usize,
            }
            write_envelope_success(out, CountResponse { kind, count })?;
            Ok(ExitCode::SUCCESS)
        }
        TelemetryCommand::Report { kind, window } => {
            let storage = JsonlStorage::new(storage_dir, tcfg.rotate_at_mb);
            let query = TelemetryQuery::new(storage);
            let windows = window
                .as_deref()
                .unwrap_or(DEFAULT_REPORT_WINDOWS)
                .to_vec();
            let summary = query.report(&windows, kind.as_deref())?;
            write_envelope_success(out, summary)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
