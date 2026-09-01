//! # Advisory evidence — the freshness gate over recorded measurements
//!
//! An advisory is a measurement this toolkit never runs: expensive,
//! stochastic, or judgment-shaped, its findings never gate. What gates is
//! the freshness of its BASIS: the recorded baseline declares the inputs it
//! measured and their content digests, and [`AdvisoryAuditor`] asks only
//! "has the evidence stopped describing its inputs" — the digests moved,
//! the instrument moved, the declaration moved, or nothing was ever
//! recorded. It never asks "did it get worse"; the payload is the
//! project's to judge.
//!
//! ## What this module refuses to do
//!
//! - **Never run a measurement.** Recording takes a payload the project's
//!   own instrument produced; re-running anything is the operator's act.
//! - **Never fabricate a zero.** A declared advisory with no recorded
//!   baseline is `advisory-unmeasured`, not an empty pass — "never
//!   recorded" and "measured clean" are different facts.
//! - **Never judge the payload.** It is stored and returned opaque; a
//!   freshness gate that also graded results would absorb the measurement's
//!   flakiness into the gate, which is what this class exists to avoid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{AdvisoryDecl, EvidenceConfig};
use crate::envelope::{Finding, Location, Severity};
use crate::error::{Error, Result};

/// One recorded baseline, as `<advisory_dir>/<id>.json` holds it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryEvidence {
    pub id: String,
    /// When the measurement was recorded (RFC 3339).
    pub recorded_at: String,
    /// Digest per declared input path, as of recording.
    pub inputs: BTreeMap<String, String>,
    /// Digest per declared engine path, as of recording.
    #[serde(default)]
    pub engine: BTreeMap<String, String>,
    /// The measurement itself, opaque to this toolkit.
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// Content digest of one declared path — a file's bytes, or a directory's
/// files walked in sorted order.
///
/// Every field is length-prefixed before it is fed, so no byte sequence
/// inside a name or a file can imitate a boundary — a file split, merged,
/// or renamed always moves the digest (separator schemes cannot frame
/// arbitrary binary). Names are joined with `/` regardless of platform and
/// must be UTF-8; a name this grammar cannot spell is refused rather than
/// lossily collapsed. A symlink is digested as its own link text and never
/// followed: following would admit cycles, reach outside the project, and
/// make the link the alias its target pretends not to be — declare the
/// real path instead. FNV-1a, the toolkit's committed-digest primitive
/// (`spec::digest` states why not `DefaultHasher`): drift detection over
/// the operator's own tree, not an adversarial boundary.
fn digest_path(root: &Path, declared: &str) -> Result<String> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    let mut entries = Vec::new();
    collect_entries(&root.join(declared), declared.to_string(), &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, kind) in entries {
        feed(rel.as_bytes());
        match kind {
            EntryKind::File(path) => {
                let bytes = std::fs::read(&path).map_err(|e| Error::IoFailure {
                    path: path.clone(),
                    source: e,
                })?;
                feed(&bytes);
            }
            EntryKind::Link(target) => feed(target.as_bytes()),
        }
    }
    Ok(format!("fnv1a:{hash:016x}"))
}

enum EntryKind {
    File(PathBuf),
    Link(String),
}

fn collect_entries(path: &Path, rel: String, out: &mut Vec<(String, EntryKind)>) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path).map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let target = target.to_str().ok_or_else(|| Error::ConfigInvalid {
            message: format!("symlink target under '{rel}' is not UTF-8"),
            location: None,
        })?;
        out.push((rel, EntryKind::Link(target.to_string())));
        return Ok(());
    }
    if meta.is_file() {
        out.push((rel, EntryKind::File(path.to_path_buf())));
        return Ok(());
    }
    for entry in std::fs::read_dir(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })? {
        let entry = entry.map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| Error::ConfigInvalid {
            message: format!("file name under '{rel}' is not UTF-8"),
            location: None,
        })?;
        collect_entries(&entry.path(), format!("{rel}/{name}"), out)?;
    }
    Ok(())
}

fn digests_of(root: &Path, declared: &[String]) -> Result<BTreeMap<String, String>> {
    declared
        .iter()
        .map(|p| Ok((p.clone(), digest_path(root, p)?)))
        .collect()
}

