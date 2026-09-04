//! Verifier strategy: an internal claim, checked against the anchor it names.

use std::path::Path;

use super::{Anchor, Claim, ClaimKind, Verifier, VerifyOutcome};
use crate::markdown::Document;

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
                        "mark it as an internal claim — a project-relative path in a file marker: \
                         [file: path/to/file.ext] for the whole file, \
                         [file: path/to/file.ext :: fn name] for what it spells, \
                         [file: path/to/doc.md § Heading] for a section, \
                         [file: path/to/file.ext:42] for a line"
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
            Anchor::Symbol(symbol) => match std::fs::read_to_string(&full) {
                Ok(content) => verify_symbol(path, &content, symbol),
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

/// A line anchor resolves when the file reaches that line and the line carries
/// text.
///
/// That is all a position can prove: a blank line names nothing, and any other
/// line answers with whatever the edits above it left there. The reader owns
/// which lines exist, so absence here is the line being out of range rather
/// than a second way to be blank.
fn verify_line(path: &str, content: &str, line: u32) -> VerifyOutcome {
    match crate::markdown::line_at(content, line) {
        Some(text) if !text.trim().is_empty() => VerifyOutcome::Ok,
        Some(_) => VerifyOutcome::Violation {
            message: format!("line {line} of '{path}' is blank"),
            hint: Some(
                "a line anchor is verified for the file being that long and the line \
                 carrying text, so it survives the edit that moved its subject — \
                 anchor the claim on what the file spells instead"
                    .into(),
            ),
        },
        None => {
            let lines = crate::markdown::line_count(content);
            let plural = if lines == 1 { "line" } else { "lines" };
            VerifyOutcome::Violation {
                message: format!("line {line} out of range ('{path}' has {lines} {plural})"),
                hint: Some("update the line number".into()),
            }
        }
    }
}

/// A symbol anchor resolves when the file spells it exactly once, abutted by
/// no character a name could continue with.
///
/// The boundary is what separates `fn from_str` from `fn from_str_rejects`:
/// a declaration name that is the prefix of another is ordinary, so a
/// substring match would resolve a claim against a definition nobody cited.
/// It applies only where the needle's own edge is one of those characters, so
/// a needle ending in a delimiter still resolves — which is what lets a name
/// the C family cannot spell be cited in full.
///
/// Two occurrences are a violation rather than a pass on the first, for the
/// reason a section's are: an anchor that names two places names neither, and
/// the author holds the longer spelling that separates them.
fn verify_symbol(path: &str, content: &str, symbol: &str) -> VerifyOutcome {
    match bounded_occurrences(content, symbol) {
        1 => VerifyOutcome::Ok,
        0 => VerifyOutcome::Violation {
            message: format!("no symbol '{symbol}' in '{path}'"),
            hint: Some(match content.contains(symbol) {
                true => format!(
                    "'{symbol}' occurs in the file only inside a longer name — cite \
                     the declaration as the file spells it"
                ),
                false => "cite the declaration exactly as the file spells it, or point \
                          at the file alone"
                    .into(),
            }),
        },
        found => VerifyOutcome::Violation {
            message: format!("'{symbol}' occurs {found} times in '{path}'"),
            hint: Some(
                "an anchor names one place — extend the spelling until it does, or \
                 point at the file alone"
                    .into(),
            ),
        },
    }
}

/// The characters that continue a name, which is the C family's identifier
/// rule and not one language's whole alphabet.
///
/// A wider set was tried and withdrawn. Adding a character can only lower the
/// count, never raise it, so it produces two transitions and not one: a lone
/// occurrence falls to none, which is loud, and two occurrences fall to one,
/// which resolves a citation that names two places as though it named one.
///
/// The rule is read on both edges of a match while the characters that would
/// join it are one-sided: `!` never begins a name, `$` in a shell is a sigil on
/// a reference rather than part of the name, and `?` only ever ends one. So
/// `!READY` and `$REPO_URL` stopped counting as uses of `READY` and `REPO_URL`.
/// Making the rule side-aware does not rescue it either, because the same
/// character on the same side belongs to a name in one language and to an
/// operator in the next — a trailing `?` spells a Ruby predicate and a Rust try,
/// a `-` spells a CSS custom property and C subtraction. Deciding between them
/// needs the language, which a citation does not carry.
///
/// A match is refused only where an identifier character abuts an identifier
/// edge of the citation, so any other neighbour lets the citation read inside
/// a longer construct: against a file spelling only `check-types-and-lint`,
/// each of `check-types`, `types` and `and-lint` answers; `.rows {` answers to
/// `table.rows {`; a combining mark is no identifier character either, so
/// `cafe` answers to a `café` the file wrote decomposed. Where a
/// name and its extension are both present the shorter has no spelling at all
/// — beside a `save!`, both `save` and `def save` read twice — and there the
/// escape is the anchor rather than the spelling: a line, or the file.
fn is_name_character(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every position `needle` reads at, counted once each.
///
/// The scan resumes one character past a match rather than past its whole
/// length, because two occurrences may share characters: `::b::` reads twice in
/// `crate::a::b::b::c`, on either side of the middle `b`. Resuming past the
/// length sees one of them, and a symbol naming two places would then resolve
/// as though it named one.
///
/// The search stays `str::find`, which is what keeps a cited file's size off
/// the critical path; asking `starts_with` at every character instead gives
/// that up.
fn bounded_occurrences(haystack: &str, needle: &str) -> usize {
    let opens = needle.chars().next().is_some_and(is_name_character);
    let closes = needle.chars().next_back().is_some_and(is_name_character);
    let mut count = 0;
    let mut from = 0;
    while let Some(found) = haystack[from..].find(needle) {
        let at = from + found;
        let before = !opens
            || !haystack[..at]
                .chars()
                .next_back()
                .is_some_and(is_name_character);
        let after = !closes
            || !haystack[at + needle.len()..]
                .chars()
                .next()
                .is_some_and(is_name_character);
        if before && after {
            count += 1;
        }
        // One character, never one byte: `from` indexes the next slice and has
        // to stay on a boundary.
        let Some(step) = haystack[at..].chars().next().map(char::len_utf8) else {
            break;
        };
        from = at + step;
    }
    count
}

/// A section anchor resolves when the file spells that heading exactly once.
///
/// The comparison is against the heading a reader reads, so `## **Storage**`
/// and a setext `Storage` both answer to `Storage` — a citation is written
/// from the rendered document, not from its source. Two headings with one
/// name is its own violation rather than a pass on the first: a pointer that
/// resolves to both places names neither, and the reader it sends is the one
/// who discovers that.
fn verify_section(path: &str, content: &str, heading: &str) -> VerifyOutcome {
    let found: Vec<u32> = Document::of(content)
        .headings()
        .iter()
        .filter(|candidate| candidate.text == heading)
        .map(|candidate| candidate.line)
        .collect();

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
    fn a_heading_a_fence_swallows_is_not_a_second_heading() {
        // CommonMark closes a fence at the end of the document, so the
        // second spelling is code — one heading, and the pointer resolves.
        // A state machine that called this unterminated blocked a citation
        // into a document a renderer reads perfectly well.
        assert!(matches!(
            outcome("## Limits\n\n```\nnever closed\n## Limits\n", "Limits"),
            VerifyOutcome::Ok
        ));
    }

    #[test]
    fn a_heading_is_matched_as_a_reader_reads_it() {
        // Each of these was a Blocker against a heading the target does have.
        for (label, doc) in [
            ("setext", "Storage\n=======\n\nbody\n"),
            ("emphasis", "## **Storage**\n\nbody\n"),
            ("a code span", "## `Storage`\n\nbody\n"),
            ("a closing run", "## Storage ##\n\nbody\n"),
        ] {
            assert!(
                matches!(outcome(doc, "Storage"), VerifyOutcome::Ok),
                "a heading spelled with {label} did not resolve"
            );
        }
        // And two spellings of one heading are still two.
        assert!(
            message("## Storage\n\n## **Storage**\n", "Storage").contains("names 2 headings"),
            "a duplicate hidden behind markup must still be a duplicate"
        );
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
