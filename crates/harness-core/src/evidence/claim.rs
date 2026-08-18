//! # Claim parser
//!
//! Extracts provenance-marked claims from arbitrary markdown text.
//! Recognised syntaxes (all whitespace-tolerant):
//!
//! - `` `path/to/file.ext:42` `` → internal file/line claim
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
static FILE_CLAIM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[file:\s*([^\]\s:]+)(?::(\d+))?\s*\]").expect("FILE_CLAIM regex")
});

/// Parse every recognised claim out of `markdown`. Order within a line is
/// the order discovered by the per-pattern regex pass.
///
/// Lines inside fenced code blocks (` ``` ` … ` ``` `) are skipped — the
/// backtick path syntax inside code samples is documentation, not a claim
/// the toolkit should verify.
pub fn parse_claims(markdown: &str) -> Vec<Claim> {
    let mut out = Vec::new();
    let mut in_fence: Option<(char, usize)> = None;
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
            match in_fence {
                Some((open_char, open_len)) if char == open_char && len >= open_len => {
                    in_fence = None;
                }
                Some(_) => {}
                None => in_fence = Some((char, len)),
            }
            continue;
        }
        // A four-space indent is a code block too, and markdown does not
        // require a fence for one.
        if in_fence.is_some() || line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }

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

        for cap in FILE_CLAIM.captures_iter(line) {
            out.push(Claim {
                raw: cap[0].to_string(),
                provenance: Some("internal".to_string()),
                kind: ClaimKind::FilePathLine {
                    path: cap[1].to_string(),
                    // The regex guarantees the group is all digits, so the
                    // only parse failure is OVERFLOW of u32 — a line number
                    // far beyond any file. Map it to u32::MAX so the verifier
                    // reports it as out-of-range, never silently as "no line
                    // to check" (which would let a bogus claim pass).
                    line: cap.get(2).map(|m| m.as_str().parse().unwrap_or(u32::MAX)),
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
    fn skips_claims_inside_code_blocks_however_they_are_written() {
        let md = "\
Inline [file: src/real.rs:5] is a claim.

````markdown
Quoting a rule that itself opens a fence:
```rust
let x = [file: src/inside.rs:99];
```
````

    An indented block also holds [file: src/indented.rs:1].

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
        assert_eq!(paths, vec!["src/real.rs", "src/after.rs"]);
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
