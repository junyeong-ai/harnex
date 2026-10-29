//! Append-only decision ledger.
//!
//! When the operator (a human, never the model) acts on a promotion
//! candidate, the toolkit records the decision here. Subsequent
//! `survey` calls exclude any `(tag, normalized_text)` already
//! marked `Approved`, `Rejected`, or `Demoted` so the candidate set
//! converges instead of regenerating noise. `Deferred` decisions are
//! informational and keep surfacing.

use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::path_guard;
use crate::wire_enum::wire_enum;

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum PromotionDecision {
        /// Pattern promoted to a rule. Excluded from future candidate surfacing.
        Approved => "approved",
        /// Pattern excluded from future candidate surfacing.
        Rejected => "rejected",
        /// Suspended; keeps surfacing (informational) until re-decided.
        Deferred => "deferred",
        /// Previously approved, now retracted. Excluded from future surfacing.
        Demoted => "demoted",
    }
}

impl PromotionDecision {
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
mod decision_tests {
    use super::PromotionDecision;

    #[test]
    fn promotion_decision_spells_itself_the_same_way_twice() {
        // The CLI value_parser reads `as_str`; the ledger serializes through
        // serde.
        for variant in PromotionDecision::ALL {
            assert_eq!(
                serde_json::to_string(variant).unwrap(),
                format!("{:?}", variant.as_str()),
                "`rename_all` and the wire string are two spellings of one name"
            );
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
            .join(format!("{}.jsonl", super::tag_filename_stem(&record.tag)?));
        let line = serde_json::to_string(record).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;
        path_guard::append_line(&path, line.as_bytes())?;
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<DecisionRecord>> {
        // Every failure to read surfaces rather than shortening the result: a
        // ledger read short loses terminal verdicts from the suppression set,
        // resurfacing already-resolved candidates and corrupting the demote
        // precondition, and one that could not be read at all is not one
        // holding no decisions.
        if !super::ledger_dir_present(&self.dir)? {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&self.dir).map_err(|e| Error::IoFailure {
            path: self.dir.clone(),
            source: e,
        })?;
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::IoFailure {
                path: self.dir.clone(),
                source: e,
            })?;
            let p = entry.path();
            if super::is_ledger_file(&p) {
                paths.push(p);
            }
        }
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
