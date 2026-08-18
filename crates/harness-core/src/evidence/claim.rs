//! # Claim parser
//!
//! Extracts provenance-marked claims from arbitrary markdown text.
//! Recognised syntaxes (all whitespace-tolerant):
//!
//! - `[file: path/to/file.ext:42]` → internal file claim; the `:line` optional
//! - `[fetched: YYYY-MM-DD] https://...` → fetched-url claim
//! - `[context7: <library-id>]` → context7 claim
//! - `[memory]` → unverified memory claim
//!
//! Lines are 1-indexed (matches editor convention).

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone)]
pub struct Claim {
    pub raw: String,
    pub provenance: Option<String>,
    pub kind: ClaimKind,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub enum ClaimKind {
    FilePathLine {
        path: String,
        line: Option<u32>,
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

static FETCHED_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[fetched:\s*(\d{4}-\d{2}-\d{2})\]\s*(https?://\S+)").expect("FETCHED_URL regex")
});

static CONTEXT7: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[context7:\s*([A-Za-z0-9_@./\-]+)\]").expect("CONTEXT7 regex"));

static MEMORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[memory\]").expect("MEMORY regex"));

// `[file: path/to/thing.rs:42]` — an internal claim, marked like every other.
//
// The line is optional: `[file: pyproject.toml]` asserts the file, which is
// what a rule naming a config section as its owner needs.
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

/// The part of `line` outside HTML comments, carrying comment state across
/// lines.
///
/// A four-space indent used to be treated as a code block, which is only true
/// at the top level: inside a list it is a nested item, and a bullet list
/// naming an owner per item is exactly what the rule template asks an author to
/// write. Those claims were skipped and `check` reported clean — a gate that
/// verifies nothing while saying it verified. Fenced blocks still cover a
/// deliberate sample; a comment covers a template's instructions; nothing else
/// is exempt.
fn strip_comments(line: &str, in_comment: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        if *in_comment {
            let Some(close) = rest.find("-->") else {
                return out;
            };
            rest = &rest[close + 3..];
            *in_comment = false;
            continue;
        }
        let Some(open) = rest.find("<!--") else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + 4..];
        *in_comment = true;
    }
}

/// Split a claim body into its path and optional line.
///
/// The line is the trailing `:<digits>`, so a path may itself contain a colon
/// — a Windows drive letter reaches the verifier intact. An empty path is not
/// a claim: `[file: ]` says nothing to check.
fn split_file_claim(body: &str) -> Option<(&str, Option<u32>)> {
    if let Some((path, tail)) = body.rsplit_once(':')
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
    {
        return (!path.is_empty()).then(|| {
            // All digits, so the only parse failure is OVERFLOW of u32 — a line
            // far beyond any file. Map it to u32::MAX so the verifier reports
            // it out of range rather than silently as "no line to check".
            (path, Some(tail.parse().unwrap_or(u32::MAX)))
        });
    }
    (!body.is_empty()).then_some((body, None))
}

