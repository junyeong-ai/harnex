//! # Claim parser
//!
//! Extracts provenance-marked claims from markdown. Recognised syntaxes
//! (all whitespace-tolerant):
//!
//! - `[file: path/to/file.ext]` → the whole file is the owner
//! - `[file: path/to/file.ext:42]` → that line
//! - `[file: path/to/file.md § Heading]` → that section
//! - `[fetched: YYYY-MM-DD] https://...` → fetched-url claim
//! - `[context7: <library-id>]` → context7 claim
//! - `[memory]` → unverified memory claim
//!
//! Lines are 1-indexed (matches editor convention).

use std::sync::LazyLock;

use regex::Regex;

pub use crate::markdown::Unclosed;
use crate::markdown::{Visibility, doc_lines};

#[derive(Debug, Clone)]
pub struct Claim {
    pub raw: String,
    pub provenance: Option<String>,
    pub kind: ClaimKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum ClaimKind {
    File {
        path: String,
        anchor: Anchor,
    },
    Url {
        url: String,
        fetched_date: Option<String>,
    },
    Context7Library {
        library: String,
    },
    Memory,
}

/// What inside a file an internal claim points at.
///
/// The anchor is what decides whether the check still means anything a year
/// later: a line is verified only for the file being that long, so it holds
/// through the edit that moves its subject, while a section is matched
/// against what the file spells and fails on the rename that invalidates the
/// claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Whole,
    Line(u32),
    Section(String),
}

/// Every claim in a document, and whether the document could be read whole.
pub struct ClaimScan {
    pub claims: Vec<Claim>,
    /// A fence or comment left open, after which a reader — and so this
    /// parser — sees nothing. The claims below it are not absent, they are
    /// unread, which is the one thing a gate may not report as clean.
    pub unread: Option<Unclosed>,
}

static FETCHED_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[fetched:\s*(\d{4}-\d{2}-\d{2})\]\s*(https?://\S+)").expect("FETCHED_URL regex")
});

static CONTEXT7: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[context7:\s*([A-Za-z0-9_@./\-]+)\]").expect("CONTEXT7 regex"));

static MEMORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[memory\]").expect("MEMORY regex"));

// `[file: path/to/thing.rs:42]` — an internal claim, marked like every other.
//
// Marked rather than inferred, because inference here has no floor. The shape
// of a file and a line is also the shape of a host and a port, and no test
// separates them: `.sh`, `.rs`, `.io` and `.dev` are file extensions and
// top-level domains both. Requiring a directory separator excluded
// `api.example.com:8080` and left `imagePullSecrets/registry.io:5000`; the next
// tightening would leave the one after that. Each is ordinary prose in a
// deployment rule, and each failed Blocker in a gate that is on by default.
//
// The other three provenances were always marked (`[fetched: …]`,
// `[context7: …]`, `[memory]`). This one being the exception is what put a
// pattern-match over prose in a blocking tier, which `keep-soften-cut` refuses.
// A marker also tells a reader which references the gate checks, where a bare
// backtick path says nothing.
const FILE_MARKER: &str = "[file:";

/// The separator between a path and the heading it points at. Spelled with
/// spaces so it cannot be read out of an ordinary `§` in prose, and matched
/// leftmost so a heading may carry one of its own.
const SECTION_SEPARATOR: &str = " § ";

/// The interior of each `[file: …]` on `line`, brackets balanced.
///
/// Scanned rather than matched, because a path may hold `]` and a regex cannot
/// count. `app/[id]/page.tsx` is idiomatic in a stack this toolkit ships a
/// profile for, and a character class that stops at the first `]` reads it as
/// `app/[id` — the real claim goes unverified and the truncation fails Blocker
/// against a path nobody wrote.
fn file_claim_bodies(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find(FILE_MARKER) {
        let interior = &rest[open + FILE_MARKER.len()..];
        let mut depth = 0usize;
        let end = interior.char_indices().find_map(|(i, c)| match c {
            '[' => {
                depth += 1;
                None
            }
            ']' if depth == 0 => Some(i),
            ']' => {
                depth -= 1;
                None
            }
            _ => None,
        });
        let Some(end) = end else {
            break;
        };
        out.push(interior[..end].trim());
        rest = &interior[end + 1..];
    }
    out
}

