//! Verifier strategy: context7 library reference.
//!
//! Validates that the cited library is in the configured allowlist (if
//! non-empty). The allowlist exists to prevent typos that would otherwise
//! resolve to a different package on the context7 service.

use std::path::Path;

use super::{Claim, ClaimKind, Verifier, VerifyOutcome};

pub(crate) struct Context7Verifier {
    provenance: String,
    library_allowlist: Vec<String>,
}

impl Context7Verifier {
    pub(crate) fn new(provenance: String, library_allowlist: Vec<String>) -> Self {
        Self {
            provenance,
            library_allowlist,
        }
    }
}

impl Verifier for Context7Verifier {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn verify(&self, claim: &Claim, _working_dir: &Path) -> VerifyOutcome {
        let library = match &claim.kind {
            ClaimKind::Context7Library { library } => library,
            _ => {
                return VerifyOutcome::Violation {
                    message: format!(
                        "provenance '{}' expects a `[context7: <library>]` claim shape",
                        self.provenance
                    ),
                    hint: Some("use the [context7: <library-id>] marker".into()),
                };
            }
        };

        // Empty allowlist = accept any library identifier shape.
        if !self.library_allowlist.is_empty()
            && !self.library_allowlist.iter().any(|l| l == library)
        {
            return VerifyOutcome::Violation {
                message: format!("library '{library}' is not in the context7 allowlist"),
                hint: Some(
                    "add the library to [[evidence.verifiers]] library_allowlist or correct the spelling".into(),
                ),
            };
        }

        VerifyOutcome::Ok
    }
}
