//! # sentinel — managed-region marker utility
//!
//! `<!-- harnex-managed:start <slug> --> ... <!-- harnex-managed:end <slug> -->`
//! sentinels delimit harnex-owned regions inside generated markdown artifacts
//! (templates, reference docs, scaffolded `CLAUDE.md`). [`extract_regions`]
//! returns every well-formed pair from a text body keyed by slug.
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
        let Some(rel_suffix) = content[after_prefix..].find(SUFFIX) else {
            break;
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
        let Some(rel_end) = content[header_end..].find(&end_marker) else {
            out.insert(slug, String::new());
            break;
        };
        let body = &content[header_end..header_end + rel_end];
        if !seen_slugs.insert(slug.clone()) {
            // Duplicate slug — poison to empty body so drift checks fire.
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
    fn slug_is_trimmed() {
        let content = "<!-- harnex-managed:start   foo   -->\nbody\n<!-- harnex-managed:end foo -->\n";
        let regions = extract_regions(content);
        assert!(regions.contains_key("foo"));
    }
}
