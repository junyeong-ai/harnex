//! Integration tests for the evidence module.

use std::path::Path;

use harness_core::config::{EvidenceConfig, VerifierDecl};
use harness_core::envelope::Severity;
use harness_core::evidence::EvidenceVerifier;
use tempfile::TempDir;

fn block_strict_config() -> EvidenceConfig {
    EvidenceConfig {
        advisory_dir: "evidence".into(),
        advisories: Vec::new(),
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

    let markdown = "See [file: src/lib.rs:2].";
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

    let markdown = "See [file: ../secret.txt:1].";
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
    let markdown = "See [file: src/missing.rs:5].";
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

    let markdown = "See [file: src/lib.rs:99].";
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message, "line 99 out of range ('src/lib.rs' has 1 lines)",
        "the range a claim missed is what tells the author where to point instead"
    );
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

    let markdown = "See [file: src/lib.rs:999999999999999999999].";
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

#[test]
fn a_section_anchor_resolves_against_the_heading_the_file_spells() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join(".claude/rules/plan.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "# Plan\n\n## Escape hatch\n\nbody\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(
        "Stated in [file: .claude/rules/plan.md § Escape hatch].",
        Path::new("test.md"),
        tmp.path(),
    );
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");

    let findings = verifier.verify_text(
        "Stated in [file: .claude/rules/plan.md § Escape hatches].",
        Path::new("test.md"),
        tmp.path(),
    );
    assert_eq!(findings.len(), 1, "a renamed heading must fail the gate");
    assert_eq!(findings[0].severity, Severity::Blocker);
    assert_eq!(findings[0].slug, "evidence-internal");
}

#[test]
fn a_symbol_anchor_resolves_against_what_the_file_spells_and_not_against_a_longer_name() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(
        &target,
        "pub fn write_atomic() {}\n\npub fn write_atomic_rejects() {}\n",
    )
    .unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let resolves = |claim: &str| {
        verifier
            .verify_text(claim, Path::new("test.md"), tmp.path())
            .is_empty()
    };

    assert!(
        resolves("Stated in [file: src/lib.rs :: pub fn write_atomic]."),
        "the declaration the file spells must resolve"
    );
    assert!(
        !resolves("Stated in [file: src/lib.rs :: pub fn write_atom]."),
        "a prefix of a longer name must not resolve"
    );
    assert!(
        !resolves("Stated in [file: src/lib.rs :: pub fn renamed_away]."),
        "the rename that invalidates the claim must fail the gate"
    );
}

#[test]
fn a_symbol_naming_two_places_names_neither() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(
        &target,
        "mod a {\n    pub fn load() {}\n}\nmod b {\n    pub fn load() {}\n}\n",
    )
    .unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(
        "Stated in [file: src/lib.rs :: pub fn load].",
        Path::new("test.md"),
        tmp.path(),
    );
    assert_eq!(findings.len(), 1, "two occurrences must fail the gate");
    assert_eq!(findings[0].severity, Severity::Blocker);
    assert!(
        findings[0].message.contains("occurs 2 times"),
        "the finding must say how many places answer to it: {}",
        findings[0].message
    );
}

#[test]
fn two_places_that_share_a_character_are_still_two_places() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "use crate::a::b::b::c;\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(
        "Stated in [file: src/lib.rs :: ::b::].",
        Path::new("test.md"),
        tmp.path(),
    );
    assert_eq!(
        findings.len(),
        1,
        "'::b::' reads at two offsets here, sharing the '::' between them"
    );
    assert!(
        findings[0].message.contains("occurs 2 times"),
        "an overlap is two places, not one: {}",
        findings[0].message
    );
}

#[test]
fn the_scan_steps_by_characters_and_not_by_bytes() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "\u{ac00}::b::b::\u{b098}\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let findings = verifier.verify_text(
        "Stated in [file: src/lib.rs :: ::b::].",
        Path::new("test.md"),
        tmp.path(),
    );
    assert_eq!(
        findings.len(),
        1,
        "a multi-byte neighbour must not move where an occurrence starts"
    );
    assert!(
        findings[0].message.contains("occurs 2 times"),
        "the count is the same one the ASCII file gives: {}",
        findings[0].message
    );
}

