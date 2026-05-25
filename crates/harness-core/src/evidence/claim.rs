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

// A backtick-wrapped path that looks like `something/with.ext:42`.
// Restricted to relative-looking paths (no leading `/`) and to file
// extensions of 1–8 alphanumerics, to avoid matching prose like
// `foo:42` (no extension) or absolute /etc/passwd:42.
static FILE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"`([A-Za-z0-9_./\-]+\.[A-Za-z0-9]{1,8}):(\d+)`").expect("FILE_LINE regex")
});

/// Parse every recognised claim out of `markdown`. Order within a line is
/// the order discovered by the per-pattern regex pass.
///
/// Lines inside fenced code blocks (` ``` ` … ` ``` `) are skipped — the
/// backtick path syntax inside code samples is documentation, not a claim
/// the toolkit should verify.
pub fn parse_claims(markdown: &str) -> Vec<Claim> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (idx, line) in markdown.lines().enumerate() {
        let line_no = (idx as u32) + 1;

        // Fenced code block delimiter (``` or ~~~ at the start of a trimmed line).
        // Toggle fence state and skip the delimiter line itself.
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
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

        for cap in FILE_LINE.captures_iter(line) {
            out.push(Claim {
                raw: cap[0].to_string(),
                provenance: Some("internal".to_string()),
                kind: ClaimKind::FilePathLine {
                    path: cap[1].to_string(),
                    // The regex guarantees `cap[2]` is all digits, so the
                    // only parse failure is OVERFLOW of u32 — a line number
                    // far beyond any file. Map it to u32::MAX so the verifier
                    // reports it as out-of-range, never silently as "no line
                    // to check" (which would let a bogus claim pass).
                    line: Some(cap[2].parse().unwrap_or(u32::MAX)),
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
    fn extracts_file_path_line() {
        let md = "See `src/lib.rs:42` for context.";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        match &claims[0].kind {
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
        let md = "intro line\n\n`src/lib.rs:10` is on line 3.";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].line, 3);
    }

    #[test]
    fn skips_backtick_paths_inside_fenced_code_blocks() {
        let md = "\
Inline `src/real.rs:5` is a claim.

```rust
// Example code, not a claim:
let x = `src/inside.rs:99`;
```

Back outside: `src/after.rs:7`.
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
`src/inside.txt:1`
~~~

`src/outside.md:2`
";
        let claims = parse_claims(md);
        assert_eq!(claims.len(), 1);
        match &claims[0].kind {
            ClaimKind::FilePathLine { path, .. } => assert_eq!(path, "src/outside.md"),
            _ => panic!("expected FilePathLine"),
        }
    }
}
