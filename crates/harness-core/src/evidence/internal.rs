//! Verifier strategy: internal file-path-line existence check.

use std::path::Path;

use super::{Claim, ClaimKind, Verifier, VerifyOutcome};

pub(crate) struct FilePathLineVerifier {
    provenance: String,
}

impl FilePathLineVerifier {
    pub(crate) fn new(provenance: String) -> Self {
        Self { provenance }
    }
}

impl Verifier for FilePathLineVerifier {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn verify(&self, claim: &Claim, working_dir: &Path) -> VerifyOutcome {
        let (path, line) = match &claim.kind {
            ClaimKind::FilePathLine { path, line } => (path, *line),
            _ => {
                return VerifyOutcome::Violation {
                    message: format!(
                        "provenance '{}' expects a `path:line` claim shape",
                        self.provenance
                    ),
                    hint: Some(
                        "use the `path/to/file.ext:line` backtick form for internal claims".into(),
                    ),
                };
            }
        };

        // A claim path must stay inside the project — reject `..` traversal
        // and absolute paths so a claim cannot verify (or read) a file
        // outside `working_dir`.
        if crate::path_guard::reject_traversal(Path::new(path)).is_err()
            || Path::new(path).is_absolute()
        {
            return VerifyOutcome::Violation {
                message: format!("claim path '{path}' escapes the project root"),
                hint: Some("use a project-relative path without `..` or a leading `/`".into()),
            };
        }

        let full = working_dir.join(path);
        if !full.is_file() {
            return VerifyOutcome::Violation {
                message: format!("file '{path}' does not exist"),
                hint: Some("update the path or remove the claim".into()),
            };
        }

        if let Some(line_no) = line {
            match std::fs::read_to_string(&full) {
                Ok(content) => {
                    let total = content.lines().count() as u32;
                    if line_no == 0 || line_no > total {
                        return VerifyOutcome::Violation {
                            message: format!(
                                "line {line_no} out of range (file has {total} lines)"
                            ),
                            hint: Some("update the line number".into()),
                        };
                    }
                }
                Err(_) => {
                    return VerifyOutcome::Violation {
                        message: format!("could not read '{path}'"),
                        hint: Some(
                            "verify the path exists and is readable from the working directory"
                                .into(),
                        ),
                    };
                }
            }
        }

        VerifyOutcome::Ok
    }
}
