//! Drift guard: the harnex plugin ships permission templates as committed
//! JSON so it never needs the `harnex` binary at scaffold time, but those
//! files are a *projection* of the built-in profiles — `profiles.rs` is the
//! single source of truth. This test fails if a template diverges from the
//! profile it is generated from.
//!
//! Regenerate a drifted template with:
//!   harnex policy permissions generate   # with [policy.permissions] profiles=["<profile>"]
//! then copy the `deny` (baseline) or `allow` (dev profile) array into the
//! matching template file.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::policy::PermissionProfile;

fn template(rel: &str) -> BTreeSet<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates")
        .join(rel);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read template {}: {e}", path.display()));
    let rules: Vec<String> = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse template {}: {e}", path.display()));
    rules.into_iter().collect()
}

fn profile_set(name: &str, field: fn(&PermissionProfile) -> &[&'static str]) -> BTreeSet<String> {
    let p = PermissionProfile::from_str(name).expect("profile must exist");
    field(&p).iter().map(|s| s.to_string()).collect()
}

/// `(profile name, template path)` for every `<lang>-dev` profile, derived
/// from the registry. A language added to `PermissionProfile::ALL` joins
/// every check below without an edit here — the pairing is what a
/// hand-listed set silently omits.
fn dev_profile_templates() -> Vec<(&'static str, String)> {
    PermissionProfile::ALL
        .iter()
        .filter_map(|name| {
            name.strip_suffix("-dev")
                .map(|lang| (*name, format!("{lang}/permissions.allow.json")))
        })
        .collect()
}

/// The two language-agnostic templates and the profiles they project. Both
/// land on the foundation tier, so a stack with no language profile still
/// receives a deny floor and an allow floor.
const FOUNDATION_TEMPLATES: &[(&str, &str)] = &[
    ("common/permissions.deny.json", "baseline"),
    ("common/permissions.allow.json", "workspace"),
];

#[test]
fn foundation_templates_match_their_profiles() {
    for (rel, profile) in FOUNDATION_TEMPLATES {
        let p = PermissionProfile::from_str(profile).expect("profile must exist");
        let rules = if rel.contains("deny") {
            &p.deny
        } else {
            &p.allow
        };
        assert_eq!(
            template(rel),
            rules.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
            "{rel} drifted from the `{profile}` profile"
        );
    }
}

#[test]
fn the_foundation_and_language_allow_sets_are_disjoint() {
    // Both tiers merge into `permissions.allow`, and the union is a sorted
    // set — a rule listed on both sides would collapse silently and leave two
    // owners for one grant.
    let floor = template("common/permissions.allow.json");
    for (_, rel) in dev_profile_templates() {
        let lang = template(&rel);
        let overlap: Vec<&String> = lang.intersection(&floor).collect();
        assert!(
            overlap.is_empty(),
            "{rel} repeats workspace floor rules {overlap:?}; the floor is foundation-tier"
        );
    }
}

#[test]
fn lang_allow_templates_match_dev_profiles() {
    for (profile, rel) in dev_profile_templates() {
        assert_eq!(
            template(&rel),
            profile_set(profile, |p| &p.allow),
            "{rel} drifted from the `{profile}` profile allow set"
        );
    }
}

#[test]
fn no_profile_or_template_carries_duplicate_rules() {
    // Set comparison alone would hide a rule listed twice on one side. Assert
    // exact-mirror integrity: neither profiles nor templates carry duplicates.
    fn assert_unique(label: &str, rules: &[String]) {
        let unique: BTreeSet<&String> = rules.iter().collect();
        assert_eq!(
            unique.len(),
            rules.len(),
            "{label} contains duplicate rules"
        );
    }
    for name in PermissionProfile::ALL {
        let p = PermissionProfile::from_str(name).unwrap();
        let deny: Vec<String> = p.deny.iter().map(|s| s.to_string()).collect();
        let allow: Vec<String> = p.allow.iter().map(|s| s.to_string()).collect();
        assert_unique(&format!("profile '{name}' deny"), &deny);
        assert_unique(&format!("profile '{name}' allow"), &allow);
    }
    let mut templates: Vec<String> = FOUNDATION_TEMPLATES
        .iter()
        .map(|(rel, _)| (*rel).to_string())
        .collect();
    templates.extend(dev_profile_templates().into_iter().map(|(_, rel)| rel));
    for rel in templates {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/harnex/templates")
            .join(&rel);
        let raw = std::fs::read_to_string(&path).unwrap();
        let rules: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert_unique(&format!("template {rel}"), &rules);
    }
}

#[test]
fn all_profile_bash_rules_use_canonical_space_wildcard() {
    // Lock the space-then-`*` spelling across every profile. The colon form
    // `cmd:*` is an equivalent trailing wildcard, so allowing both spellings
    // invites semantic duplicates (`destroy *` + `destroy:*`) that an exact
    // set comparison cannot see. Forbidding `:*)` at the source removes the
    // class without the false positives a normalized-base dedup would hit
    // (e.g. `rm -rf /` vs `rm -rf /*`).
    for name in PermissionProfile::ALL {
        let p = PermissionProfile::from_str(name).unwrap();
        for rule in p.deny.iter().chain(&p.allow).chain(&p.ask) {
            assert!(
                !rule.contains(":*)"),
                "profile '{name}' rule '{rule}' uses colon-style wildcard; use ` *`"
            );
        }
    }
}

#[test]
fn every_dev_profile_has_a_committed_allow_template() {
    // Guards the reverse gap: a new `*-dev` profile must ship a template so
    // the plugin can scaffold it without the binary.
    for (name, rel) in dev_profile_templates() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/harnex/templates")
            .join(&rel);
        assert!(
            path.exists(),
            "dev profile '{name}' has no plugin template at {rel}"
        );
    }
}
