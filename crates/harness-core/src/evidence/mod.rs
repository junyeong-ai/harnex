//! # Claim evidence verifier
//!
//! Parses markdown for provenance-marked claims and verifies each against
//! a configured [`Verifier`]. Strategies are pluggable; built-in strategies
//! cover `file-path-line`, `context7`, `fetched-url`, and `memory-only`.
//!
//! ## What this module refuses to do
//!
//! - Never make a network call at verify time. URL-based strategies check
//!   format + recorded fetch timestamp only. The actual fetch is the
//!   author's job; the provenance marker is a contract, not an oracle.
//! - Never invent provenance. Unmarked free text is not a claim; every claim
//!   carries one of the bracketed markers [`parse_claims`] recognises, the
//!   internal one included — a bare backtick path is documentation and
//!   stays so.
//! - Never silently downgrade severity. The block / warn distinction
//!   derives from the configured `block_on_memory_only` flag.
//! - Never judge a document's markdown. CommonMark closes a fence the author
//!   forgot to close at the end of the file, so the claims below it are code
//!   and go unchecked, and nothing here says so — the accident is real and
//!   the alternative was worse: the finding that reported it fired on every
//!   document a renderer reads correctly. `plan audit` keeps its own signal
//!   because its subject is rows an author believes recorded; this module's
//!   is what a document asserts, and a reader asserts nothing it cannot see.

pub mod advisory;
mod claim;
pub mod context7;
pub mod fetched;
pub mod internal;
pub mod memory;

use std::collections::HashMap;
use std::path::Path;

pub use claim::{Anchor, AnchorKind, Claim, ClaimKind, parse_claims};

use crate::config::EvidenceConfig;
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};
use crate::wire_enum::wire_enum;

/// Provenance-specific verifier.
///
/// Implementations are pure functions of `(claim, working_dir)`.
/// They MUST NOT touch the network or call external processes at
/// verify time; build-time fetches are encoded into the claim's
/// recorded fetched-date instead.
pub trait Verifier: Send + Sync {
    fn provenance(&self) -> &str;
    fn verify(&self, claim: &Claim, working_dir: &Path) -> VerifyOutcome;
}

wire_enum! {
    /// Closed set of supported verifier strategies. Adding a variant requires
    /// updating [`from_str`], [`as_str`], [`ALL`], and the match in
    /// [`EvidenceVerifier::new`] — the compiler enforces all four sites via
    /// exhaustive `match`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VerifierStrategy {
        FilePathLine => "file-path-line",
        Context7 => "context7",
        FetchedUrl => "fetched-url",
        MemoryOnly => "memory-only",
    }
}

/// Result of verifying a single claim.
#[derive(Debug, Clone)]
pub enum VerifyOutcome {
    Ok,
    Violation {
        message: String,
        hint: Option<String>,
    },
}

/// Orchestrates verifier strategies against a configured policy.
pub struct EvidenceVerifier {
    strategies: HashMap<String, Box<dyn Verifier>>,
    default_provenance: String,
    block_on_memory_only: bool,
}

impl EvidenceVerifier {
    /// Construct from a loaded [`EvidenceConfig`]. Returns an error if any
    /// declared strategy is unknown (defence in depth — `Config::validate`
    /// also rejects this, but the verifier should not assume a pre-validated
    /// config when used as a library).
    pub fn new(cfg: &EvidenceConfig) -> Result<Self> {
        let mut strategies: HashMap<String, Box<dyn Verifier>> = HashMap::new();
        for v in &cfg.verifiers {
            let provenance = v.provenance.clone();
            let strategy =
                VerifierStrategy::from_str(&v.strategy).ok_or_else(|| Error::ConfigInvalid {
                    message: format!(
                        "unknown verifier strategy '{}' (known: {})",
                        v.strategy,
                        VerifierStrategy::ALL
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    location: None,
                })?;
            let verifier: Box<dyn Verifier> = match strategy {
                VerifierStrategy::FilePathLine => {
                    Box::new(internal::InternalFileVerifier::new(provenance.clone()))
                }
                VerifierStrategy::Context7 => Box::new(context7::Context7Verifier::new(
                    provenance.clone(),
                    v.library_allowlist.clone(),
                )),
                VerifierStrategy::FetchedUrl => Box::new(fetched::FetchedUrlVerifier::new(
                    provenance.clone(),
                    v.max_age_days.unwrap_or(90),
                )),
                VerifierStrategy::MemoryOnly => {
                    Box::new(memory::MemoryOnlyVerifier::new(provenance.clone()))
                }
            };
            strategies.insert(provenance, verifier);
        }
        Ok(Self {
            strategies,
            default_provenance: cfg.default_provenance.clone(),
            block_on_memory_only: cfg.block_on_memory_only,
        })
    }

    /// Verify all claims found in `markdown`. The `source` path appears in
    /// the location of any finding produced.
    pub fn verify_text(&self, markdown: &str, source: &Path, working_dir: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        for claim in parse_claims(markdown) {
            let provenance = claim
                .provenance
                .as_deref()
                .unwrap_or(self.default_provenance.as_str());

            let Some(verifier) = self.strategies.get(provenance) else {
                findings.push(Finding {
                    slug: "evidence-unknown-provenance".to_string(),
                    severity: Severity::Major,
                    location: Location::line(source.to_path_buf(), claim.line),
                    message: format!("provenance '{provenance}' is not registered"),
                    hint: Some(format!(
                        "register a verifier for '{provenance}' under [[evidence.verifiers]]"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                continue;
            };

            match verifier.verify(&claim, working_dir) {
                VerifyOutcome::Ok => {}
                VerifyOutcome::Violation { message, hint } => {
                    let severity = if provenance == "memory-only" && !self.block_on_memory_only {
                        Severity::Minor
                    } else {
                        Severity::Blocker
                    };
                    findings.push(Finding {
                        slug: format!("evidence-{provenance}"),
                        severity,
                        location: Location::line(source.to_path_buf(), claim.line),
                        message,
                        hint,
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
            }
        }
        findings
    }

    /// Convenience: read `path` and verify its contents.
    pub fn verify_file(&self, path: &Path, working_dir: &Path) -> Result<Vec<Finding>> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(self.verify_text(&contents, path, working_dir))
    }
}

#[cfg(test)]
mod strategy_tests {
    use super::VerifierStrategy;

    #[test]
    fn from_str_round_trips_every_variant() {
        for s in VerifierStrategy::ALL {
            assert_eq!(VerifierStrategy::from_str(s.as_str()), Some(*s));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(VerifierStrategy::from_str("nope"), None);
    }
}
