//! Append-only ledger of observations.
//!
//! Each observation records a single sighting of a recurring concern
//! ("developers keep getting the same null defect", "this rule was cited
//! by yet another spec"). Threshold-crossing aggregates surface via
//! [`super::PromotionCandidateFinder`].

use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::path_guard;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Observation {
    pub tag: String,
    pub text: String,
    /// Free-form source identifier (spec slug, file path, …).
    pub source: String,
    pub timestamp: Timestamp,
}

pub struct ObservationLedger {
    dir: PathBuf,
}

impl ObservationLedger {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn append(&self, tag: &str, text: &str, source: &str) -> Result<Observation> {
        let obs = Observation {
            tag: tag.to_string(),
            text: text.to_string(),
            source: source.to_string(),
            timestamp: Timestamp::now(),
        };
        let path = self
            .dir
            .join(format!("{}.jsonl", super::tag_filename_stem(tag)?));
        let line = serde_json::to_string(&obs).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;
        path_guard::append_line(&path, line.as_bytes())?;
        Ok(obs)
    }

    pub fn load_all(&self) -> Result<Vec<Observation>> {
        // Every failure to read surfaces rather than shortening the result:
        // a ledger read short undercounts observations and corrupts promotion
        // grouping, and one that could not be read at all is not one nobody
        // has written to. `try_exists` is what separates absent from
        // unreadable — `exists` answers false for both.
        let present = self.dir.try_exists().map_err(|e| Error::IoFailure {
            path: self.dir.clone(),
            source: e,
        })?;
        if !present {
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
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
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
                match serde_json::from_str::<Observation>(line) {
                    Ok(o) => out.push(o),
                    Err(e) => {
                        return Err(Error::LifecycleObservationCorrupt {
                            path: path.clone(),
                            message: format!("line {}: {e}", idx + 1),
                        });
                    }
                }
            }
        }
        Ok(out)
    }
}
