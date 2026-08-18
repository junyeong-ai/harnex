//! Drift guard for the audit finding vocabulary.
//!
//! A slug is a wire contract: CI greps it out of the envelope and the plugin's
//! audit mode is what turns it into something an operator can act on. Both of
//! those live outside the Rust source, so the vocabulary needs a guard rather
//! than a convention — the check added most recently reached neither document,
//! and an operator would have seen a finding the skill could not name.
//!
//! Both directions, because each catches a different mistake: a new slug that
//! never reached a document, and a document still explaining a slug that was
//! renamed away (Constitution IX).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::audit::AuditFindingSlug;

/// The documents that must explain every slug, and what each is for.
const SLUG_DOCS: &[(&str, &str)] = &[
    (
        "plugins/harnex/SKILL.md",
        "the plugin's audit mode presents these findings to an operator",
    ),
    (
        ".claude/rules/audit.md",
        "the editing contract for the audit module",
    ),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    let full = repo_root().join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("read {}: {e}", full.display()))
}

/// Every `audit-…` token the document spells, on one line.
fn slugs_named_by(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in body.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("audit-") {
            let tail = &rest[at..];
            let end = tail
                .find(|c: char| !c.is_ascii_lowercase() && c != '-')
                .unwrap_or(tail.len());
            found.insert(tail[..end].trim_end_matches('-').to_string());
            rest = &tail[end.max(1)..];
        }
    }
    found
}

fn declared() -> BTreeSet<String> {
    AuditFindingSlug::ALL
        .iter()
        .map(|s| s.as_str().to_string())
        .collect()
}

#[test]
fn every_slug_round_trips() {
    for slug in AuditFindingSlug::ALL {
        assert_eq!(AuditFindingSlug::from_str(slug.as_str()), Some(*slug));
    }
    assert_eq!(
        declared().len(),
        AuditFindingSlug::ALL.len(),
        "AuditFindingSlug::ALL carries a duplicate wire string"
    );
}

#[test]
fn from_str_rejects_unknown() {
    assert!(AuditFindingSlug::from_str("audit-made-up").is_none());
}

/// Every slug the auditors can emit is explained where an operator reads.
#[test]
fn every_slug_is_documented() {
    for (path, purpose) in SLUG_DOCS {
        let named = slugs_named_by(&read(path));
        let missing: Vec<&str> = AuditFindingSlug::ALL
            .iter()
            .map(|s| s.as_str())
            .filter(|s| !named.contains(*s))
            .collect();
        assert!(
            missing.is_empty(),
            "{path} ({purpose}) does not name {missing:?} — a finding an operator \
             cannot act on. Spell each slug on one line."
        );
    }
}

/// No document explains a slug the auditors no longer emit.
#[test]
fn no_document_names_a_retired_slug() {
    let declared = declared();
    for (path, purpose) in SLUG_DOCS {
        let stale: Vec<String> = slugs_named_by(&read(path))
            .into_iter()
            .filter(|s| !declared.contains(s))
            .collect();
        assert!(
            stale.is_empty(),
            "{path} ({purpose}) names {stale:?}, which no auditor emits"
        );
    }
}

/// The emit sites go through the enum, so the vocabulary has one owner. A
/// literal at an emit site is the state this guard exists to prevent: it
/// bypasses `AuditFindingSlug::ALL`, and both checks above then pass while the
/// slug reaches no document.
#[test]
fn no_audit_module_emits_a_literal_slug() {
    let dir = repo_root().join("crates/harness-core/src/audit");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let body = std::fs::read_to_string(&path).unwrap();
        for (i, line) in body.lines().enumerate() {
            if line.contains("slug: \"") {
                offenders.push(format!("{}:{}", rel(&path), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{offenders:?} emit a slug as a literal — use AuditFindingSlug::<V>.as_str()"
    );
}

fn rel(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}
