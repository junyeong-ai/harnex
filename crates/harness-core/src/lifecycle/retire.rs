//! Retirement sweep — walk every kind × consumer detector × glob,
//! classify each file under the three retirement signals.
//!
//! The silence state is derived from one scan of the invocation record a
//! project declares (`[lifecycle] invocation_kind`) over the configured
//! `silence_window_days` — the deterministic alternative to operators
//! asserting `--silence` by hand. A window holding no invocation, or a
//! project declaring no Kind that records one, yields `Unmeasured`: silence
//! is never fabricated from a ledger with nothing to be absent from.

use std::collections::HashSet;
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
    invocation_kind: Option<&'a str>,
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
            invocation_kind: lc.invocation_kind.as_deref(),
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
        type Unit = (String, Box<dyn ConsumerDetector>, Vec<(PathBuf, String)>);
        let mut units: Vec<Unit> = Vec::new();
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
            units.push((kind_decl.name.clone(), detector, entries));
            processed.push(kind_decl.name.clone());
        }

        // One ledger scan for the whole sweep, over every slug it will classify.
        let all_slugs: Vec<&str> = units
            .iter()
            .flat_map(|(_, _, entries)| entries.iter().map(|(_, slug)| slug.as_str()))
            .collect();
        let invoked = self.invocations_in_window(&all_slugs)?;

        // Pass 2: classify each artifact against its derived silence state.
        let mut verdicts = Vec::new();
        let mut files_classified = 0;
        for (kind, detector, entries) in &units {
            for (path, slug) in entries {
                // An empty slug (a file with no stem) names nothing an
                // invocation could record, so its silence is unknowable.
                let silence = match &invoked {
                    Some(invoked) if !slug.is_empty() => {
                        if invoked.contains(slug.as_str()) {
                            SilenceState::Active
                        } else {
                            SilenceState::Silent
                        }
                    }
                    _ => SilenceState::Unmeasured,
                };
                let verdict = classifier.classify(kind, path, detector.as_ref(), silence)?;
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

    /// Read the invocation record for the window: `None` when there is none to
    /// read, otherwise the slugs it names.
    ///
    /// Silence is a claim about invocations, so it is measured against the one
    /// Kind a project declares as its invocation record
    /// (`[lifecycle] invocation_kind`) — never inferred from ledger traffic at
    /// large. A payload of an unrelated Kind may carry any string, so reading
    /// one as an invocation would make a slug's fate turn on a coincidence:
    /// `{"area": "policy"}` would both revive the rule named `policy` and
    /// convict every other rule of silence. Narrowing to the declared Kind
    /// makes the measurement exact rather than probable, and leaves the field
    /// naming to the project, which a brownfield ledger needs.
    ///
    /// `None` — no declared Kind, or no event of it within the window — means
    /// the window records no invocation at all, so nothing can be absent from
    /// it and every slug is [`SilenceState::Unmeasured`].
    fn invocations_in_window(&self, slugs: &[&str]) -> Result<Option<HashSet<String>>> {
        let Some(kind) = &self.invocation_kind else {
            return Ok(None);
        };
        let cutoff =
            Timestamp::now() - SignedDuration::from_hours((self.silence_window_days as i64) * 24);
        let mut measured = false;
        let mut invoked = HashSet::new();
        self.telemetry.scan_events(&mut |event| {
            if &event.kind != kind || event.timestamp < cutoff {
                return;
            }
            measured = true;
            for slug in slugs {
                if !slug.is_empty()
                    && !invoked.contains(*slug)
                    && json_contains_string_exact(&event.payload, slug)
                {
                    invoked.insert((*slug).to_string());
                }
            }
        })?;
        Ok(measured.then_some(invoked))
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
