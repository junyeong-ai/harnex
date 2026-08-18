//! # sentinel — the markers harnex writes into generated files
//!
//! Two grammars, both reserved to harnex and both HTML comments so they are
//! invisible when the markdown renders.
//!
//! `<!-- harnex-managed:start <slug> --> ... <!-- harnex-managed:end <slug> -->`
//! sentinels delimit harnex-owned regions inside generated markdown artifacts
//! (templates, reference docs, scaffolded `CLAUDE.md`). [`extract_regions`]
//! returns every well-formed pair from a text body keyed by slug.
//!
//! `<!-- harnex-fill: <what> -->` marks a value the generating step must
//! replace with a project observation. [`fill_markers`] returns every one left
//! behind. Being reserved is what makes the check exact: a template that spells
//! its gaps `<PROJECT_NAME>` or `Observed: <...>` cannot be distinguished from
//! prose, so an unfilled placeholder ships in silence — which is the blank-page
//! problem the templates exist to avoid, arriving as a finished-looking file.
//!
//! Single source of truth for sentinel parsing. Every consumer — the
//! managed-region auditor, the `spec_facts_sync` drift test, future
//! regenerate flow — calls this function. Two implementations of sentinel
//! syntax would invite divergence (one allowing trailing whitespace, the
//! other not, …) — Constitution IX forbids it.
//!
//! ## What this module refuses to do
//!
//! - Never write. The util is read-only structural extraction.
//! - Never normalize the extracted body (case, whitespace). Callers
//!   normalize for their own equality semantics — preserving the raw bytes
//!   keeps the parser format-agnostic.
//! - Never interpret markdown context. The parser is line-oriented and
//!   **does not recognize code fences** (`` ``` `` / `~~~`). A sentinel
//!   appearing inside a fenced code block will be extracted as a real
//!   region. Do not include literal sentinel syntax inside fenced code
//!   blocks in templates or reference docs — use a paraphrased example
//!   or an HTML entity escape instead.
//! - Never panic on malformed input. An unterminated `:start` returns the
//!   slug with an empty body so the caller's drift check still fires
//!   (a missing closing sentinel is itself drift).

use std::collections::BTreeMap;

const START_PREFIX: &str = "<!-- harnex-managed:start ";
const SUFFIX: &str = " -->";
const FILL_PREFIX: &str = "<!--";
const FILL_TAG: &str = "harnex-fill:";

/// One unresolved fill marker: its 1-based line, and what the template asked
/// the generating step to observe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillMarker {
    pub line: usize,
    pub wanted: String,
}

/// Every `<!-- harnex-fill: … -->` left in `content`.
///
/// A marker is harnex's own token, so finding one is a fact rather than an
/// interpretation of prose. Whitespace between `<!--` and the tag is not part
/// of that token: `<!--harnex-fill:` is the same marker to every reader and to
/// every markdown renderer, and requiring the space made a template author's
/// spacing decide whether the check ran at all.
///
/// A marker whose `-->` sits on a later line is reported at its opening line —
/// several of the shipped templates spell a long instruction that way, and
/// stopping at the newline meant the longest markers, the ones asking for the
/// most work, were the ones the check could not see. `wanted` is then the whole
/// instruction with its line breaks flattened, because a finding is read on one
/// line.
///
/// A line carrying more than one marker reports the first: the finding is that
/// the file is unfinished, and one per line is enough to say so without turning
/// a report into a concordance.
pub fn fill_markers(content: &str) -> Vec<FillMarker> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = content[cursor..].find(FILL_PREFIX) {
        let open = cursor + rel;
        let after_open = open + FILL_PREFIX.len();
        let rest = &content[after_open..];
        let Some(tagged) = rest.trim_start().strip_prefix(FILL_TAG) else {
            cursor = after_open;
            continue;
        };
        let Some(end) = tagged.find("-->") else {
            break;
        };
        out.push(FillMarker {
            line: content[..open].matches('\n').count() + 1,
            wanted: tagged[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        });
        cursor = after_open + (rest.len() - tagged.len()) + end + "-->".len();
    }
    out
}

