//! # scaffold — the composition manifest a generated harness is built from
//!
//! `templates/scaffold.toml` declares every template-derived artifact a
//! harness contains: the template it comes from, where it lands, whether it is
//! copied or merged into a JSON destination, and which tier it belongs to.
//! Emissions whose content comes from the project rather than from a template
//! — a `rustfmt.toml` carrying the detected edition, a cloud permission
//! profile composed from CI config — are the skill's, and `SKILL.md` names
//! them where it reads this file. Three consumers
//! read it — the skill that emits a harness, the test that builds a fixture,
//! and the auditor that reports which destinations a project holds — so the
//! composition is stated once and drift between those views is impossible.
//!
//! [`Tier`] is what makes a partial harness expressible. `Foundation`
//! artifacts carry no language dependency: the permission floor, the
//! foundation rules, the hook wrappers, the secret-scan git hook. `Language`
//! artifacts need a detected stack, because a formatter and a toolchain allow
//! list cannot be chosen without one. A repo whose stack has no profile can
//! therefore receive the foundation tier and be told exactly what is absent,
//! rather than receiving nothing.
//!
//! ## What this module refuses to do
//!
//! - Never write. It parses and validates the manifest; emitting artifacts is
//!   the skill's job, and the binary composing a harness would put generation
//!   in the surface that exists to verify it.
//! - Never carry a language list. The set of supported languages is
//!   `PermissionProfile::ALL`'s `<lang>-dev` members; a second list here would
//!   be the duplication this manifest exists to remove.
//! - Never accept a path that escapes the root it is joined to — on either
//!   field, by the same check. A manifest is data a plugin ships, and
//!   `Path::join` discards its base when the joined component is absolute, so
//!   an unchecked `template` reads any file on the machine exactly as an
//!   unchecked `destination` writes one. Checking the two separately is how
//!   they drift apart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::envelope::Location;
use crate::error::{Error, Result};

const MANIFEST_FILENAME: &str = "scaffold.toml";

/// The placeholder a language-tier artifact resolves against the detected
/// stack. Foundation artifacts never carry it — enforced at load, so a
/// consumer that has no language can emit the whole tier without a
/// substitution step.
const LANG_PLACEHOLDER: &str = "{lang}";

/// Which half of a harness an artifact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// Language-agnostic: emitted for every project, including one whose
    /// stack has no profile.
    Foundation,
    /// Needs a detected language profile.
    Language,
}

impl Tier {
    pub const ALL: &'static [Self] = &[Self::Foundation, Self::Language];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "foundation" => Self::Foundation,
            "language" => Self::Language,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Foundation => "foundation",
            Self::Language => "language",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Artifact {
    pub tier: Tier,
    /// Path under `templates/`, possibly carrying `{lang}`.
    pub template: String,
    /// Project-relative destination, possibly carrying `{lang}`.
    pub destination: String,
    /// JSON key path this fragment contributes to. Absent means the template
    /// is copied to `destination` verbatim.
    #[serde(default)]
    pub merge: Option<String>,
    /// Written 0o755 — a hook Claude Code invokes directly.
    #[serde(default)]
    pub executable: bool,
    /// Carries `harnex-managed` sentinels, so the managed-region auditor
    /// holds the project's copy to this template.
    #[serde(default)]
    pub managed: bool,
}

impl Artifact {
    /// The template path with `{lang}` resolved. Returns `None` when the
    /// artifact needs a language and none was supplied.
    pub fn template_for(&self, lang: Option<&str>) -> Option<String> {
        substitute(&self.template, lang)
    }

    /// The destination path with `{lang}` resolved, under `project_root`.
    pub fn destination_for(&self, lang: Option<&str>) -> Option<PathBuf> {
        substitute(&self.destination, lang).map(PathBuf::from)
    }

    /// A glob that matches this artifact's destination whatever the language
    /// — the form a consumer with no detected stack can still resolve.
    pub fn destination_glob(&self) -> String {
        self.destination.replace(LANG_PLACEHOLDER, "*")
    }

