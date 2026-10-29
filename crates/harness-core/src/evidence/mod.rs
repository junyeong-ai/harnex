//! # evidence — claims a document makes about this project, held to the tree
//!
//! Parses markdown for `[file: …]` claims and resolves each against the
//! project: the file exists, and the line, section or symbol it names is
//! still there. [`advisory`] holds the recorded basis of a measurement this
//! toolkit never runs fresh.
//!
//! ## What this module refuses to do
//!
//! - Never check a claim it cannot resolve here. A claim about a web page or
//!   a library's documentation is unverifiable without the network, and a
//!   check of its shape alone would report it verified.
//! - Never invent a claim. Unmarked free text is not one; a bare backtick path
//!   is documentation and stays so.
//! - Never judge a document's markdown. CommonMark closes a fence the author
//!   forgot to close at the end of the file, so the claims below it are code
//!   and go unchecked, and nothing here says so — the accident is real and
//!   the alternative was worse: the finding that reported it fired on every
//!   document a renderer reads correctly. This module's subject is what a
//!   document asserts, and a reader asserts nothing it cannot see.

pub mod advisory;
mod claim;
mod resolve;

use std::path::Path;

pub use claim::{Anchor, AnchorKind, Claim, parse_claims};

use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

/// Result of resolving a single claim.
#[derive(Debug, Clone)]
enum VerifyOutcome {
    Ok,
    Violation { message: String, hint: String },
}

/// Resolve every claim in `markdown`. The `source` path appears in the
/// location of any finding produced.
pub fn verify_text(markdown: &str, source: &Path, working_dir: &Path) -> Vec<Finding> {
    parse_claims(markdown)
        .into_iter()
        .filter_map(|claim| match resolve::resolve(&claim, working_dir) {
            VerifyOutcome::Ok => None,
            VerifyOutcome::Violation { message, hint } => Some(Finding {
                slug: "evidence-claim-unresolved".to_string(),
                severity: Severity::Blocker,
                location: Location::line(source.to_path_buf(), claim.line),
                message,
                hint: Some(hint),
            }),
        })
        .collect()
}

/// Read `path` and resolve its claims.
pub fn verify_file(path: &Path, working_dir: &Path) -> Result<Vec<Finding>> {
    let contents = std::fs::read_to_string(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(verify_text(&contents, path, working_dir))
}