/// Extract every `harnex-managed` block from `content` keyed by slug.
///
/// Returns one entry per `:start <slug> -->` marker found. The body is the
/// raw bytes between the start marker and the matching `:end <slug> -->`,
/// including surrounding newlines. Unterminated markers yield an empty body
/// for that slug — see the module-level note.
///
/// Duplicate slugs are forbidden: if the same slug appears more than once,
/// the entry is replaced with an empty body (forcing a downstream drift
/// comparison to fail) rather than silently retaining one of the two
/// conflicting regions. Callers can detect the collision by checking for
/// an unexpectedly empty body when the template has a non-empty one.
pub fn extract_regions(content: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut seen_slugs = std::collections::BTreeSet::new();
    let mut cursor = 0usize;
    while cursor < content.len() {
        let Some(rel_start) = content[cursor..].find(START_PREFIX) else {
            break;
        };
        let header_pos = cursor + rel_start;
        let after_prefix = header_pos + START_PREFIX.len();
        // A marker is one HTML comment on one line, so the closing `-->` is
        // looked for on that line and nowhere else. Searching forward without
        // the bound turned a single typo — `:start notes-->`, one space short —
        // into a slug spanning the rest of the document: the malformed header
        // swallowed every well-formed region below it, and the auditor reported
        // an intact managed region as missing.
        let line_end = content[after_prefix..]
            .find('\n')
            .map_or(content.len(), |n| after_prefix + n);
        let Some(rel_suffix) = content[after_prefix..line_end].find(SUFFIX) else {
            cursor = line_end;
            continue;
        };
        let header_end = after_prefix + rel_suffix + SUFFIX.len();
        let slug = content[after_prefix..after_prefix + rel_suffix]
            .trim()
            .to_string();
        if slug.is_empty() {
            cursor = header_end;
            continue;
        }
        let end_marker = format!("<!-- harnex-managed:end {slug} -->");
        // An unterminated start records its slug with an empty body so a drift
        // check still fires, and scanning continues past the header rather than
        // stopping: one region an operator forgot to close must not hide every
        // region after it.
        let Some(rel_end) = content[header_end..].find(&end_marker) else {
            out.insert(slug, String::new());
            cursor = header_end;
            continue;
        };
        let body = &content[header_end..header_end + rel_end];
        // A body that itself contains a start marker means malformed nesting
        // (e.g. `:start a` … `:start a` … `:end a`): the inner `:end` closed
        // the OUTER start and the nested start was swallowed into this body,
        // so the duplicate-slug guard below never sees it. Poison to an empty
        // body so a downstream drift check fires rather than accepting a
        // region with a stray sentinel inside it.
        if !seen_slugs.insert(slug.clone()) || body.contains(START_PREFIX) {
            // Duplicate / nested slug — poison to empty body so drift checks fire.
            out.insert(slug, String::new());
        } else {
            out.insert(slug, body.to_string());
        }
        cursor = header_end + rel_end + end_marker.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fill_marker_is_found_however_it_is_spaced_and_wrapped() {
        // Seven shipped templates spell a long instruction across lines, and
        // the whitespace after `<!--` is an author's habit rather than part of
        // the token. Requiring either made the longest markers — the ones
        // asking for the most work — the ones the check could not see.
        let found = fill_markers(
            "a\n<!--harnex-fill: no space -->\nb\n<!-- harnex-fill: spread\n   over lines -->\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].wanted, "no space");
        assert_eq!(found[1].line, 4);
        assert_eq!(found[1].wanted, "spread over lines");
    }

    #[test]
    fn a_comment_that_is_not_a_fill_marker_is_left_alone() {
        assert!(fill_markers("<!-- an ordinary note -->").is_empty());
        assert!(fill_markers("<!-- harnex-managed:start x -->").is_empty());
        assert!(fill_markers("prose mentioning harnex-fill: without a comment").is_empty());
    }

    #[test]
    fn a_malformed_marker_does_not_swallow_the_regions_below_it() {
        // One missing space in an operator's own marker. Searching for the
        // closing `-->` past the newline made the whole rest of the document
        // one slug, hid the well-formed region under it, and the auditor
        // reported an intact managed region as missing — a Major finding on a
        // file nobody had touched.
        let content = "\
# proj
<!-- harnex-managed:start notes-->
my note
<!-- harnex-managed:end notes -->
<!-- harnex-managed:start enforcement-summary -->
canonical
<!-- harnex-managed:end enforcement-summary -->
";
        let regions = extract_regions(content);
        assert_eq!(
            regions.keys().collect::<Vec<_>>(),
            vec!["enforcement-summary"]
        );
        assert_eq!(
            regions.get("enforcement-summary").map(String::as_str),
            Some("\ncanonical\n")
        );
    }

    #[test]
    fn an_unterminated_region_does_not_hide_the_ones_after_it() {
        let content = "\
<!-- harnex-managed:start opened -->
never closed
<!-- harnex-managed:start closed -->
body
<!-- harnex-managed:end closed -->
";
        let regions = extract_regions(content);
        assert_eq!(regions.get("opened").map(String::as_str), Some(""));
        assert_eq!(regions.get("closed").map(String::as_str), Some("\nbody\n"));
    }

    #[test]
    fn extracts_single_region() {
        let content = "before\n<!-- harnex-managed:start foo -->\nbody\n<!-- harnex-managed:end foo -->\nafter\n";
        let regions = extract_regions(content);
        assert_eq!(regions.get("foo").map(String::as_str), Some("\nbody\n"));
    }

    #[test]
    fn extracts_multiple_regions() {
        let content = "\
a
<!-- harnex-managed:start x -->
X
<!-- harnex-managed:end x -->
mid
<!-- harnex-managed:start y -->
Y
<!-- harnex-managed:end y -->
end
";
        let regions = extract_regions(content);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions.get("x").map(String::as_str), Some("\nX\n"));
        assert_eq!(regions.get("y").map(String::as_str), Some("\nY\n"));
    }

    #[test]
    fn empty_input_yields_empty_map() {
        assert!(extract_regions("").is_empty());
    }

    #[test]
    fn no_markers_yields_empty_map() {
        assert!(extract_regions("plain prose without sentinels").is_empty());
    }

    #[test]
    fn unterminated_start_records_empty_body() {
        // The start marker is recognised but the matching end is missing —
        // record the slug with an empty body so a downstream equality check
        // surfaces drift rather than silently accepting truncated input.
        let content = "<!-- harnex-managed:start foo -->\nbody but no end";
        let regions = extract_regions(content);
        assert_eq!(regions.get("foo").map(String::as_str), Some(""));
    }

    #[test]
    fn empty_slug_is_skipped() {
        // `<!-- harnex-managed:start  -->` (no slug) cannot pair with any end
        // marker; skip rather than record an empty key.
        let content = "<!-- harnex-managed:start  -->\n<!-- harnex-managed:end  -->\n";
        assert!(extract_regions(content).is_empty());
    }

    #[test]
    fn duplicate_slug_poisons_to_empty_body() {
        // Two regions with the same slug — the second occurrence replaces the
        // first with an empty body so a downstream equality check detects the
        // collision rather than silently picking one.
        let content = "\
<!-- harnex-managed:start a -->
first
<!-- harnex-managed:end a -->
gap
<!-- harnex-managed:start a -->
second
<!-- harnex-managed:end a -->
";
        let regions = extract_regions(content);
        assert_eq!(
            regions.get("a").map(String::as_str),
            Some(""),
            "duplicate slug must poison to empty body"
        );
    }

    #[test]
    fn nested_same_slug_poisons_to_empty_body() {
        // `:start a` … `:start a` … `:end a` … `:end a`: the inner `:end`
        // closes the outer start and the nested start lands inside the body.
        // That body contains a start marker → poison so drift is detected.
        let content = "\
<!-- harnex-managed:start a -->
outer
<!-- harnex-managed:start a -->
inner
<!-- harnex-managed:end a -->
trailer
<!-- harnex-managed:end a -->
";
        let regions = extract_regions(content);
        assert_eq!(
            regions.get("a").map(String::as_str),
            Some(""),
            "nested same-slug must poison to empty body"
        );
    }

    #[test]
    fn slug_is_trimmed() {
        let content =
            "<!-- harnex-managed:start   foo   -->\nbody\n<!-- harnex-managed:end foo -->\n";
        let regions = extract_regions(content);
        assert!(regions.contains_key("foo"));
    }
}