/// Split a claim body into its path and the anchor it names.
///
/// A section separator wins over a line: a path does not carry one and a
/// heading may, so the leftmost splits and the heading is whatever follows.
/// Otherwise a trailing `:<digits>` is the line, which leaves a path free to
/// hold a colon of its own — a Windows drive letter reaches the verifier
/// intact. An empty path is not a claim: `[file: ]` says nothing to check.
fn split_file_claim(body: &str) -> Option<(&str, Anchor)> {
    if let Some((path, heading)) = body.split_once(SECTION_SEPARATOR) {
        let (path, heading) = (path.trim_end(), heading.trim());
        return (!path.is_empty() && !heading.is_empty())
            .then(|| (path, Anchor::Section(heading.to_string())));
    }
    if let Some((path, tail)) = body.rsplit_once(':')
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
    {
        return (!path.is_empty()).then(|| {
            // All digits, so the only parse failure is OVERFLOW of u32 — a line
            // far beyond any file. Map it to u32::MAX so the verifier reports
            // it out of range rather than silently as "no line to check".
            (path, Anchor::Line(tail.parse().unwrap_or(u32::MAX)))
        });
    }
    (!body.is_empty()).then_some((body, Anchor::Whole))
}

/// Parse every recognised claim out of `markdown`. Order within a line is
/// the order discovered by the per-pattern pass.
///
/// Two exemptions and no others: a fenced code block, which is a deliberate
/// sample of the syntax, and an HTML comment, which is an instruction. Prose,
/// tables, blockquotes and every depth of list item carry live claims — a
/// nested bullet is where a rule names its owners.
pub fn parse_claims(markdown: &str) -> ClaimScan {
    let mut claims = Vec::new();
    let mut doc = Visibility::new();

    for (idx, raw_line) in doc_lines(markdown).into_iter().enumerate() {
        let line_no = (idx as u32) + 1;
        let Some(line) = doc.read(raw_line, line_no) else {
            continue;
        };
        let line = line.as_str();

        for cap in FETCHED_URL.captures_iter(line) {
            claims.push(Claim {
                raw: cap[0].to_string(),
                provenance: Some("fetched-url".to_string()),
                kind: ClaimKind::Url {
                    url: cap[2].to_string(),
                    fetched_date: Some(cap[1].to_string()),
                },
                line: line_no,
            });
        }

        for cap in CONTEXT7.captures_iter(line) {
            claims.push(Claim {
                raw: cap[0].to_string(),
                provenance: Some("context7".to_string()),
                kind: ClaimKind::Context7Library {
                    library: cap[1].to_string(),
                },
                line: line_no,
            });
        }

        for _ in MEMORY.captures_iter(line) {
            claims.push(Claim {
                raw: "[memory]".to_string(),
                provenance: Some("memory-only".to_string()),
                kind: ClaimKind::Memory,
                line: line_no,
            });
        }

        for body in file_claim_bodies(line) {
            let Some((path, anchor)) = split_file_claim(body) else {
                continue;
            };
            claims.push(Claim {
                raw: format!("{FILE_MARKER} {body}]"),
                provenance: Some("internal".to_string()),
                kind: ClaimKind::File {
                    path: path.to_string(),
                    anchor,
                },
                line: line_no,
            });
        }
    }

    ClaimScan {
        claims,
        unread: doc.unclosed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_claims(md: &str) -> Vec<(String, Anchor)> {
        parse_claims(md)
            .claims
            .into_iter()
            .filter_map(|c| match c.kind {
                ClaimKind::File { path, anchor } => Some((path, anchor)),
                _ => None,
            })
            .collect()
    }

    fn paths(md: &str) -> Vec<String> {
        file_claims(md).into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn a_host_and_port_is_not_a_file_claim() {
        // `name.ext:digits` is also how a host and port are written, with or
        // without a directory-looking prefix, and a deployment rule names them
        // routinely. Reading one as a claim fails Blocker against a file
        // nobody meant to exist — the worst outcome available to a check that
        // is on by default. A marker is what removes the class rather than
        // narrowing it: nothing here is a claim.
        for md in [
            "The gateway is at `api.example.com:8080`.",
            "Point it at `db.internal:5432`.",
            "Pull from `imagePullSecrets/registry.io:5000`.",
            "Route via `api/gateway.io:8443`.",
            "Cache at `cache/redis.internal:6379`.",
            "Fetch `https://api.example.com:8080/v1` for the payload.",
            "A plain backtick path `src/lib.rs:42` is prose, not a claim.",
            "The spec's § 4 covers it, and `guide.md § Limits` is prose too.",
        ] {
            assert!(
                parse_claims(md).claims.is_empty(),
                "prose parsed as a file claim: {md}"
            );
        }
    }

    #[test]
    fn a_path_may_hold_brackets_a_colon_and_a_space() {
        // `app/[id]/page.tsx` is idiomatic in a stack this toolkit ships a
        // profile for. A character class that stops at the first `]` read it
        // as `app/[id`: the real claim went unverified and the truncation
        // failed Blocker against a path nobody wrote — both directions at once.
        for (md, want_path, want_anchor) in [
            (
                "See [file: app/[id]/page.tsx:10] for the handler.",
                "app/[id]/page.tsx",
                Anchor::Line(10),
            ),
            (
                "Catch-all [file: app/[[...slug]]/route.ts] is registered.",
                "app/[[...slug]]/route.ts",
                Anchor::Whole,
            ),
            (
                "On Windows [file: C:/repo/x.rs:4].",
                "C:/repo/x.rs",
                Anchor::Line(4),
            ),
            (
                "Spaces survive [file: My Docs/notes.md:2].",
                "My Docs/notes.md",
                Anchor::Line(2),
            ),
        ] {
            assert_eq!(
                file_claims(md),
                vec![(want_path.to_string(), want_anchor)],
                "from: {md}"
            );
        }
    }

    #[test]
    fn an_anchor_names_the_whole_file_a_line_or_a_section() {
        for (md, want_path, want_anchor) in [
            (
                "Owned by [file: pyproject.toml].",
                "pyproject.toml",
                Anchor::Whole,
            ),
            (
                "See [file: src/lib.rs:42] for context.",
                "src/lib.rs",
                Anchor::Line(42),
            ),
            (
                "Stated in [file: .claude/rules/x.md § The bookend trigger].",
                ".claude/rules/x.md",
                Anchor::Section("The bookend trigger".into()),
            ),
            // A heading may carry the separator; the leftmost one splits.
            (
                "See [file: docs/g.md § Limits § scope].",
                "docs/g.md",
                Anchor::Section("Limits § scope".into()),
            ),
            // A section wins over a trailing line, so a heading ending in
            // digits is a heading.
            (
                "See [file: docs/g.md § Step 1:2].",
                "docs/g.md",
                Anchor::Section("Step 1:2".into()),
            ),
        ] {
            assert_eq!(
                file_claims(md),
                vec![(want_path.to_string(), want_anchor)],
                "from: {md}"
            );
        }
    }

    #[test]
    fn a_marker_with_nothing_to_check_is_not_a_claim() {
        for md in [
            "An empty one [file: ] says nothing.",
            "An unterminated [file: src/lib.rs:1 never closes.",
            "A link [file](https://example.com) is not a marker.",
        ] {
            assert!(
                parse_claims(md).claims.is_empty(),
                "parsed a claim from: {md}"
            );
        }
    }

    #[test]
    fn a_closing_fence_carries_nothing_but_its_own_characters() {
        // Per CommonMark a fence line with trailing prose does not close the
        // block, so a claim under it is still inside code.
        let md = "\
```markdown
Text
``` note: trailing prose, so the block stays open
[file: src/hidden.rs:99]
```

[file: src/after.rs:1]
";
        assert_eq!(paths(md), vec!["src/after.rs"]);
    }

    #[test]
    fn extracts_fetched_url() {
        let md = "Per [fetched: 2026-05-20] https://example.com/x the rule is …";
        let claims = parse_claims(md).claims;
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("fetched-url"));
    }

    #[test]
    fn extracts_context7() {
        let md = "Per [context7: vercel/next.js] middleware fires before …";
        let claims = parse_claims(md).claims;
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("context7"));
    }

    #[test]
    fn extracts_memory_marker() {
        let md = "The runtime [memory] does not re-read skills mid-turn.";
        let claims = parse_claims(md).claims;
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("memory-only"));
    }

    #[test]
    fn ignores_prose_colons() {
        let md = "TODO: handle this case. Also remember: be precise.";
        assert!(parse_claims(md).claims.is_empty());
    }

    #[test]
    fn line_numbers_are_one_indexed() {
        let md = "intro line\n\n[file: src/lib.rs:10] is on line 3.";
        let claims = parse_claims(md).claims;
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].line, 3);
    }

    #[test]
    fn a_claim_is_live_unless_it_is_a_sample_or_an_instruction() {
        let md = "\
Inline [file: src/real.rs:5] is a claim.

````markdown
Quoting a rule that itself opens a fence:
```rust
let x = [file: src/inside.rs:99];
```
````

- A nested bullet is not code:
    - and its claim [file: src/nested.rs:1] is live.

<!-- An instruction, though, is not an assertion:
     [file: src/commented.rs:1] -->

Back outside: [file: src/after.rs:7].
";
        assert_eq!(
            paths(md),
            vec!["src/real.rs", "src/nested.rs", "src/after.rs"],
            "a nested bullet is where a rule names its owners; a comment is an \
             instruction and a fence is a sample"
        );
    }

    #[test]
    fn a_document_the_renderer_reads_as_many_lines_is_not_read_as_one() {
        // CR-delimited and BOM-prefixed documents reach a renderer as many
        // lines. Read as one, the whole file collapsed into a single
        // fence-opening line and every claim in it went unparsed — a gate
        // reporting clean over a document it never opened.
        for (label, md) in [
            (
                "carriage returns",
                "\r```\r[file: src/sample.rs:1]\r```\r\r[file: src/real.rs:1]\r",
            ),
            (
                "a byte-order mark",
                "\u{feff}```\n[file: src/sample.rs:1]\n```\n\n[file: src/real.rs:1]\n",
            ),
        ] {
            assert_eq!(paths(md), vec!["src/real.rs"], "with {label}");
        }
    }

    #[test]
    fn a_delimiter_left_open_is_reported_rather_than_read_as_no_claims() {
        let scan = parse_claims("[file: src/before.rs:1]\n\n```\n[file: src/after.rs:1]\n");
        assert_eq!(paths_of(&scan), vec!["src/before.rs"]);
        assert_eq!(scan.unread, Some(Unclosed::Fence { line: 3 }));

        let scan = parse_claims("<!--\n[file: src/after.rs:1]\n");
        assert_eq!(scan.unread, Some(Unclosed::Comment { line: 1 }));

        assert_eq!(parse_claims("[file: src/x.rs:1]\n").unread, None);
    }

    fn paths_of(scan: &ClaimScan) -> Vec<String> {
        scan.claims
            .iter()
            .filter_map(|c| match &c.kind {
                ClaimKind::File { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn supports_tilde_fenced_blocks() {
        let md = "\
~~~text
[file: src/inside.txt:1]
~~~

[file: src/outside.md:2]
";
        assert_eq!(paths(md), vec!["src/outside.md"]);
    }
}
