//! # Telemetry: append-only closed-schema event ledger
//!
//! Projects declare a fixed set of Kinds in `[[telemetry.kinds]]` with
//! `payload_schema`. Appends validate against the schema at write time —
//! unknown fields, missing required fields, type mismatches, and enum
//! violations all fail synchronously.
//!
//! [`TelemetryQuery::report`] aggregates per-Kind activity over
//! configurable trailing windows. The report feeds retirement decisions
//! (low / zero activity Kinds surface as silent candidates) and
//! operator dashboards.
//!
//! ## What this module refuses to do
//!
//! - Never accept payload fields outside the declared `payload_schema`.
//! - Never silently rotate or delete records.
//! - Never call a network sink.

pub mod jsonl;
pub mod kind;

use std::collections::{BTreeMap, HashMap};

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::config::TelemetryConfig;
use crate::error::{Error, Result};

pub use jsonl::JsonlStorage;
pub use kind::KindSchema;

/// Closed set of telemetry storage backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Jsonl,
}

impl StorageKind {
    pub const ALL: &'static [Self] = &[Self::Jsonl];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "jsonl" => Self::Jsonl,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
        }
    }
}

/// A single recorded event.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Event {
    pub kind: String,
    pub timestamp: Timestamp,
    pub payload: serde_json::Value,
}

/// Write-side: validates payload against declared schema, then appends
/// through the [`JsonlStorage`] backend.
pub struct TelemetryAppender {
    schemas: HashMap<String, KindSchema>,
    storage: JsonlStorage,
}

impl TelemetryAppender {
    pub fn new(cfg: &TelemetryConfig, storage: JsonlStorage) -> Result<Self> {
        let mut schemas = HashMap::new();
        for k in &cfg.kinds {
            schemas.insert(k.name.clone(), KindSchema::from_value(&k.payload_schema)?);
        }
        Ok(Self { schemas, storage })
    }

    pub fn append(&mut self, kind: &str, payload: serde_json::Value) -> Result<Event> {
        let schema = self
            .schemas
            .get(kind)
            .ok_or_else(|| Error::TelemetryKindUnknown {
                kind: kind.to_string(),
            })?;
        schema.validate(&payload)?;
        let event = Event {
            kind: kind.to_string(),
            timestamp: Timestamp::now(),
            payload,
        };
        self.storage.append(&event)?;
        Ok(event)
    }
}

/// Default windows for `report` (trailing-day buckets): 1d, 7d, 30d, 90d.
pub const DEFAULT_REPORT_WINDOWS: &[u32] = &[1, 7, 30, 90];

/// Per-Kind activity rollup.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct KindSummary {
    pub kind: String,
    pub total: usize,
    pub first_seen: Option<Timestamp>,
    pub last_seen: Option<Timestamp>,
    /// Map from trailing window (days) → event count within the window.
    pub last_n_days: BTreeMap<u32, usize>,
}

/// Aggregated report across all Kinds (or a single kind when filtered).
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TelemetrySummary {
    /// Sorted by `kind` ascending for deterministic output.
    pub kinds: Vec<KindSummary>,
    /// The trailing-day windows used (mirrored from `report` arg).
    pub windows: Vec<u32>,
}

/// Read-side: aggregate queries over the ledger.
pub struct TelemetryQuery {
    storage: JsonlStorage,
}

impl TelemetryQuery {
    pub fn new(storage: JsonlStorage) -> Self {
        Self { storage }
    }

    /// Visit every event in insertion order. Used by analyses that
    /// need to inspect payload contents (e.g., retirement Silent
    /// derivation scanning for slug string occurrences).
    pub fn scan_events(&self, visitor: &mut dyn FnMut(&Event)) -> Result<()> {
        self.storage.scan(visitor)
    }

    /// Count events of `kind`, optionally restricted to `>= since`.
    pub fn count(&self, kind: &str, since: Option<Timestamp>) -> Result<usize> {
        let mut n = 0usize;
        self.storage.scan(&mut |event| {
            if event.kind == kind {
                match since {
                    Some(s) if event.timestamp < s => {}
                    _ => n += 1,
                }
            }
        })?;
        Ok(n)
    }

    /// Aggregate per-Kind activity over the provided trailing-day windows.
    /// If `kind_filter` is Some, restrict to that single Kind.
    pub fn report(
        &self,
        windows: &[u32],
        kind_filter: Option<&str>,
    ) -> Result<TelemetrySummary> {
        let now = Timestamp::now();
        let mut accumulator: HashMap<String, KindAccumulator> = HashMap::new();

        self.storage.scan(&mut |event| {
            if let Some(k) = kind_filter
                && event.kind != k
            {
                return;
            }
            let acc = accumulator
                .entry(event.kind.clone())
                .or_insert_with(KindAccumulator::default);
            acc.total += 1;
            acc.first = match acc.first {
                Some(t) if t <= event.timestamp => Some(t),
                _ => Some(event.timestamp),
            };
            acc.last = match acc.last {
                Some(t) if t >= event.timestamp => Some(t),
                _ => Some(event.timestamp),
            };
            for &w in windows {
                let cutoff = now - SignedDuration::from_hours((w as i64) * 24);
                if event.timestamp >= cutoff {
                    *acc.windows.entry(w).or_insert(0) += 1;
                }
            }
        })?;

        let mut kinds: Vec<KindSummary> = accumulator
            .into_iter()
            .map(|(name, acc)| {
                let mut last_n_days = BTreeMap::new();
                for &w in windows {
                    last_n_days.insert(w, *acc.windows.get(&w).unwrap_or(&0));
                }
                KindSummary {
                    kind: name,
                    total: acc.total,
                    first_seen: acc.first,
                    last_seen: acc.last,
                    last_n_days,
                }
            })
            .collect();
        kinds.sort_by(|a, b| a.kind.cmp(&b.kind));

        Ok(TelemetrySummary {
            kinds,
            windows: windows.to_vec(),
        })
    }
}

#[derive(Default)]
struct KindAccumulator {
    total: usize,
    first: Option<Timestamp>,
    last: Option<Timestamp>,
    windows: HashMap<u32, usize>,
}
