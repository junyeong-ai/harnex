//! Three-signal retirement classifier.
//!
//! - `Stale`: file mtime older than `stale_days`
//! - `NoConsumers`: ConsumerDetector finds zero referencing files
//! - `Silent`: zero telemetry events mentioning the slug within
//!   `silence_window_days` (caller computes; passes in as a bool)

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Serialize;

use crate::config::{LifecycleConfig, RetirementConfig};
use crate::envelope::Severity;
use crate::error::{Error, Result};
use crate::lifecycle::consumer::ConsumerDetector;

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RetirementOutcome {
    pub kind: String,
    pub slug: String,
    pub path: PathBuf,
    pub age_days: i64,
    pub consumer_count: usize,
    pub silent: bool,
    pub signals: Vec<RetirementSignal>,
    pub severity: Severity,
    pub exempt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RetirementSignal {
    Stale,
    NoConsumers,
    Silent,
}

pub struct RetirementClassifier<'a> {
    config: &'a LifecycleConfig,
    retirement: Option<&'a RetirementConfig>,
}

impl<'a> RetirementClassifier<'a> {
    pub fn new(config: &'a LifecycleConfig, retirement: Option<&'a RetirementConfig>) -> Self {
        Self { config, retirement }
    }

    pub fn classify(
        &self,
        kind: &str,
        path: &Path,
        consumer: &dyn ConsumerDetector,
        silent: bool,
    ) -> Result<RetirementOutcome> {
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let metadata = std::fs::metadata(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mtime = metadata.modified().map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mtime_ts = Timestamp::try_from(mtime).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: std::io::Error::other(format!("modified-time conversion: {e}")),
        })?;
        let now = Timestamp::now();
        let elapsed_secs = now.duration_since(mtime_ts).as_secs();
        let age_days = (elapsed_secs / 86400).max(0);
        let in_grace = age_days < (self.config.grace_period_days as i64);

        let consumers = consumer.find_consumers(&slug)?;
        let consumer_count = consumers.len();

        let mut signals = Vec::new();
        if age_days > (self.config.stale_days as i64) {
            signals.push(RetirementSignal::Stale);
        }
        if consumer_count == 0 {
            signals.push(RetirementSignal::NoConsumers);
        }
        if silent {
            signals.push(RetirementSignal::Silent);
        }

        let severity = match signals.len() {
            3 => Severity::Major,
            2 => Severity::Minor,
            _ => Severity::Info,
        };

        let exempt = in_grace
            || self
                .retirement
                .map(|r| {
                    r.exempt.kinds.iter().any(|k| k == kind)
                        || r.exempt.slugs.iter().any(|s| s == &slug)
                })
                .unwrap_or(false);

        Ok(RetirementOutcome {
            kind: kind.to_string(),
            slug,
            path: path.to_path_buf(),
            age_days,
            consumer_count,
            silent,
            signals,
            severity,
            exempt,
        })
    }
}