/// Where an advisory's baseline lives.
pub fn evidence_path(root: &Path, cfg: &EvidenceConfig, id: &str) -> PathBuf {
    root.join(&cfg.advisory_dir).join(format!("{id}.json"))
}

/// Record a measurement: digest the declared inputs and engine NOW, and
/// write the baseline atomically. A declared path that does not exist is an
/// error — evidence recorded over a missing input would be fresh forever.
pub fn record(
    root: &Path,
    cfg: &EvidenceConfig,
    id: &str,
    payload: serde_json::Value,
) -> Result<AdvisoryEvidence> {
    // Config::validate also holds these, but a library caller may hand a
    // config it built itself, and an id or dir that shapes a path outside
    // the advisory directory must not reach the write — the same call
    // EvidenceVerifier::new makes on its strategies.
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || id.is_empty()
        || !crate::path_guard::literal_relative(&cfg.advisory_dir)
    {
        return Err(Error::ConfigInvalid {
            message: format!("advisory id '{id}' or advisory_dir is not a shape record can honor"),
            location: None,
        });
    }
    let Some(decl) = cfg.advisories.iter().find(|a| a.id == id) else {
        return Err(Error::ConfigInvalid {
            message: format!(
                "advisory '{id}' is not declared — add it under [[evidence.advisories]]"
            ),
            location: None,
        });
    };
    let path = evidence_path(root, cfg, id);
    if payload.is_null()
        && let Ok(raw) = std::fs::read_to_string(&path)
        && let Ok(existing) = serde_json::from_str::<AdvisoryEvidence>(&raw)
        && !existing.payload.is_null()
    {
        return Err(Error::ConfigInvalid {
            message: format!(
                "recording without a payload would discard the measurement '{id}' holds — pass                  --payload with the new measurement, or delete the baseline first"
            ),
            location: None,
        });
    }
    let evidence = AdvisoryEvidence {
        id: id.to_string(),
        recorded_at: jiff::Timestamp::now().to_string(),
        inputs: digests_of(root, &decl.inputs)?,
        engine: digests_of(root, &decl.engine)?,
        payload,
    };
    let body = serde_json::to_string_pretty(&evidence).map_err(|e| Error::ConfigInvalid {
        message: format!("serialize evidence: {e}"),
        location: None,
    })?;
    crate::path_guard::write_atomic(&path, format!("{body}\n").as_bytes())?;
    Ok(evidence)
}

/// Cross-input check: every declared advisory's baseline still describes
/// its inputs.
pub struct AdvisoryAuditor<'a> {
    root: &'a Path,
    cfg: &'a EvidenceConfig,
    /// An unattended context may block only on staleness the person being
    /// blocked can clear in the same sitting (`unattended_remeasure`);
    /// everything else reports without gating there.
    unattended: bool,
}

impl<'a> AdvisoryAuditor<'a> {
    pub fn new(root: &'a Path, cfg: &'a EvidenceConfig, unattended: bool) -> Self {
        Self {
            root,
            cfg,
            unattended,
        }
    }

    fn staleness_severity(&self, decl: &AdvisoryDecl) -> Severity {
        if self.unattended && !decl.unattended_remeasure {
            Severity::Minor
        } else {
            Severity::Major
        }
    }