/// Parse every recognised claim out of `markdown`. Order within a line is
/// the order discovered by the per-pattern pass.
///
/// Two exemptions and no others: a fenced code block, which is a deliberate
/// sample of the syntax, and an HTML comment, which is an instruction. Prose,
/// tables, blockquotes and every depth of list item carry live claims — a
/// nested bullet is where a rule names its owners.
pub fn parse_claims(markdown: &str) -> Vec<Claim> {
    let mut out = Vec::new();
    let mut in_fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    for (idx, line) in markdown.lines().enumerate() {
        let line_no = (idx as u32) + 1;

        // Fence delimiters, per CommonMark: a closing fence must be at least
        // as long as the one that opened it. Toggling on any run of three
        // meant a four-backtick block quoting three-backtick examples — how a
        // rule about writing rules is spelled — closed itself at the first
        // inner fence and read the rest of the block as prose.
        let trimmed = line.trim_start();
        let run = |c: char| trimmed.chars().take_while(|&x| x == c).count();
        let fence = [('`', run('`')), ('~', run('~'))]
            .into_iter()
            .find(|&(_, len)| len >= 3);
        if let Some((char, len)) = fence {
            // A closing fence carries nothing but its own characters, per
            // CommonMark. Without that, a line like "``` note: still inside"
            // closes the block a renderer keeps open, and the claim below it
            // is read as live.
            let bare = trimmed[len..].trim().is_empty();
            match in_fence {
                Some((open_char, open_len)) if bare && char == open_char && len >= open_len => {
                    in_fence = None;
                }
                Some(_) => {}
                None => in_fence = Some((char, len)),
            }
            continue;
        }
        if in_fence.is_some() {
            continue;
        }
        // An HTML comment is instruction rather than assertion. Every template
        // that teaches this grammar writes an example inside one, and a rule
        // carrying a `harnex-fill` block would otherwise report the example as
        // a claim about the project.
        let live = strip_comments(line, &mut in_comment);
        let line = live.as_str();

        for cap in FETCHED_URL.captures_iter(line) {
            out.push(Claim {
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
            out.push(Claim {
                raw: cap[0].to_string(),
                provenance: Some("context7".to_string()),
                kind: ClaimKind::Context7Library {
                    library: cap[1].to_string(),
                },
                line: line_no,
            });
        }

        for _ in MEMORY.captures_iter(line) {
            out.push(Claim {
                raw: "[memory]".to_string(),
                provenance: Some("memory-only".to_string()),
                kind: ClaimKind::Memory,
                line: line_no,
            });
        }

        for body in file_claim_bodies(line) {
            let Some((path, cited)) = split_file_claim(body) else {
                continue;
            };
            out.push(Claim {
                raw: format!("{FILE_MARKER} {body}]"),
                provenance: Some("internal".to_string()),
                kind: ClaimKind::FilePathLine {
                    path: path.to_string(),
                    line: cited,
                },
                line: line_no,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ] {
            assert!(
                parse_claims(md).is_empty(),
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
        for (md, want_path, want_line) in [
            (
                "See [file: app/[id]/page.tsx:10] for the handler.",
                "app/[id]/page.tsx",
                Some(10),
            ),
            (
                "Catch-all [file: app/[[...slug]]/route.ts] is registered.",
                "app/[[...slug]]/route.ts",
                None,
            ),
            (
                "On Windows [file: C:/repo/x.rs:4].",
                "C:/repo/x.rs",
                Some(4),
            ),
            (
                "Spaces survive [file: My Docs/notes.md:2].",
                "My Docs/notes.md",
                Some(2),
            ),
        ] {
            let claims = parse_claims(md);
            assert_eq!(claims.len(), 1, "no claim parsed from: {md}");
            match &claims[0].kind {
                ClaimKind::FilePathLine { path, line } => {
                    assert_eq!(path, want_path, "from: {md}");
                    assert_eq!(*line, want_line, "from: {md}");
                }
                _ => panic!("expected FilePathLine"),
            }
        }
    }

    #[test]
    fn a_marker_with_nothing_to_check_is_not_a_claim() {
        for md in [
            "An empty one [file: ] says nothing.",
            "An unterminated [file: src/lib.rs:1 never closes.",
            "A link [file](https://example.com) is not a marker.",
        ] {
            assert!(parse_claims(md).is_empty(), "parsed a claim from: {md}");
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
        let paths: Vec<String> = parse_claims(md)
            .iter()
            .filter_map(|c| match &c.kind {
                ClaimKind::FilePathLine { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec!["src/after.rs"]);
    }

    #[test]
    fn a_file_claim_carries_an_optional_line() {
        // A rule naming a config section as its owner asserts the file; one
        // naming an item asserts the line too.
        match &parse_claims("Owned by [file: pyproject.toml].")[0].kind {
            ClaimKind::FilePathLine { path, line } => {
                assert_eq!(path, "pyproject.toml");
                assert_eq!(*line, None);
            }
            _ => panic!("expected FilePathLine"),
        }
        match &parse_claims("See [file: src/lib.rs:42] for context.")[0].kind {
            ClaimKind::FilePathLine { path, line } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(*line, Some(42));
            }
            _ => panic!("expected FilePathLine"),
        }
    }

    #[test]
    fn extracts_fetched_url() {
        let md = "Per [fetched: 2026-05-20] https://example.com/x the rule is …";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("fetched-url"));
    }

    #[test]
    fn extracts_context7() {
        let md = "Per [context7: vercel/next.js] middleware fires before …";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("context7"));
    }

    #[test]
    fn extracts_memory_marker() {
        let md = "The runtime [memory] does not re-read skills mid-turn.";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].provenance.as_deref(), Some("memory-only"));
    }

    #[test]
    fn ignores_prose_colons() {
        let md = "TODO: handle this case. Also remember: be precise.";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 0);
    }

    #[test]
    fn line_numbers_are_one_indexed() {
        let md = "intro line\n\n[file: src/lib.rs:10] is on line 3.";
        let claims = parse_claims(md);
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
        let claims = parse_claims(md);
        let paths: Vec<&str> = claims
            .iter()
            .filter_map(|c| match &c.kind {
                ClaimKind::FilePathLine { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            vec!["src/real.rs", "src/nested.rs", "src/after.rs"],
            "a nested bullet is where a rule names its owners; a comment is an \
             instruction and a fence is a sample"
        );
    }

    #[test]
    fn supports_tilde_fenced_blocks() {
        let md = "\
~~~text
[file: src/inside.txt:1]
~~~

[file: src/outside.md:2]
";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        match &claims[0].kind {
            ClaimKind::FilePathLine { path, .. } => assert_eq!(path, "src/outside.md"),
            _ => panic!("expected FilePathLine"),
        }
    }
}
