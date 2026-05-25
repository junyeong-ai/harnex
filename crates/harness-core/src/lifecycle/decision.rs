//! Append-only decision ledger.
//!
//! When the operator (a human, never the model) acts on a promotion
//! candidate, the toolkit records the decision here. Subsequent
//! `list_candidates` calls exclude any `(tag, normalized_text)` already
//! marked `Approved`, `Rejected`, or `Demoted` so the candidate set
//! converges instead of regenerating noise. `Deferred` decisions are
//! informational and keep surfacing.

use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::path_guard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PromotionDecision {
    /// Pattern promoted to a rule. Excluded from future candidate surfacing.
    Approved,
    /// Pattern excluded from future candidate surfacing.
    Rejected,
    /// Suspended; keeps surfacing (informational) until re-decided.
    Deferred,
    /// Previously approved, now retracted. Excluded from future surfacing.
    Demoted,
}

impl PromotionDecision {
    pub const ALL: &'static [Self] = &[
        Self::Approved,
        Self::Rejected,
        Self::Deferred,
        Self::Demoted,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "deferred" => Self::Deferred,
            "demoted" => Self::Demoted,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Deferred => "deferred",
            Self::Demoted => "demoted",
        }
    }

    /// Whether this decision suppresses future candidate surfacing.
    ///
    /// Exhaustive match — adding a new [`PromotionDecision`] variant forces
    /// this function to update, preventing silent "default = false" drift.
    pub fn suppresses_resurfacing(self) -> bool {
        match self {
            Self::Approved | Self::Rejected | Self::Demoted => true,
            Self::Deferred => false,
        }
    }
}

#[cfg(test)]
mod strategy_tests {
    use super::PromotionDecision;

    #[test]
    fn from_str_round_trips_every_variant() {
        for d in PromotionDecision::ALL {
            assert_eq!(PromotionDecision::from_str(d.as_str()), Some(*d));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(PromotionDecision::from_str("nope"), None);
    }

    #[test]
    fn as_str_matches_serde_kebab_case() {
        // serde_json emits the same kebab-case strings as as_str — the
        // CLI value_parser (via decision_kind_values) and the ledger
        // serialization MUST agree on the wire representation.
        for d in PromotionDecision::ALL {
            let json = serde_json::to_string(d).unwrap();
            // serde quotes the variant: "\"approved\""
            assert_eq!(json, format!("\"{}\"", d.as_str()));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DecisionRecord {
    pub tag: String,
    pub normalized_text: String,
    pub decision: PromotionDecision,
    /// Human-authored rationale. Must be non-empty.
    pub decision_text: String,
    pub timestamp: Timestamp,
}

pub struct DecisionLedger {
    dir: PathBuf,
}

impl DecisionLedger {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn append(&self, record: &DecisionRecord) -> Result<()> {
        let path = self
            .dir
            .join(format!("{}.jsonl", super::tag_filename_stem(&record.tag)));
        let line = serde_json::to_string(record).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;
        path_guard::append_line(&path, line.as_bytes())?;
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<DecisionRecord>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&self.dir).map_err(|e| Error::IoFailure {
            path: self.dir.clone(),
            source: e,
        })?;
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .collect();
        paths.sort();
        for path in paths {
            let content = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            for (idx, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<DecisionRecord>(line) {
                    Ok(r) => out.push(r),
                    Err(e) => {
                        return Err(Error::LifecycleObservationCorrupt {
                            path: path.clone(),
                            message: format!("decision line {}: {e}", idx + 1),
                        });
                    }
                }
            }
        }
        Ok(out)
    }
}