#[test]
fn a_line_anchor_pointing_at_a_blank_line_points_at_nothing() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "pub fn a() {}\n\npub fn b() {}\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    assert!(
        verifier
            .verify_text("[file: src/lib.rs:1].", Path::new("test.md"), tmp.path())
            .is_empty(),
        "a line carrying text resolves"
    );

    let findings = verifier.verify_text("[file: src/lib.rs:2].", Path::new("test.md"), tmp.path());
    assert_eq!(
        findings.len(),
        1,
        "a claim on a blank line names nothing and must fail"
    );
    assert_eq!(findings[0].severity, Severity::Blocker);
}

#[test]
fn the_leftmost_reserved_separator_decides_which_anchor_a_body_names() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("doc.md"), "# D\n\n## A :: B\n\nbody\n").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "pub fn a() {} // § b\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    let resolves = |claim: &str| {
        verifier
            .verify_text(claim, Path::new("test.md"), tmp.path())
            .is_empty()
    };

    assert!(
        resolves("[file: doc.md § A :: B]."),
        "a heading may carry the symbol separator, because the leftmost one decides"
    );
    assert!(
        resolves("[file: lib.rs :: pub fn a() {} // § b]."),
        "a symbol may carry the section separator, for the same reason"
    );
}

#[test]
fn a_body_with_nothing_after_its_separator_never_named_an_anchor() {
    // A claim carries the trimmed interior of its marker, and each separator
    // is spaced, so a body with whitespace on either side of one never matches
    // it. The path the verifier reports is what proves both: an untrimmed body
    // would split here and hand the anchor nothing to resolve.
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("t.rs"), "pub fn a() {}\n").unwrap();

    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();
    for (body, path) in [
        ("t.rs :: ", "t.rs ::"),
        ("t.rs ::   ", "t.rs ::"),
        (" :: sym", ":: sym"),
        ("t.md § ", "t.md §"),
        (" § Head", "§ Head"),
    ] {
        let findings =
            verifier.verify_text(&format!("[file: {body}]"), Path::new("test.md"), tmp.path());
        assert_eq!(findings.len(), 1, "`{body}` must raise one finding");
        assert_eq!(
            findings[0].message,
            format!("file '{path}' does not exist"),
            "`{body}` must reach the verifier as that path, not as an empty anchor"
        );
    }

    assert!(
        verifier
            .verify_text("[file: ]", Path::new("test.md"), tmp.path())
            .is_empty(),
        "an empty body says nothing to check and is not a claim"
    );
}

#[test]
fn a_sample_is_a_sample_wherever_its_container_indents_it() {
    // Each of these was a Blocker against a path the author wrote as an
    // example: the fence opens at a column a line-at-a-time reader reads as
    // too deep, or CommonMark closes it at the end of the document and a
    // state machine called that unterminated.
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "a\nb\nc\n").unwrap();
    let verifier = EvidenceVerifier::new(&block_strict_config()).unwrap();

    for (label, markdown) in [
        (
            "inside a list item",
            "- Like this:\n\n    ```markdown\n    [file: no/such.rs:1]\n    ```\n\n\
             Real: [file: src/lib.rs:2].\n",
        ),
        (
            "inside a block quote",
            "> ```\n> [file: no/such.rs:1]\n> ```\n\nReal: [file: src/lib.rs:2].\n",
        ),
        (
            "indented code",
            "Prose.\n\n    [file: no/such.rs:1]\n\nReal: [file: src/lib.rs:2].\n",
        ),
        (
            "terminated by the document",
            "Real: [file: src/lib.rs:2].\n\n```\n[file: no/such.rs:1]\n",
        ),
    ] {
        let findings = verifier.verify_text(markdown, Path::new("test.md"), tmp.path());
        assert!(findings.is_empty(), "with {label}: {findings:#?}");
    }
}