    pub fn audit(&self) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for decl in &self.cfg.advisories {
            let path = evidence_path(self.root, self.cfg, &decl.id);
            if !path.is_file() {
                // Major in every context, deliberately outside the
                // unattended split: staleness is drift, absence is a
                // declaration never honored — downgrading it would let an
                // unattended-only pipeline hold "declared, never measured"
                // below the gate forever.
                findings.push(Finding {
                    slug: "advisory-unmeasured".into(),
                    severity: Severity::Major,
                    location: Location::file(path),
                    message: format!("advisory '{}' has no recorded evidence", decl.id),
                    hint: Some(format!(
                        "run the measurement and record it: `harnex evidence record --id {}`",
                        decl.id
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                continue;
            }
            let raw = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            let evidence: AdvisoryEvidence = match serde_json::from_str(&raw) {
                Ok(e) => e,
                Err(e) => {
                    findings.push(Finding {
                        slug: "advisory-evidence-invalid".into(),
                        severity: Severity::Major,
                        location: Location::file(path),
                        message: format!("evidence does not parse: {e}"),
                        hint: Some("re-record it; the schema is closed".into()),
                        auto_fixable: false,
                        fix_command: None,
                    });
                    continue;
                }
            };
            if evidence.id != decl.id {
                findings.push(Finding {
                    slug: "advisory-evidence-invalid".into(),
                    severity: Severity::Major,
                    location: Location::file(path),
                    message: format!(
                        "evidence carries id '{}' where '{}' was declared — a copied baseline is                          not a recording",
                        evidence.id, decl.id
                    ),
                    hint: Some("re-record it under its own declaration".into()),
                    auto_fixable: false,
                    fix_command: None,
                });
                continue;
            }
            self.audit_axis(
                decl,
                &path,
                "advisory-stale-input",
                "input",
                &decl.inputs,
                &evidence.inputs,
                &mut findings,
            );
            self.audit_axis(
                decl,
                &path,
                "advisory-stale-engine",
                "engine",
                &decl.engine,
                &evidence.engine,
                &mut findings,
            );
        }
        self.audit_orphans(&mut findings)?;
        Ok(findings)
    }

    /// One declared axis (inputs or engine) against what the baseline
    /// recorded. The declaration moving is staleness exactly as content
    /// moving is: the evidence was measured over a different basis.
    #[allow(clippy::too_many_arguments)]
    fn audit_axis(
        &self,
        decl: &AdvisoryDecl,
        path: &Path,
        slug: &str,
        axis: &str,
        declared: &[String],
        recorded: &BTreeMap<String, String>,
        findings: &mut Vec<Finding>,
    ) {
        let severity = self.staleness_severity(decl);
        let mut stale: Vec<String> = Vec::new();
        for entry in declared {
            match recorded.get(entry) {
                None => stale.push(format!("{entry} (not in the recorded basis)")),
                Some(recorded_digest) => match digest_path(self.root, entry) {
                    Ok(now) if &now == recorded_digest => {}
                    Ok(_) => stale.push(entry.clone()),
                    Err(_) => stale.push(format!(
                        "{entry} (no longer readable — restore or re-declare it first; record \
                         refuses a missing input)"
                    )),
                },
            }
        }
        for entry in recorded.keys() {
            if !declared.iter().any(|d| d == entry) {
                stale.push(format!("{entry} (no longer declared)"));
            }
        }
        if stale.is_empty() {
            return;
        }
        findings.push(Finding {
            slug: slug.into(),
            severity,
            location: Location::file(path.to_path_buf()),
            message: format!(
                "advisory '{}': the evidence no longer describes its {axis}s: {}",
                decl.id,
                stale.join(", ")
            ),
            hint: Some(format!(
                "re-measure and `harnex evidence record --id {}` — the advisory itself never \
                 gates, its basis going stale is what does",
                decl.id
            )),
            auto_fixable: false,
            fix_command: None,
        });
    }

    /// A baseline whose declaration is gone: report it, per the symmetric
    /// lifecycle — nothing long-lived enters without an exit path.
    fn audit_orphans(&self, findings: &mut Vec<Finding>) -> Result<()> {
        let dir = self.root.join(&self.cfg.advisory_dir);
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| Error::IoFailure {
            path: dir.clone(),
            source: e,
        })? {
            let entry = entry.map_err(|e| Error::IoFailure {
                path: dir.clone(),
                source: e,
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
                .map(str::to_string)
            else {
                continue;
            };
            if !self.cfg.advisories.iter().any(|a| a.id == id) {
                findings.push(Finding {
                    slug: "advisory-evidence-orphaned".into(),
                    severity: Severity::Minor,
                    location: Location::file(path),
                    message: format!("evidence '{id}' has no [[evidence.advisories]] declaration"),
                    hint: Some(
                        "the advisory was retired or renamed — delete the baseline, or restore \
                         the declaration"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdvisoryDecl;

    fn cfg(advisories: Vec<AdvisoryDecl>) -> EvidenceConfig {
        EvidenceConfig {
            default_provenance: "internal".into(),
            block_on_memory_only: false,
            verifiers: Vec::new(),
            advisory_dir: "evidence".into(),
            advisories,
        }
    }

    fn decl(
        id: &str,
        inputs: &[&str],
        engine: &[&str],
        unattended_remeasure: bool,
    ) -> AdvisoryDecl {
        AdvisoryDecl {
            id: id.into(),
            inputs: inputs.iter().map(|s| (*s).into()).collect(),
            engine: engine.iter().map(|s| (*s).into()).collect(),
            unattended_remeasure,
        }
    }

    fn slugs(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.slug.as_str()).collect()
    }

    #[test]
    fn unmeasured_is_a_finding_never_an_empty_pass() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &[], false)]);
        let findings = AdvisoryAuditor::new(root.path(), &cfg, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-unmeasured"]);
        assert_eq!(findings[0].severity, Severity::Major);
    }

    #[test]
    fn fresh_evidence_is_silent_and_a_changed_input_is_stale() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/a.css"), "a { color: red }\n").unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &[], false)]);
        record(
            root.path(),
            &cfg,
            "contrast",
            serde_json::json!({"pairs": 12}),
        )
        .unwrap();

        let auditor = AdvisoryAuditor::new(root.path(), &cfg, false);
        assert!(auditor.audit().unwrap().is_empty());

        std::fs::write(root.path().join("src/a.css"), "a { color: blue }\n").unwrap();
        let findings = auditor.audit().unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-stale-input"]);
        assert!(findings[0].message.contains("src"));
    }

    #[test]
    fn the_declaration_moving_is_staleness_too() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::create_dir_all(root.path().join("styles")).unwrap();
        std::fs::write(root.path().join("styles/b.css"), "b {}\n").unwrap();
        let recorded_under = cfg(vec![decl("contrast", &["src"], &[], false)]);
        record(
            root.path(),
            &recorded_under,
            "contrast",
            serde_json::Value::Null,
        )
        .unwrap();

        let now_declared = cfg(vec![decl("contrast", &["src", "styles"], &[], false)]);
        let findings = AdvisoryAuditor::new(root.path(), &now_declared, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-stale-input"]);
        assert!(findings[0].message.contains("not in the recorded basis"));
    }

    #[test]
    fn the_engine_moving_is_named_apart_from_the_subject() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("measure.sh"), "v1\n").unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &["measure.sh"], false)]);
        record(root.path(), &cfg, "contrast", serde_json::Value::Null).unwrap();
        std::fs::write(root.path().join("measure.sh"), "v2\n").unwrap();
        let findings = AdvisoryAuditor::new(root.path(), &cfg, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-stale-engine"]);
    }

    #[test]
    fn unattended_staleness_gates_only_where_remeasure_is_clearable() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/a"), "1").unwrap();
        let expensive = cfg(vec![decl("slow", &["src"], &[], false)]);
        let cheap = cfg(vec![decl("fast", &["src"], &[], true)]);
        record(root.path(), &expensive, "slow", serde_json::Value::Null).unwrap();
        record(root.path(), &cheap, "fast", serde_json::Value::Null).unwrap();
        std::fs::write(root.path().join("src/a"), "2").unwrap();

        let sev = |cfg: &EvidenceConfig, unattended: bool| {
            AdvisoryAuditor::new(root.path(), cfg, unattended)
                .audit()
                .unwrap()
                .iter()
                .find(|f| f.slug == "advisory-stale-input")
                .map(|f| f.severity)
                .unwrap()
        };
        assert_eq!(sev(&expensive, true), Severity::Minor);
        assert_eq!(sev(&expensive, false), Severity::Major);
        assert_eq!(sev(&cheap, true), Severity::Major);
    }

