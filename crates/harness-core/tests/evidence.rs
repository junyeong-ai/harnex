//! Integration tests for the evidence module.

use std::path::Path;

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::envelope::Severity;
use harness_core::evidence::EvidenceVerifier;
use tempfile::TempDir;

fn block_strict_config() -> EvidenceConfig {
    EvidenceConfig {
        default_provenance: "memory-only".to_string(),
        block_on_memory_only: true,
        verifiers: vec![
            VerifierDecl {
                provenance: "internal".to_string(),
                strategy: "file-path-line".to_string(),
                library_allowlist: vec![],
                max_age_days: None,
            },
            VerifierDecl {
                provenance: "memory-only".to_string(),
                strategy: "memory-only".to_string(),
                library_allowlist: vec![],
                max_age_days: None,
            },
            VerifierDecl {
                provenance: "fetched-url".to_string(),
                strategy: "fetched-url".to_string(),
                library_allowlist: vec![],
                max_age_days: Some(90),
            },
            VerifierDecl {
                provenance: "context7".to_string(),
                strategy: "context7".to_string(),
                library_allowlist: vec!["vercel/next.js".to_string()],
                max_age_days: None,
            },
        ],
    }
}

#[test]
fn passes_when_file_path_line_resolves() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "line1\nline2\nline3\n").unwrap();

    let markdown = "See `src/lib.rs:2`.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn rejects_path_traversal_outside_project() {
    // A claim path with `..` must not verify (or read) a file outside the
    // project root, even if that file exists.
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    // A real file just outside the project root.
    std::fs::write(tmp.path().join("secret.txt"), "x\n").unwrap();

    let markdown = "See `../secret.txt:1`.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), &project);
    assert_eq!(findings.len(), 1, "traversal claim must be a finding");
    assert_eq!(findings[0].slug, "evidence-internal");
    assert!(
        findings[0].message.contains("escapes the project root"),
        "expected traversal rejection, got: {}",
        findings[0].message
    );
}

#[test]
fn rejects_nonexistent_path() {
    let tmp = TempDir::new().unwrap();
    let markdown = "See `src/missing.rs:5`.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Blocker);
    assert_eq!(findings[0].slug, "evidence-internal");
}

#[test]
fn rejects_line_out_of_range() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "only one line\n").unwrap();

    let markdown = "See `src/lib.rs:99`.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("out of range"));
}

#[test]
fn rejects_overflowing_line_number() {
    // A line literal that overflows u32 must surface as out-of-range, never
    // be silently dropped to "no line to check" (which would pass on an
    // existing file).
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "only one line\n").unwrap();

    let markdown = "See `src/lib.rs:999999999999999999999`.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1, "overflowing line must be a finding");
    assert!(findings[0].message.contains("out of range"));
}

#[test]
fn memory_only_blocks_when_configured() {
    let tmp = TempDir::new().unwrap();
    let markdown = "Unverified [memory] claim.";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Blocker);
    assert_eq!(findings[0].slug, "evidence-memory-only");
}

#[test]
fn memory_only_warns_when_not_blocking() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = block_strict_config();
    cfg.block_on_memory_only = false;
    let markdown = "Unverified [memory] claim.";
    let verifier = EvidenceVerifier::new(&cfg).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(
        findings.len(),
        1,
        "expected one advisory finding: {findings:?}"
    );
    assert_eq!(findings[0].severity, Severity::Minor);
    assert_eq!(findings[0].slug, "evidence-memory-only");
}

#[test]
fn fetched_url_rejects_stale_date() {
    let tmp = TempDir::new().unwrap();
    let markdown = "See [fetched: 2020-01-01] https://example.com/old";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("old"));
}

#[test]
fn fetched_url_rejects_future_date() {
    let tmp = TempDir::new().unwrap();
    let markdown = "See [fetched: 2099-01-01] https://example.com/x";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("future"));
}

#[test]
fn context7_allowlist_rejects_unknown_library() {
    let tmp = TempDir::new().unwrap();
    let markdown = "Per [context7: bogus/library] docs the API is …";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0]
            .message
            .contains("not in the context7 allowlist")
    );
}

#[test]
fn context7_allowlist_accepts_listed_library() {
    let tmp = TempDir::new().unwrap();
    let markdown = "Per [context7: vercel/next.js] middleware fires …";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn verify_file_reads_and_reports() {
    let tmp = TempDir::new().unwrap();
    let md_path = tmp.path().join("plan.md");
    std::fs::write(&md_path, "Unverified [memory] claim.\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_file(&md_path, tmp.path()).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].location.path, md_path);
    assert_eq!(findings[0].location.line, Some(1));
}
