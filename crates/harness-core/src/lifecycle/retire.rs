//! Retirement sweep — walk every kind × consumer detector × glob,
//! classify each file under the three retirement signals.
//!
//! The silence state is derived from one scan of the invocation records the
//! kinds declare (`[[kinds]] invocation_kind`) over the configured
//! `silence_window_days` — the deterministic alternative to operators
//! asserting `--silence` by hand. A kind declaring no record, or one whose
//! record holds nothing in the window, yields `Unmeasured` for its every
//! slug: silence is never fabricated from a ledger that could not have
//! named the artifact in the first place.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::envelope::SkippedRule;
use crate::error::{Error, Result};
use crate::lifecycle::consumer::{ConsumerDetector, consumer_detector_for};
use crate::lifecycle::retirement::{RetirementClassifier, RetirementOutcome, SilenceState};
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

        let mut processed = Vec::new();
        let mut skipped = Vec::new();

        // Pass 1: resolve every classifiable artifact and its per-kind consumer
        // detector. The silence state needs the whole slug set before any single
        // verdict (see the scan below), so classification waits for pass 2.
        struct Unit<'u> {
            kind: &'u str,
            /// The telemetry Kind recording this kind's invocations, if any.
            record: Option<&'u str>,
            detector: Box<dyn ConsumerDetector>,
            entries: Vec<(PathBuf, String)>,
        }
        let mut units: Vec<Unit<'_>> = Vec::new();
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

            // The project's own path is a literal, not a pattern — see
            // `glob_root`. Unescaped, a `[` in an ancestor directory empties
            // every kind's match list and retirement reads it as "nothing here".
            let Ok(pat_str) = crate::glob_root::rooted(self.working_dir, &kind_decl.glob) else {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: "kind glob path is not valid UTF-8".into(),
                });
                continue;
            };
            let glob_iter = match glob::glob(&pat_str) {
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
            let mut entries: Vec<(PathBuf, String)> = Vec::new();
            for entry in glob_iter {
                match entry {
                    Ok(p) => {
                        let slug = p
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        entries.push((p, slug));
                    }
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
            units.push(Unit {
                kind: &kind_decl.name,
                record: kind_decl.invocation_kind.as_deref(),
                detector,
                entries,
            });
            processed.push(kind_decl.name.clone());
        }

        // One ledger scan for the whole sweep, over every slug it will classify.
        let all_slugs: Vec<&str> = units
            .iter()
            .flat_map(|u| u.entries.iter().map(|(_, slug)| slug.as_str()))
            .collect();
        let records = self.invocations_in_window(&all_slugs)?;

        // Pass 2: classify each artifact against its derived silence state.
        let mut verdicts = Vec::new();
        let mut files_classified = 0;
        for unit in &units {
            // The record for this kind, present only when it declared one and
            // that Kind recorded something in the window. Absent, silence is
            // unknowable for every slug of the kind — never presumed.
            let named = unit.record.and_then(|record| records.get(record));
            for (path, slug) in &unit.entries {
                // An empty slug (a file with no stem) names nothing an
                // invocation could record, so its silence is unknowable too.
                let silence = match named {
                    Some(named) if !slug.is_empty() => {
                        if named.contains(slug.as_str()) {
                            SilenceState::Active
                        } else {
                            SilenceState::Silent
                        }
                    }
                    _ => SilenceState::Unmeasured,
                };
                let verdict =
                    classifier.classify(unit.kind, path, unit.detector.as_ref(), silence)?;
                verdicts.push(verdict);
                files_classified += 1;
            }
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

    /// Read every declared invocation record over the window, as a map from
    /// the telemetry Kind to the slugs its events name. A Kind absent from the
    /// map recorded nothing in the window.
    ///
    /// Silence is a claim about invocations, so it is measured only against
    /// the Kind a project declares as the record of that artifact class
    /// (`[[kinds]] invocation_kind`) — never inferred from ledger traffic at
    /// large. Two things would otherwise be guessed. Any payload may carry any
    /// string, so reading an unrelated Kind as an invocation would make a
    /// slug's fate turn on a coincidence: `{"area": "policy"}` would both
    /// revive the rule named `policy` and convict every other rule. And a
    /// record names one class of artifact, so reading it for another convicts
    /// that whole class: an invocation record naming skills can never name a
    /// rule, which is loaded rather than invoked, so one skill call would
    /// retire every rule in the project. Declaring the record per kind decides
    /// both, and leaves payload shape to the project, which a brownfield
    /// ledger needs.
    fn invocations_in_window(&self, slugs: &[&str]) -> Result<HashMap<String, HashSet<String>>> {
        let declared: HashSet<&str> = self
            .config
            .kinds
            .iter()
            .filter_map(|k| k.invocation_kind.as_deref())
            .collect();
        if declared.is_empty() {
            return Ok(HashMap::new());
        }
        let cutoff =
            Timestamp::now() - SignedDuration::from_hours((self.silence_window_days as i64) * 24);
        let mut records: HashMap<String, HashSet<String>> = HashMap::new();
        self.telemetry.scan_events(&mut |event| {
            if event.timestamp < cutoff || !declared.contains(event.kind.as_str()) {
                return;
            }
            let named = records.entry(event.kind.clone()).or_default();
            for slug in slugs {
                if !slug.is_empty()
                    && !named.contains(*slug)
                    && json_contains_string_exact(&event.payload, slug)
                {
                    named.insert((*slug).to_string());
                }
            }
        })?;
        Ok(records)
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