    /// True when the destination cannot be named without a language.
    pub fn destination_is_language_parameterized(&self) -> bool {
        self.destination.contains(LANG_PLACEHOLDER)
    }
}

fn substitute(value: &str, lang: Option<&str>) -> Option<String> {
    if !value.contains(LANG_PLACEHOLDER) {
        return Some(value.to_string());
    }
    lang.map(|l| value.replace(LANG_PLACEHOLDER, l))
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    #[serde(default)]
    artifact: Vec<Artifact>,
}

/// Parsed `templates/scaffold.toml`.
#[derive(Debug, Clone)]
pub struct ScaffoldManifest {
    artifacts: Vec<Artifact>,
    path: PathBuf,
}

impl ScaffoldManifest {
    /// Load from a plugin's `templates/` directory.
    pub fn load(templates_root: &Path) -> Result<Self> {
        let path = templates_root.join(MANIFEST_FILENAME);
        let raw = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: e,
        })?;
        let parsed: ManifestFile = toml::from_str(&raw).map_err(|e| Error::ConfigInvalid {
            message: format!("{MANIFEST_FILENAME} parse failure: {e}"),
            location: Some(Location::file(path.clone())),
        })?;
        let manifest = Self {
            artifacts: parsed.artifact,
            path,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Artifacts in the given tier, in declaration order.
    pub fn tier(&self, tier: Tier) -> impl Iterator<Item = &Artifact> {
        self.artifacts.iter().filter(move |a| a.tier == tier)
    }

    /// Artifacts whose project copy the managed-region auditor compares
    /// against its template.
    pub fn managed(&self) -> impl Iterator<Item = &Artifact> {
        self.artifacts.iter().filter(|a| a.managed)
    }

    fn invalid(&self, message: String) -> Error {
        Error::ConfigInvalid {
            message,
            location: Some(Location::file(self.path.clone())),
        }
    }

    /// A manifest path must stay under the root it is joined to. Absolute and
    /// `~`-rooted values are rejected because `Path::join` replaces the base
    /// with them outright; `..` is rejected by component so a filename that
    /// merely contains dots stays legal.
    fn check_contained(&self, label: &str, value: &str) -> Result<()> {
        let path = Path::new(value);
        if path.is_absolute() || value.starts_with('~') {
            return Err(self.invalid(format!("{label} '{value}' is not relative")));
        }
        if path.components().any(|c| c.as_os_str() == "..") {
            return Err(self.invalid(format!("{label} '{value}' escapes its root with '..'")));
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.artifacts.is_empty() {
            return Err(self.invalid(format!("{MANIFEST_FILENAME} declares no artifacts")));
        }
        for a in &self.artifacts {
            // Both fields are joined under a root and then read or written, so
            // both need the same containment. Checking them separately is what
            // let the two drift apart: `Path::join` discards its base when the
            // joined component is absolute, so an unchecked field reaches any
            // path on the machine.
            self.check_contained("destination", &a.destination)?;
            self.check_contained("template", &a.template)?;
            if a.tier == Tier::Foundation
                && (a.template.contains(LANG_PLACEHOLDER)
                    || a.destination.contains(LANG_PLACEHOLDER))
            {
                return Err(self.invalid(format!(
                    "foundation artifact '{}' carries {LANG_PLACEHOLDER}; a tier emitted without a \
                     detected language cannot resolve it",
                    a.template
                )));
            }
            if a.managed && a.tier == Tier::Language {
                return Err(self.invalid(format!(
                    "artifact '{}' is managed on the language tier; a managed pair must resolve \
                     without a detected stack, and one that cannot is silently never audited",
                    a.template
                )));
            }
            if let Some(merge) = &a.merge
                && (merge.is_empty() || merge.split('.').any(|k| k.trim().is_empty()))
            {
                return Err(self.invalid(format!(
                    "artifact '{}' declares an empty `merge` key path '{merge}'",
                    a.template
                )));
            }
            if a.merge.is_some() && !a.destination.ends_with(".json") {
                return Err(self.invalid(format!(
                    "artifact '{}' declares `merge` into a non-JSON destination '{}'",
                    a.template, a.destination
                )));
            }
            if a.managed && a.merge.is_some() {
                return Err(self.invalid(format!(
                    "artifact '{}' is both merged and managed; sentinel regions need a whole file",
                    a.template
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tier_tests {
    use super::Tier;

    #[test]
    fn from_str_round_trips_every_variant() {
        for t in Tier::ALL {
            assert_eq!(Tier::from_str(t.as_str()), Some(*t));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert!(Tier::from_str("optional").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest_from(body: &str) -> Result<ScaffoldManifest> {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), body).unwrap();
        ScaffoldManifest::load(tmp.path())
    }

    #[test]
    fn resolves_the_language_placeholder_on_both_sides() {
        let m = manifest_from(
            r#"
[[artifact]]
tier = "language"
template = "{lang}/rules/{lang}-conventions.md"
destination = ".claude/rules/{lang}-conventions.md"
"#,
        )
        .unwrap();
        let a = &m.artifacts()[0];
        assert_eq!(
            a.template_for(Some("rust")).unwrap(),
            "rust/rules/rust-conventions.md"
        );
        assert_eq!(
            a.destination_for(Some("rust")).unwrap(),
            PathBuf::from(".claude/rules/rust-conventions.md")
        );
        assert!(a.template_for(None).is_none());
        assert_eq!(a.destination_glob(), ".claude/rules/*-conventions.md");
    }

    #[test]
    fn foundation_artifacts_resolve_without_a_language() {
        let m = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
template = "common/CLAUDE.md"
destination = "CLAUDE.md"
managed = true
"#,
        )
        .unwrap();
        let a = &m.artifacts()[0];
        assert_eq!(a.template_for(None).unwrap(), "common/CLAUDE.md");
        assert_eq!(a.destination_for(None).unwrap(), PathBuf::from("CLAUDE.md"));
    }

    #[test]
    fn rejects_a_foundation_artifact_that_needs_a_language() {
        let err = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
template = "{lang}/post-format.sh"
destination = "hooks/post-format.sh"
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("{lang}"), "{err}");
    }

    #[test]
    fn rejects_a_path_that_escapes_its_root_on_either_field() {
        // `Path::join` discards its base when the joined component is
        // absolute, so an unchecked field on either side reaches any path on
        // the machine — a manifest is data a plugin ships.
        for escape in ["/etc/passwd", "../outside.md", "~/.ssh/config"] {
            for (field, other) in [
                ("destination", "template = \"common/CLAUDE.md\""),
                ("template", "destination = \"CLAUDE.md\""),
            ] {
                let err = manifest_from(&format!(
                    "[[artifact]]\ntier = \"foundation\"\n{other}\n{field} = \"{escape}\"\n"
                ))
                .unwrap_err();
                assert!(
                    err.to_string().contains(escape) && err.to_string().contains(field),
                    "{field} '{escape}' must be refused: {err}"
                );
            }
        }
    }

    #[test]
    fn accepts_a_filename_that_merely_contains_dots() {
        // `..` is rejected by path component, so a legal name is not caught by
        // a substring test.
        let m = manifest_from(
            "[[artifact]]\ntier = \"foundation\"\ntemplate = \"common/a..b.md\"\ndestination = \"a..b.md\"\n",
        )
        .unwrap();
        assert_eq!(m.artifacts().len(), 1);
    }

    #[test]
    fn rejects_merge_into_a_non_json_destination() {
        let err = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
template = "common/CLAUDE.md"
destination = "CLAUDE.md"
merge = "permissions.deny"
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-JSON"), "{err}");
    }

    #[test]
    fn rejects_a_managed_artifact_on_the_language_tier() {
        // The managed-region auditor resolves managed pairs without a stack.
        // One that needs a language resolves to `None` and is skipped — the
        // artifact would be silently never audited.
        let err = manifest_from(
            r#"
[[artifact]]
tier = "language"
template = "{lang}/rules/{lang}-conventions.md"
destination = ".claude/rules/{lang}-conventions.md"
managed = true
"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("managed on the language tier"),
            "{err}"
        );
    }

    #[test]
    fn rejects_an_empty_merge_key_path() {
        for merge in ["", ".", "a..b", " "] {
            let err = manifest_from(&format!(
                "[[artifact]]\ntier = \"foundation\"\ntemplate = \"common/hooks.json\"\ndestination = \".claude/settings.json\"\nmerge = \"{merge}\"\n"
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("empty `merge` key path"),
                "merge '{merge}' must be refused: {err}"
            );
        }
    }

    #[test]
    fn rejects_an_artifact_that_is_both_merged_and_managed() {
        let err = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
template = "common/hooks.json"
destination = ".claude/settings.json"
merge = "hooks"
managed = true
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("merged and managed"), "{err}");
    }

    #[test]
    fn rejects_an_empty_manifest() {
        let err = manifest_from("").unwrap_err();
        assert!(err.to_string().contains("no artifacts"), "{err}");
    }
}