    #[test]
    fn a_file_split_cannot_imitate_the_recorded_boundaries() {
        // Reproduced pre-fix: with separator framing, one file whose bytes
        // spell "b <sep> styles/c <sep> d" digested like the two files
        // {a: "b", c: "d"} and a real tree change read fresh.
        let root = tempfile::tempdir().unwrap();
        let styles = root.path().join("styles");
        std::fs::create_dir_all(&styles).unwrap();
        std::fs::write(styles.join("a"), "b").unwrap();
        std::fs::write(styles.join("c"), "d").unwrap();
        let cfg = cfg(vec![decl("contrast", &["styles"], &[], false)]);
        record(root.path(), &cfg, "contrast", serde_json::Value::Null).unwrap();

        std::fs::remove_file(styles.join("c")).unwrap();
        let mut forged = b"b".to_vec();
        forged.push(0x1f);
        forged.extend_from_slice(b"styles/c");
        forged.push(0x1f);
        forged.extend_from_slice(b"d");
        std::fs::write(styles.join("a"), forged).unwrap();

        let findings = AdvisoryAuditor::new(root.path(), &cfg, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-stale-input"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_digests_as_its_link_text_and_a_cycle_cannot_hang() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(root.path().join("real.css"), "v1").unwrap();
        std::os::unix::fs::symlink("../real.css", src.join("link.css")).unwrap();
        std::os::unix::fs::symlink("..", src.join("cycle")).unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &[], false)]);
        record(root.path(), &cfg, "contrast", serde_json::Value::Null).unwrap();

        // Content behind the link is outside the basis; the link text is in it.
        std::fs::write(root.path().join("real.css"), "v2").unwrap();
        let auditor = AdvisoryAuditor::new(root.path(), &cfg, false);
        assert!(auditor.audit().unwrap().is_empty());
        std::fs::remove_file(src.join("link.css")).unwrap();
        std::os::unix::fs::symlink("../other.css", src.join("link.css")).unwrap();
        assert_eq!(
            slugs(&auditor.audit().unwrap()),
            vec!["advisory-stale-input"]
        );
    }

    #[test]
    fn a_library_caller_cannot_shape_a_path_out_of_the_advisory_dir() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        for (bad_id, dir) in [
            ("../escape", "evidence"),
            ("a/b", "evidence"),
            ("ok", "/tmp/evidence"),
        ] {
            let mut cfg = cfg(vec![decl(bad_id, &["src"], &[], false)]);
            cfg.advisory_dir = dir.into();
            assert!(
                record(root.path(), &cfg, bad_id, serde_json::Value::Null).is_err(),
                "{bad_id}/{dir} accepted"
            );
        }
    }

    #[test]
    fn a_copied_baseline_is_not_a_recording() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/a"), "1").unwrap();
        let cfg = cfg(vec![
            decl("original", &["src"], &[], false),
            decl("renamed", &["src"], &[], false),
        ]);
        record(root.path(), &cfg, "original", serde_json::Value::Null).unwrap();
        std::fs::copy(
            root.path().join("evidence/original.json"),
            root.path().join("evidence/renamed.json"),
        )
        .unwrap();
        let findings = AdvisoryAuditor::new(root.path(), &cfg, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-evidence-invalid"]);
        assert!(findings[0].message.contains("copied baseline"));
    }

    #[test]
    fn recording_without_a_payload_never_discards_a_measurement_in_silence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/a"), "1").unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &[], false)]);
        record(
            root.path(),
            &cfg,
            "contrast",
            serde_json::json!({"pairs": 3}),
        )
        .unwrap();
        let err = record(root.path(), &cfg, "contrast", serde_json::Value::Null).unwrap_err();
        assert!(err.to_string().contains("discard"));
        // A freshness-only baseline re-stamps freely.
        let bare = cfg;
        record(
            root.path(),
            &bare,
            "contrast",
            serde_json::json!({"pairs": 4}),
        )
        .unwrap();
        record(root.path(), &bare, "contrast", serde_json::json!(null)).unwrap_err();
    }

    #[test]
    fn absence_gates_every_context_and_the_split_is_pinned() {
        // Staleness is drift; absence is a declaration never honored — the
        // unattended split applies to the first and deliberately not the
        // second.
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        let cfg = cfg(vec![decl("slow", &["src"], &[], false)]);
        let findings = AdvisoryAuditor::new(root.path(), &cfg, true)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-unmeasured"]);
        assert_eq!(findings[0].severity, Severity::Major);
    }

    #[test]
    fn recording_a_missing_input_is_refused_and_an_orphan_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let cfg = cfg(vec![decl("contrast", &["src"], &[], false)]);
        assert!(record(root.path(), &cfg, "contrast", serde_json::Value::Null).is_err());
        assert!(record(root.path(), &cfg, "undeclared", serde_json::Value::Null).is_err());

        std::fs::create_dir_all(root.path().join("evidence")).unwrap();
        std::fs::write(root.path().join("evidence/retired.json"), "{}").unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/a"), "1").unwrap();
        record(root.path(), &cfg, "contrast", serde_json::Value::Null).unwrap();
        let findings = AdvisoryAuditor::new(root.path(), &cfg, false)
            .audit()
            .unwrap();
        assert_eq!(slugs(&findings), vec!["advisory-evidence-orphaned"]);
    }
}
