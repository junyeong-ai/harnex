//! Verifier strategy: an internal claim, checked against the anchor it names.

use std::path::Path;

use super::{Anchor, Claim, ClaimKind, Verifier, VerifyOutcome};
use crate::markdown::{Unclosed, Visibility, atx_heading, doc_lines};

pub(crate) struct InternalFileVerifier {
    provenance: String,
}

impl InternalFileVerifier {
    pub(crate) fn new(provenance: String) -> Self {
        Self { provenance }
    }
}

impl Verifier for InternalFileVerifier {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn verify(&self, claim: &Claim, working_dir: &Path) -> VerifyOutcome {
        let (path, anchor) = match &claim.kind {
            ClaimKind::File { path, anchor } => (path, anchor),
            _ => {
                return VerifyOutcome::Violation {
                    message: format!("provenance '{}' expects a file claim", self.provenance),
                    hint: Some(
                        "mark it as an internal claim: a `file:` marker in square brackets around a \
                         project-relative path, with an optional `:line` or ` § Heading`"
                            .into(),
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

        match anchor {
            Anchor::Whole => VerifyOutcome::Ok,
            Anchor::Line(line) => match std::fs::read_to_string(&full) {
                Ok(content) => verify_line(path, &content, *line),
                Err(_) => unreadable(path),
            },
            Anchor::Section(heading) => match std::fs::read_to_string(&full) {
                Ok(content) => verify_section(path, &content, heading),
                Err(_) => unreadable(path),
            },
        }
    }
}

fn unreadable(path: &str) -> VerifyOutcome {
    VerifyOutcome::Violation {
        message: format!("could not read '{path}'"),
        hint: Some("verify the path exists and is readable from the working directory".into()),
    }
}

fn verify_line(path: &str, content: &str, line: u32) -> VerifyOutcome {
    let total = doc_lines(content).len() as u32;
    if line == 0 || line > total {
        return VerifyOutcome::Violation {
            message: format!("line {line} out of range ('{path}' has {total} lines)"),
            hint: Some("update the line number".into()),
        };
    }
    VerifyOutcome::Ok
}

/// A section anchor resolves when the file spells that heading exactly once.
///
/// Two headings with one name is its own violation rather than a pass on the
/// first: a pointer that resolves to both places names neither, and the
/// reader it sends is the one who discovers that.
fn verify_section(path: &str, content: &str, heading: &str) -> VerifyOutcome {
    let mut doc = Visibility::new();
    let mut found: Vec<u32> = Vec::new();

    for (idx, raw_line) in doc_lines(content).into_iter().enumerate() {
        let line_no = (idx as u32) + 1;
        let Some(line) = doc.read(raw_line, line_no) else {
            continue;
        };
        if atx_heading(&line).is_some_and(|(_, title)| title == heading) {
            found.push(line_no);
        }
    }

    // Headings below an open delimiter were never seen, so neither a miss nor
    // a single hit is an answer about the file — only about the part of it a
    // reader still sees.
    if let Some(unclosed) = doc.unclosed() {
        let (line, what) = match unclosed {
            Unclosed::Fence { line } => (line, "a code fence"),
            Unclosed::Comment { line } => (line, "an HTML comment"),
        };
        return VerifyOutcome::Violation {
            message: format!(
                "'{path}' cannot be read for headings: {what} opened at line {line} never closes"
            ),
            hint: Some(format!("close the delimiter opened at {path}:{line}")),
        };
    }

    match found.as_slice() {
        [_] => VerifyOutcome::Ok,
        [] => VerifyOutcome::Violation {
            message: format!("no heading '{heading}' in '{path}'"),
            hint: Some(match content.contains(heading) {
                true => format!(
                    "'{heading}' occurs in the file but not as a heading of its own — cite the \
                     heading exactly as it is written, including any words after it"
                ),
                false => "cite the heading exactly as the file spells it, or point at the file \
                          alone"
                    .into(),
            }),
        },
        lines => VerifyOutcome::Violation {
            message: format!(
                "'{heading}' names {} headings in '{path}' (lines {})",
                lines.len(),
                lines
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            hint: Some(
                "a pointer that resolves to more than one section names none of them — give one \
                 of them a distinct heading"
                    .into(),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(file: &str, heading: &str) -> VerifyOutcome {
        verify_section("doc.md", file, heading)
    }

    fn message(file: &str, heading: &str) -> String {
        match outcome(file, heading) {
            VerifyOutcome::Violation { message, .. } => message,
            VerifyOutcome::Ok => panic!("expected a violation for `{heading}`"),
        }
    }

    #[test]
    fn a_heading_the_file_spells_once_resolves() {
        let doc = "# Title\n\n## The bookend trigger\n\nbody\n\n### Deeper\n";
        for heading in ["Title", "The bookend trigger", "Deeper"] {
            assert!(
                matches!(outcome(doc, heading), VerifyOutcome::Ok),
                "expected `{heading}` to resolve"
            );
        }
    }

    #[test]
    fn a_prefix_of_a_heading_is_not_that_heading() {
        // The measured shape: a pointer written from memory as
        // `§ Two refutation regimes` against a heading that reads
        // `## Two refutation regimes, chosen by subject`.
        let doc = "## Two refutation regimes, chosen by subject\n\nbody\n";
        let message = message(doc, "Two refutation regimes");
        assert!(message.contains("no heading"), "{message}");
        let VerifyOutcome::Violation { hint, .. } = outcome(doc, "Two refutation regimes") else {
            unreachable!()
        };
        assert!(
            hint.is_some_and(|h| h.contains("not as a heading of its own")),
            "the hint must separate a renamed heading from a wrong file"
        );
    }

    #[test]
    fn two_headings_with_one_name_resolve_to_neither() {
        let doc = "## Limits\n\na\n\n## Limits\n\nb\n";
        let message = message(doc, "Limits");
        assert!(message.contains("names 2 headings"), "{message}");
        assert!(message.contains("lines 1, 5"), "{message}");
    }

    #[test]
    fn a_heading_a_reader_never_sees_does_not_resolve() {
        for (label, doc) in [
            ("fenced", "```\n## Sample\n```\n"),
            ("commented", "<!--\n## Sample\n-->\n"),
        ] {
            assert!(
                message(doc, "Sample").contains("no heading"),
                "a {label} heading anchors nothing"
            );
        }
    }

    #[test]
    fn a_document_that_cannot_be_read_whole_is_not_answered() {
        // The heading is present and unique above the open fence, so a reader
        // that stopped at the count would pass — while every heading below it
        // is invisible, duplicates included.
        let doc = "## Limits\n\n```\nnever closed\n## Limits\n";
        let message = message(doc, "Limits");
        assert!(message.contains("never closes"), "{message}");
        assert!(message.contains("line 3"), "{message}");
    }

    #[test]
    fn a_line_anchor_counts_lines_as_the_renderer_does() {
        assert!(matches!(
            verify_line("doc.md", "a\rb\rc", 3),
            VerifyOutcome::Ok
        ));
        assert!(matches!(
            verify_line("doc.md", "a\rb\rc", 4),
            VerifyOutcome::Violation { .. }
        ));
        assert!(matches!(
            verify_line("doc.md", "a\nb\n", 0),
            VerifyOutcome::Violation { .. }
        ));
    }
}
