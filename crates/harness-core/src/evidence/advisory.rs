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
/// files walked in sorted order, each fed as (relative path, bytes).
///
/// FNV-1a with separator bytes, the toolkit's committed-digest primitive
/// (`spec::digest` states why not `DefaultHasher`): drift detection over the
/// operator's own tree, not an adversarial boundary.
fn digest_path(root: &Path, declared: &str) -> Result<String> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    const UNIT: u8 = 0x1f;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= u64::from(UNIT);
        hash = hash.wrapping_mul(PRIME);
    };
    let base = root.join(declared);
    let mut files = Vec::new();
    collect_files(&base, &mut files)?;
    files.sort();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(&file).map_err(|e| Error::IoFailure {
            path: file.clone(),
            source: e,
        })?;
        feed(rel.as_bytes());
        feed(&bytes);
    }
    Ok(format!("fnv1a:{hash:016x}"))
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let meta = std::fs::metadata(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;
    if meta.is_file() {
        out.push(path.to_path_buf());
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
        collect_files(&entry.path(), out)?;
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
    let Some(decl) = cfg.advisories.iter().find(|a| a.id == id) else {
        return Err(Error::ConfigInvalid {
            message: format!(
                "advisory '{id}' is not declared — add it under [[evidence.advisories]]"
            ),
            location: None,
        });
    };
    let evidence = AdvisoryEvidence {
        id: id.to_string(),
        recorded_at: jiff::Timestamp::now().to_string(),
        inputs: digests_of(root, &decl.inputs)?,
        engine: digests_of(root, &decl.engine)?,
        payload,
    };
    let path = evidence_path(root, cfg, id);
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
                    Err(_) => stale.push(format!("{entry} (no longer readable)")),
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
