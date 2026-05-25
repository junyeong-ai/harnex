//! Permission profile composition (`PermissionGenerator`) and audit
//! (`PermissionAuditor`).

use std::collections::{BTreeSet, HashSet};

use serde::Serialize;

use crate::config::PermissionsPolicy;
use crate::error::{Error, Result};

use super::profiles::PermissionProfile;

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct PermissionsBlock {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

pub struct PermissionGenerator<'a> {
    policy: &'a PermissionsPolicy,
}

impl<'a> PermissionGenerator<'a> {
    pub fn new(policy: &'a PermissionsPolicy) -> Result<Self> {
        for p in &policy.profiles {
            if !PermissionProfile::ALL.contains(&p.as_str()) {
                return Err(Error::PolicyProfileUnknown { name: p.clone() });
            }
        }
        Ok(Self { policy })
    }

    pub fn generate(&self) -> PermissionsBlock {
        let mut allow = BTreeSet::new();
        let mut ask = BTreeSet::new();
        let mut deny = BTreeSet::new();
        for name in &self.policy.profiles {
            if let Some(p) = PermissionProfile::from_str(name) {
                allow.extend(p.allow.iter().map(|s| s.to_string()));
                ask.extend(p.ask.iter().map(|s| s.to_string()));
                deny.extend(p.deny.iter().map(|s| s.to_string()));
            }
        }
        allow.extend(self.policy.extra_allow.iter().cloned());
        ask.extend(self.policy.extra_ask.iter().cloned());
        deny.extend(self.policy.extra_deny.iter().cloned());
        PermissionsBlock {
            allow: allow.into_iter().collect(),
            ask: ask.into_iter().collect(),
            deny: deny.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PermissionFinding {
    pub kind: PermissionFindingKind,
    pub message: String,
    pub rule: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionFindingKind {
    MissingBaselineDeny,
    ContradictoryRule,
}

pub struct PermissionAuditor<'a> {
    policy: &'a PermissionsPolicy,
    settings_allow: &'a [String],
    settings_ask: &'a [String],
    settings_deny: &'a [String],
}

impl<'a> PermissionAuditor<'a> {
    /// Validates profile names up front — symmetric with
    /// [`PermissionGenerator::new`]. An unknown profile would otherwise be
    /// silently skipped by [`Self::audit`], dropping an intended guardrail
    /// with no signal even on the public library path (the CLI path is
    /// already covered by `Config::validate`, but the library API must not
    /// depend on the caller having validated first).
    pub fn new(
        policy: &'a PermissionsPolicy,
        settings_allow: &'a [String],
        settings_ask: &'a [String],
        settings_deny: &'a [String],
    ) -> Result<Self> {
        for p in &policy.profiles {
            if !PermissionProfile::ALL.contains(&p.as_str()) {
                return Err(Error::PolicyProfileUnknown { name: p.clone() });
            }
        }
        Ok(Self {
            policy,
            settings_allow,
            settings_ask,
            settings_deny,
        })
    }

    pub fn audit(&self) -> Vec<PermissionFinding> {
        let mut findings = Vec::new();
        let deny_set: HashSet<&String> = self.settings_deny.iter().collect();

        for name in &self.policy.profiles {
            if let Some(p) = PermissionProfile::from_str(name) {
                for required in &p.deny {
                    let s = required.to_string();
                    if !deny_set.contains(&s) {
                        findings.push(PermissionFinding {
                            kind: PermissionFindingKind::MissingBaselineDeny,
                            message: format!("profile '{}' requires deny: {required}", p.name),
                            rule: Some(required.to_string()),
                        });
                    }
                }
            }
        }
        for a in self.settings_allow {
            if deny_set.contains(a) {
                findings.push(PermissionFinding {
                    kind: PermissionFindingKind::ContradictoryRule,
                    message: format!("rule '{a}' appears in both allow and deny"),
                    rule: Some(a.clone()),
                });
            }
        }
        for a in self.settings_ask {
            if deny_set.contains(a) {
                findings.push(PermissionFinding {
                    kind: PermissionFindingKind::ContradictoryRule,
                    message: format!("rule '{a}' appears in both ask and deny"),
                    rule: Some(a.clone()),
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_composes_profile_and_extras() {
        let policy = PermissionsPolicy {
            profiles: vec!["baseline".into()],
            extra_allow: vec!["Bash(pnpm gate:*)".into()],
            extra_ask: vec![],
            extra_deny: vec!["Bash(custom-danger *)".into()],
        };
        let block = PermissionGenerator::new(&policy).unwrap().generate();
        assert!(block.allow.contains(&"Bash(pnpm gate:*)".to_string()));
        assert!(block.deny.contains(&"Bash(custom-danger *)".to_string()));
        assert!(block.deny.contains(&"Bash(sudo *)".to_string()));
    }

    #[test]
    fn generator_rejects_unknown_profile() {
        let policy = PermissionsPolicy {
            profiles: vec!["made-up".into()],
            ..Default::default()
        };
        assert!(PermissionGenerator::new(&policy).is_err());
    }

    #[test]
    fn auditor_finds_missing_baseline_deny() {
        let policy = PermissionsPolicy {
            profiles: vec!["baseline".into()],
            ..Default::default()
        };
        let allow = vec!["Bash(ls *)".to_string()];
        let ask: Vec<String> = vec![];
        let deny = vec![]; // intentionally empty
        let findings = PermissionAuditor::new(&policy, &allow, &ask, &deny)
            .unwrap()
            .audit();
        assert!(!findings.is_empty());
        assert!(
            findings
                .iter()
                .any(|f| f.kind == PermissionFindingKind::MissingBaselineDeny)
        );
    }

    #[test]
    fn auditor_detects_allow_deny_contradiction() {
        let policy = PermissionsPolicy::default();
        let allow = vec!["Bash(foo)".to_string()];
        let ask: Vec<String> = vec![];
        let deny = vec!["Bash(foo)".to_string()];
        let findings = PermissionAuditor::new(&policy, &allow, &ask, &deny)
            .unwrap()
            .audit();
        assert!(
            findings
                .iter()
                .any(|f| f.kind == PermissionFindingKind::ContradictoryRule)
        );
    }

    #[test]
    fn auditor_detects_ask_deny_contradiction() {
        let policy = PermissionsPolicy::default();
        let allow: Vec<String> = vec![];
        let ask = vec!["Bash(bar)".to_string()];
        let deny = vec!["Bash(bar)".to_string()];
        let findings = PermissionAuditor::new(&policy, &allow, &ask, &deny)
            .unwrap()
            .audit();
        assert!(
            findings
                .iter()
                .any(|f| f.kind == PermissionFindingKind::ContradictoryRule)
        );
        assert!(findings.iter().any(|f| f.message.contains("ask and deny")));
    }

    #[test]
    fn auditor_rejects_unknown_profile() {
        // Symmetric with generator_rejects_unknown_profile — the public
        // library path must not silently skip an unvalidated profile name.
        let policy = PermissionsPolicy {
            profiles: vec!["basline".into()],
            ..Default::default()
        };
        let empty: Vec<String> = vec![];
        assert!(PermissionAuditor::new(&policy, &empty, &empty, &empty).is_err());
    }
}
