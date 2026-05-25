//! Retirement sweep — walk every kind × consumer detector × glob,
//! classify each file under the three retirement signals.
//!
//! The `Silent` signal is derived automatically by scanning the
//! telemetry ledger for events whose payload contains the slug as a
//! string value within the configured `silence_window_days`. This is
//! the deterministic alternative to operators passing `--silent` by hand.

use std::path::{Path, PathBuf};

use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::envelope::SkippedRule;
use crate::error::{Error, Result};
use crate::lifecycle::consumer::consumer_detector_for;
use crate::lifecycle::retirement::{RetirementClassifier, RetirementOutcome};
use crate::telemetry::TelemetryQuery;

/// Aggregate output of a sweep.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SweepOutcome {
    /// One verdict per scanned file, sorted by (severity, kind, slug).
    pub verdicts: Vec<RetirementOutcome>,
    /// Kinds that were actually walked.
    pub kinds_processed: Vec<String>,
    /// Kinds that were skipped, with the reason (foundation, no detector,
    /// or empty glob match).
    pub kinds_skipped: Vec<SkippedRule>,
    /// Total files classified.
    pub files_classified: usize,
}

pub struct RetirementSweeper<'a> {
    config: &'a Config,
    working_dir: &'a Path,
    telemetry: &'a TelemetryQuery,
    silence_window_days: u32,
}

impl<'a> RetirementSweeper<'a> {
    pub fn new(
        config: &'a Config,
        working_dir: &'a Path,
        telemetry: &'a TelemetryQuery,
    ) -> Result<Self> {
        let lc = config
            .lifecycle
            .as_ref()
            .ok_or_else(|| Error::ConfigInvalid {
                message: "no [lifecycle] section in harness.toml".into(),
                location: None,
            })?;
        Ok(Self {
            config,
            working_dir,
            telemetry,
            silence_window_days: lc.silence_window_days,
        })
    }

    /// Override the silence window for this sweep (defaults to
    /// `[lifecycle].silence_window_days`).
    pub fn with_silence_window(mut self, days: u32) -> Self {
        self.silence_window_days = days;
        self
    }

    pub fn run(&self) -> Result<SweepOutcome> {
        let lc = self.config.lifecycle.as_ref().unwrap();
        let classifier = RetirementClassifier::new(lc, self.config.retirement.as_ref());

        let mut verdicts = Vec::new();
        let mut processed = Vec::new();
        let mut skipped = Vec::new();
        let mut files_classified = 0;

        for kind_decl in &self.config.kinds {
            if kind_decl.foundation {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: "foundation kind (excluded from retirement)".into(),
                });
                continue;
            }
            let Some(detector_decl) = lc
                .consumer_detectors
                .iter()
                .find(|d| d.kind == kind_decl.name)
            else {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: "no [[lifecycle.consumer_detectors]] for this kind".into(),
                });
                continue;
            };
            let detector = consumer_detector_for(detector_decl.clone(), self.working_dir)?;

            let pattern = self.working_dir.join(&kind_decl.glob);
            let Some(pat_str) = pattern.to_str() else {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: "kind glob path is not valid UTF-8".into(),
                });
                continue;
            };
            let glob_iter = match glob::glob(pat_str) {
                Ok(it) => it,
                Err(e) => {
                    skipped.push(SkippedRule {
                        slug: kind_decl.name.clone(),
                        reason: format!("glob '{}' invalid: {e}", kind_decl.glob),
                    });
                    continue;
                }
            };
            // An unreadable match (e.g. permission-denied during traversal)
            // must NOT be dropped: a silently skipped artifact could escape
            // classification and be treated as if it does not exist. Record
            // each unreadable entry as a skip so the gap is visible.
            let mut entries: Vec<PathBuf> = Vec::new();
            for entry in glob_iter {
                match entry {
                    Ok(p) => entries.push(p),
                    Err(e) => skipped.push(SkippedRule {
                        slug: kind_decl.name.clone(),
                        reason: format!(
                            "kind '{}' match unreadable ({}): {}",
                            kind_decl.name,
                            e.path().display(),
                            e.error()
                        ),
                    }),
                }
            }

            for path in entries {
                let slug = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let silent = self.is_slug_silent(&slug)?;
                let verdict =
                    classifier.classify(&kind_decl.name, &path, detector.as_ref(), silent)?;
                verdicts.push(verdict);
                files_classified += 1;
            }
            processed.push(kind_decl.name.clone());
        }

        // Sort: actionable items first within each severity band.
        // (1) exempt asc — non-exempt (`false`) precedes exempt (`true`)
        //     so triage focus lands on actionable verdicts.
        // (2) severity asc — most severe first.
        // (3) kind / slug — deterministic tiebreaker.
        verdicts.sort_by(|a, b| {
            a.exempt
                .cmp(&b.exempt)
                .then_with(|| a.severity.rank().cmp(&b.severity.rank()))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.slug.cmp(&b.slug))
        });
        processed.sort();
        skipped.sort_by(|a, b| a.slug.cmp(&b.slug));

        Ok(SweepOutcome {
            verdicts,
            kinds_processed: processed,
            kinds_skipped: skipped,
            files_classified,
        })
    }

    /// A slug is Silent when no telemetry event within the silence window
    /// contains the slug as a string value anywhere in its payload.
    fn is_slug_silent(&self, slug: &str) -> Result<bool> {
        if slug.is_empty() {
            return Ok(true);
        }
        let cutoff =
            Timestamp::now() - SignedDuration::from_hours((self.silence_window_days as i64) * 24);
        let mut found = false;
        self.telemetry.scan_events(&mut |event| {
            if found || event.timestamp < cutoff {
                return;
            }
            if json_contains_string_exact(&event.payload, slug) {
                found = true;
            }
        })?;
        Ok(!found)
    }
}

fn json_contains_string_exact(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(s) => s == needle,
        Value::Array(arr) => arr.iter().any(|v| json_contains_string_exact(v, needle)),
        Value::Object(obj) => obj.values().any(|v| json_contains_string_exact(v, needle)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_contains_finds_in_nested() {
        let v: Value = serde_json::json!({
            "outer": {"skill": "my-skill", "other": [1, 2, "nope"]},
            "list": [{"k": "my-skill"}]
        });
        assert!(json_contains_string_exact(&v, "my-skill"));
        assert!(!json_contains_string_exact(&v, "absent"));
    }

    #[test]
    fn json_contains_is_exact_match_not_substring() {
        let v: Value = serde_json::json!({"x": "my-skill-extended"});
        assert!(!json_contains_string_exact(&v, "my-skill"));
        assert!(json_contains_string_exact(&v, "my-skill-extended"));
    }
}
