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

use crate::config::{Config, ConsumerDetectorDecl, KindDecl};
use crate::envelope::SkippedRule;
use crate::error::{Error, Result};
use crate::lifecycle::consumer::{ConsumerAsk, ConsumerDetector};
use crate::lifecycle::retirement::{
    RetirementClassifier, RetirementOutcome, SilenceState, last_changes, slug_of,
};
use crate::telemetry::TelemetryQuery;

/// Aggregate output of a sweep.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SweepOutcome {
    /// One verdict per scanned file, sorted by (severity, kind, slug).
    pub verdicts: Vec<RetirementOutcome>,
    /// Kinds that were actually walked.
    pub kinds_processed: Vec<String>,
    /// Kinds that were skipped, with the reason: foundation (and how many
    /// paths it carved out), no consumer detector, a detector that could not
    /// read its corpus, a glob that could not be read, or one match that
    /// could not be — a link whose target is gone.
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
        // One instant for the whole sweep: the silence window and every age
        // are measured back from it.
        let now = Timestamp::now();
        let classifier = RetirementClassifier::new(lc, self.config.retirement.as_ref(), now);

        let mut processed = Vec::new();
        let mut skipped = Vec::new();

        // Pass 1: resolve every classifiable artifact and its kind's consumer
        // pattern. Silence and consumers are each read once for the whole slug
        // set (see below), so classification waits for pass 2.
        struct Unit<'u> {
            kind: &'u str,
            /// The telemetry Kind recording this kind's invocations, if any.
            record: Option<&'u str>,
            detector_decl: &'u ConsumerDetectorDecl,
            entries: Vec<(PathBuf, String)>,
        }
        // A path a foundation kind names is foundation for every kind: the
        // declaration is what keeps a load-bearing artifact out of the sweep,
        // and a broader glob that also matches it must not put it back.
        let mut foundation: HashSet<PathBuf> = HashSet::new();
        for kind_decl in self.config.kinds.iter().filter(|k| k.foundation) {
            let Some(matches) = self.subjects(kind_decl, &mut skipped) else {
                continue;
            };
            // The skip says how much the declaration protected: a glob that
            // names nothing carves nothing out, and the file it was written
            // for is swept under whatever other kind matches it.
            let reason = match matches.len() {
                0 => {
                    "foundation kind: its glob names no path, so nothing is carved out".to_string()
                }
                n => format!("foundation kind (excluded from retirement): {n} path(s) carved out"),
            };
            skipped.push(SkippedRule {
                slug: kind_decl.name.clone(),
                reason,
            });
            foundation.extend(matches);
        }

        // The files the project owns are one set for every kind, listed once,
        // the first time a kind needs them.
        let mut detector: Option<std::result::Result<ConsumerDetector, String>> = None;
        let mut units: Vec<Unit<'_>> = Vec::new();
        for kind_decl in &self.config.kinds {
            if kind_decl.foundation {
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
            if detector.is_none() {
                detector = Some(match ConsumerDetector::new(self.working_dir) {
                    Ok(detector) => Ok(detector),
                    Err(Error::LifecycleGitFailure { message }) => Err(message),
                    Err(e) => return Err(e),
                });
            }
            // A corpus that cannot be listed measures no consumers, and each
            // kind says so. The explicit ask, `classify`, fails on the same
            // error.
            if let Some(Err(message)) = &detector {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: format!("consumers unmeasured: {message}"),
                });
                continue;
            }
            let Some(mut matches) = self.subjects(kind_decl, &mut skipped) else {
                continue;
            };
            matches.retain(|path| !foundation.contains(path));
            let entries = readable(kind_decl, matches, &mut skipped);
            units.push(Unit {
                kind: &kind_decl.name,
                record: kind_decl.invocation_kind.as_deref(),
                detector_decl,
                entries,
            });
            processed.push(kind_decl.name.clone());
        }

        // One ledger scan for the whole sweep, over every slug it will classify.
        let all_slugs: Vec<&str> = units
            .iter()
            .flat_map(|u| u.entries.iter().map(|(_, slug)| slug.as_str()))
            .collect();
        let records = self.invocations_in_window(now, &all_slugs)?;
        // One history walk, for when each of them last changed.
        let paths: Vec<PathBuf> = units
            .iter()
            .flat_map(|u| u.entries.iter().map(|(path, _)| path.clone()))
            .collect();
        let mut changes = last_changes(self.working_dir, &paths)?.into_iter();
        // And one read of the project's files, for what cites each of them.
        let asks: Vec<ConsumerAsk<'_>> = units
            .iter()
            .flat_map(|u| {
                u.entries.iter().map(|(_, slug)| ConsumerAsk {
                    decl: u.detector_decl,
                    slug,
                })
            })
            .collect();
        let mut consumers = match &detector {
            Some(Ok(detector)) => detector.find(&asks)?,
            // No kind was swept, so nothing was asked.
            Some(Err(_)) | None => Vec::new(),
        }
        .into_iter();

        // Pass 2: classify each artifact against its derived silence state.
        let mut verdicts = Vec::new();
        let mut files_classified = 0;
        for unit in &units {
            // The record for this kind, present only when it declared one and
            // that Kind recorded something in the window. Absent, silence is
            // unknowable for every slug of the kind — never presumed.
            let named = unit.record.and_then(|record| records.get(record));
            for (((path, slug), changed), cited_by) in unit
                .entries
                .iter()
                .zip(changes.by_ref())
                .zip(consumers.by_ref())
            {
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
                    classifier.classify(unit.kind, path, cited_by.len(), silence, changed)?;
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

    /// Every path a kind's glob names, or `None` when the glob itself could
    /// not be read — reported as a skip either way, so a kind that matched
    /// nothing is told apart from one the sweep could not see. A path is named
    /// whether or not it can be read: a link whose target is gone is still
    /// one a foundation kind carves out.
    fn subjects(
        &self,
        kind_decl: &KindDecl,
        skipped: &mut Vec<SkippedRule>,
    ) -> Option<Vec<PathBuf>> {
        // The project's own path is a literal, not a pattern — see
        // `glob_root`. Unescaped, a `[` in an ancestor directory empties
        // every kind's match list and retirement reads it as "nothing here".
        let Ok(pat_str) = crate::glob_root::rooted(self.working_dir, &kind_decl.glob) else {
            skipped.push(SkippedRule {
                slug: kind_decl.name.clone(),
                reason: "kind glob path is not valid UTF-8".into(),
            });
            return None;
        };
        let glob_iter = match glob::glob(&pat_str) {
            Ok(it) => it,
            Err(e) => {
                skipped.push(SkippedRule {
                    slug: kind_decl.name.clone(),
                    reason: format!("glob '{}' invalid: {e}", kind_decl.glob),
                });
                return None;
            }
        };
        let mut matches = Vec::new();
        for entry in glob_iter {
            match entry {
                Ok(path) => matches.push(path),
                Err(e) => unreadable(kind_decl, e.path(), &e.error().to_string(), skipped),
            }
        }
        Some(matches)
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
    fn invocations_in_window(
        &self,
        now: Timestamp,
        slugs: &[&str],
    ) -> Result<HashMap<String, HashSet<String>>> {
        let declared: HashSet<&str> = self
            .config
            .kinds
            .iter()
            .filter_map(|k| k.invocation_kind.as_deref())
            .collect();
        if declared.is_empty() {
            return Ok(HashMap::new());
        }
        let cutoff = now - SignedDuration::from_hours((self.silence_window_days as i64) * 24);
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

/// The matches a kind can classify, each with its slug. One that cannot be
/// read — a link whose target is gone among them — is neither dropped, which
/// would treat a present artifact as absent, nor the end of the sweep, which
/// would leave every other unjudged: it is declared a skip.
fn readable(
    kind_decl: &KindDecl,
    matches: Vec<PathBuf>,
    skipped: &mut Vec<SkippedRule>,
) -> Vec<(PathBuf, String)> {
    let mut entries = Vec::new();
    for path in matches {
        match std::fs::metadata(&path) {
            Ok(_) => {
                let slug = slug_of(&path);
                entries.push((path, slug));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && path.is_symlink() => {
                unreadable(
                    kind_decl,
                    &path,
                    "a link to a path that does not exist",
                    skipped,
                );
            }
            Err(e) => unreadable(kind_decl, &path, &e.to_string(), skipped),
        }
    }
    entries
}

fn unreadable(kind_decl: &KindDecl, path: &Path, error: &str, skipped: &mut Vec<SkippedRule>) {
    skipped.push(SkippedRule {
        slug: kind_decl.name.clone(),
        reason: format!(
            "kind '{}' match unreadable ({}): {error}",
            kind_decl.name,
            path.display()
        ),
    });
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
