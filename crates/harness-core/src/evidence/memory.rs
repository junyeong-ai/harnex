//! Verifier strategy: memory-only.
//!
//! Memory-only claims have no external verifiable source. This verifier
//! always returns a Violation; the orchestrator (`EvidenceVerifier::verify_text`)
//! modulates severity based on `block_on_memory_only` — Blocker when true,
//! Minor when false.

use std::path::Path;

use super::{Claim, VerifyOutcome, Verifier};

pub(crate) struct MemoryOnlyVerifier {
    provenance: String,
}

impl MemoryOnlyVerifier {
    pub(crate) fn new(provenance: String) -> Self {
        Self { provenance }
    }
}

impl Verifier for MemoryOnlyVerifier {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn verify(&self, _claim: &Claim, _working_dir: &Path) -> VerifyOutcome {
        VerifyOutcome::Violation {
            message: "claim is memory-only — provide a verifiable source".into(),
            hint: Some(
                "replace [memory] with [context7: <lib>] or [fetched: YYYY-MM-DD] https://… or a `path:line` internal citation".into(),
            ),
        }
    }
}
