//! # scaffold — the composition manifest a generated harness is built from
//!
//! `templates/scaffold.toml` declares every template-derived artifact a
//! harness contains: the template it comes from, where it lands, which tier it
//! belongs to, and how the project's copy relates to the template it came
//! from. Emissions whose content comes from the project rather than from a
//! template — a `rustfmt.toml` carrying the detected edition, a cloud
//! permission profile composed from CI config — are the skill's, and
//! `SKILL.md` names them where it reads this file. Three consumers read it —
//! the skill that emits a harness, the test that builds a fixture, and the
//! auditor that reports which destinations a project holds — so the
//! composition is stated once and drift between those views is impossible.
//!
//! [`Content`] is what keeps those three views agreeing. How an artifact is
//! emitted, how its presence is tested, and what counts as drift are one
//! decision, so they are one field: machinery held byte-identical, a seed
//! handed to the project outright, a sentinel-partitioned file, or a JSON
//! fragment whose presence is its landing at a key rather than its shared
//! destination existing.
//!
//! [`Tier`] is what makes a partial harness expressible. `Foundation`
//! artifacts carry no language dependency; `Language` artifacts need a
//! detected stack, because a formatter and a toolchain allow list cannot be
//! chosen without one. A repo whose stack has no profile can therefore receive
//! the foundation tier and be told exactly what is absent, rather than
//! receiving nothing. Which artifacts sit in which tier is the manifest's
//! answer, never a list restated in prose.
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
use crate::policy::PermissionProfile;
use crate::wire_enum::wire_enum;

const MANIFEST_FILENAME: &str = "scaffold.toml";

/// The placeholder a language-tier artifact resolves against the detected
/// stack. Foundation artifacts never carry it — enforced at load, so a
/// consumer that has no language can emit the whole tier without a
/// substitution step.
const LANG_PLACEHOLDER: &str = "{lang}";

wire_enum! {
    /// Which half of a harness an artifact belongs to.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum Tier {
        /// Language-agnostic: emitted for every project, including one whose
        /// stack has no profile.
        Foundation => "foundation",
        /// Needs a detected language profile.
        Language => "language",
    }
}

/// How an artifact's project copy relates to the template that emits it.
///
/// One discriminator, because the relationship decides three answers that must
/// agree: how the artifact is emitted, how its presence is tested, and what
/// counts as drift. Independent flags could also spell "merged and managed",
/// which named nothing and had to be rejected at load — an invalid state that
/// this enum cannot represent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Content {
    /// Machinery. The project's copy must stay byte-identical to the template,
    /// because the template is the only statement of what it does.
    Copy,
    /// A starting point the project takes ownership of. harnex writes it once
    /// and asserts nothing about it afterwards — a project's governance is its
    /// own, and holding it to the template would make tailoring read as drift.
    Seed,
    /// Partitioned by `harnex-managed` sentinels: harnex owns what they bound,
    /// the project owns everything outside them.
    Managed,
    /// A JSON fragment contributed into a shared destination at `key`. Several
    /// artifacts may name the same destination, so this is the only kind whose
    /// presence is not answered by the destination existing.
    Merge { key: String },
}

/// Closed schema (Constitution V). `executable` defaults, so a misspelled
/// field is not a parse error but a silently weaker artifact — a hook that
/// lands without its exec bit is wired and unrunnable.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub tier: Tier,
    pub content: Content,
    /// Path under `templates/`, possibly carrying `{lang}`.
    pub template: String,
    /// Project-relative destination, possibly carrying `{lang}`.
    pub destination: String,
    /// Written 0o755 — a hook Claude Code invokes directly.
    #[serde(default)]
    pub executable: bool,
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

    /// Every template this artifact can be emitted from: one per shipped
    /// language when the manifest parameterizes it, otherwise exactly one.
    pub fn resolved_templates(&self) -> Vec<String> {
        self.resolved_pairs().into_iter().map(|(t, _)| t).collect()
    }

    /// Every `(template, destination)` this artifact resolves to, paired by the
    /// language that produced both.
    ///
    /// Pairing is what separates "this file matches some template harnex ships"
    /// from "this file matches the template that emits it". While the formatter
    /// landed at one fixed `hooks/post-format.sh`, the language was not
    /// recoverable from the destination and matching any template was the
    /// honest answer; now that the destination carries it, the union would let
    /// a Rust project hold the Python formatter and call it undrifted.
    pub fn resolved_pairs(&self) -> Vec<(String, PathBuf)> {
        if self.template.contains(LANG_PLACEHOLDER) || self.destination_is_language_parameterized()
        {
            PermissionProfile::languages()
                .filter_map(|lang| {
                    Some((
                        self.template_for(Some(lang))?,
                        self.destination_for(Some(lang))?,
                    ))
                })
                .collect()
        } else {
            self.template_for(None)
                .zip(self.destination_for(None))
                .into_iter()
                .collect()
        }
    }

    /// Every concrete destination this artifact can occupy: one per language
    /// the oracle ships a profile for when the manifest parameterizes it,
    /// otherwise exactly one.
    ///
    /// Enumerating the languages is what makes a stack-free answer exact. A
    /// `*` in `{lang}`'s place reads any name of the same shape as coverage —
    /// a project's own `api-conventions.md` would report the language rule
    /// present with no harnex rule anywhere.
    pub fn resolved_destinations(&self) -> Vec<PathBuf> {
        if self.destination_is_language_parameterized() {
            PermissionProfile::languages()
                .filter_map(|lang| self.destination_for(Some(lang)))
                .collect()
        } else {
            self.destination_for(None).into_iter().collect()
        }
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
    let lang = lang?;
    // `check_contained` ran at load time against the unsubstituted string, so
    // a language carrying path syntax would rewrite the shape it approved —
    // `{lang}/x` with `lang = "../.."` resolves outside the root. Production
    // callers pass a closed set, but both resolvers are public and a
    // guarantee that holds only while every caller behaves is not one.
    let identifier = !lang.is_empty()
        && lang
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    identifier.then(|| value.replace(LANG_PLACEHOLDER, lang))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// Artifacts in the given content kind, in declaration order.
    pub fn with_content(&self, kind: &Content) -> impl Iterator<Item = &Artifact> {
        self.artifacts.iter().filter(move |a| &a.content == kind)
    }

    fn invalid(&self, message: String) -> Error {
        Error::ConfigInvalid {
            message,
            location: Some(Location::file(self.path.clone())),
        }
    }

    /// A manifest path must stay under the root it is joined to.
    ///
    /// Three rejections, each for its own mechanism. Absolute values because
    /// `Path::join` replaces the base with them outright. Tilde-rooted values
    /// because a shell downstream expands them — `Path` itself never does — and
    /// both spellings expand, `~/x` to `$HOME` and `~user/x` to another
    /// account's home, so the test is a leading `~` on a value that goes on to
    /// name a directory. A tilde with no separator is just a filename:
    /// `~notes.md`, or an Office lock file's `~$doc`. `..` by component, so a
    /// filename that merely contains dots stays legal.
    fn check_contained(&self, label: &str, value: &str) -> Result<()> {
        let path = Path::new(value);
        if path.is_absolute() || (value.starts_with('~') && value.contains('/')) {
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
            match &a.content {
                Content::Managed if a.tier == Tier::Language => {
                    return Err(self.invalid(format!(
                        "artifact '{}' is managed on the language tier; a managed pair must \
                         resolve without a detected stack, and one that cannot is silently \
                         never audited",
                        a.template
                    )));
                }
                Content::Merge { key } => {
                    if key.is_empty() || key.split('.').any(|k| k.trim().is_empty()) {
                        return Err(self.invalid(format!(
                            "artifact '{}' declares an empty merge key path '{key}'",
                            a.template
                        )));
                    }
                    if !a.destination.ends_with(".json") {
                        return Err(self.invalid(format!(
                            "artifact '{}' merges into a non-JSON destination '{}'",
                            a.template, a.destination
                        )));
                    }
                }
                Content::Copy | Content::Seed | Content::Managed => {}
            }
        }
        Ok(())
    }
}

/// Whether a [`Content::Merge`] fragment reached `key` in a merged document.
///
/// Containment, not equality — that is what `merge` means. A destination is
/// shared by both tiers and by whatever the project added itself, so demanding
/// equality would report the foundation's contribution missing the moment a
/// language fragment landed beside it. Objects match key-wise; arrays match
/// element-wise and unordered, because the order two fragments land in is a
/// property neither of them declares.
///
/// An absent key path is absent, never an empty match: the destination exists
/// because some other artifact wrote it, which is exactly why its existence
/// cannot answer for this fragment.
pub fn fragment_landed(doc: &serde_json::Value, key: &str, fragment: &serde_json::Value) -> bool {
    key.split('.')
        .try_fold(doc, |node, seg| node.get(seg))
        .is_some_and(|landed| contains(landed, fragment))
}

fn contains(whole: &serde_json::Value, part: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (whole, part) {
        (Value::Object(w), Value::Object(p)) => p
            .iter()
            .all(|(k, v)| w.get(k).is_some_and(|got| contains(got, v))),
        (Value::Array(w), Value::Array(p)) => {
            p.iter().all(|v| w.iter().any(|got| contains(got, v)))
        }
        _ => whole == part,
    }
}

#[cfg(test)]
mod containment_tests {

    use super::fragment_landed;
    use serde_json::json;

    #[test]
    fn a_fragment_is_found_beside_another_contributors_entries() {
        let settings = json!({"hooks": {
            "SessionStart": [{"matcher": "startup"}],
            "Stop": [{}],
            "PostToolUse": [{"matcher": "Edit|Write"}],
            "PreToolUse": [{"matcher": "operator's own"}],
        }});
        let foundation = json!({"SessionStart": [{"matcher": "startup"}], "Stop": [{}]});
        let language = json!({"PostToolUse": [{"matcher": "Edit|Write"}]});
        assert!(fragment_landed(&settings, "hooks", &foundation));
        assert!(fragment_landed(&settings, "hooks", &language));
    }

    #[test]
    fn a_fragment_that_never_merged_is_not_found() {
        let settings = json!({"hooks": {"SessionStart": [{"matcher": "startup"}]}});
        let language = json!({"PostToolUse": [{"matcher": "Edit|Write"}]});
        assert!(!fragment_landed(&settings, "hooks", &language));
    }

    #[test]
    fn an_array_matches_element_wise_and_unordered() {
        let doc = json!({"permissions": {"allow": ["Bash(git commit *)", "Write", "Bash(uv *)"]}});
        assert!(fragment_landed(
            &doc,
            "permissions.allow",
            &json!(["Bash(uv *)", "Write"])
        ));
        assert!(!fragment_landed(
            &doc,
            "permissions.allow",
            &json!(["Bash(poe *)"])
        ));
    }

    #[test]
    fn an_absent_key_path_is_absent_rather_than_empty() {
        let doc = json!({"permissions": {"deny": []}});
        assert!(fragment_landed(&doc, "permissions.deny", &json!([])));
        assert!(!fragment_landed(&doc, "permissions.allow", &json!([])));
        assert!(!fragment_landed(&doc, "hooks.SessionStart", &json!([])));
    }
}

#[cfg(test)]
mod tier_tests {
    use super::Tier;

    #[test]
    fn tier_spells_itself_the_same_way_twice() {
        for variant in Tier::ALL {
            assert_eq!(
                serde_json::to_string(variant).unwrap(),
                format!("{:?}", variant.as_str()),
                "`rename_all` and the wire string are two spellings of one name"
            );
        }
    }

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
    fn a_misspelled_field_is_rejected_rather_than_defaulted() {
        // Every field here defaults, so an unrecognised one is not a loud
        // parse failure but a quietly weaker artifact: `manageed` leaves
        // `managed` false and the artifact drops out of drift enforcement
        // while the manifest still reads as though it were covered.
        let err = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
content = { kind = "copy" }
template = "common/CLAUDE.md"
destination = "CLAUDE.md"
manageed = true
"#,
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::ConfigInvalid { .. }),
            "expected ConfigInvalid, got {err:?}"
        );
    }

    #[test]
    fn a_language_that_is_not_an_identifier_resolves_to_nothing() {
        // `check_contained` approved the unsubstituted string, so a language
        // carrying path syntax would rewrite the shape it approved. Both
        // resolvers are public, so the guarantee cannot rest on the caller.
        let m = manifest_from(
            r#"
[[artifact]]
tier = "language"
content = { kind = "copy" }
template = "{lang}/post-format.sh"
destination = ".claude/rules/{lang}-conventions.md"
"#,
        )
        .unwrap();
        let a = &m.artifacts()[0];
        for hostile in ["../../..", "a/b", "..", "UPPER", "with space", ""] {
            assert_eq!(a.destination_for(Some(hostile)), None, "lang '{hostile}'");
            assert_eq!(a.template_for(Some(hostile)), None, "lang '{hostile}'");
        }
        assert!(a.destination_for(Some("jvm")).is_some());
    }

    #[test]
    fn a_leading_tilde_is_only_rejected_where_a_shell_would_expand_it() {
        // `Path` never expands a tilde, so the rejection is about a shell
        // downstream — and that only expands `~/`. A bare `~` starts ordinary
        // relative filenames.
        assert!(
            manifest_from(
                r#"
[[artifact]]
tier = "foundation"
content = { kind = "copy" }
template = "common/notes.md"
destination = "~notes.md"
"#,
            )
            .is_ok()
        );
        // Both spellings a shell expands: `~/` to this account's home and
        // `~user/` to another's.
        for rooted in ["~/notes.md", "~someone/notes.md"] {
            assert!(
                manifest_from(&format!(
                    r#"
[[artifact]]
tier = "foundation"
content = {{ kind = "copy" }}
template = "common/notes.md"
destination = "{rooted}"
"#,
                ))
                .is_err(),
                "'{rooted}' should be rejected"
            );
        }
    }

    #[test]
    fn resolves_the_language_placeholder_on_both_sides() {
        let m = manifest_from(
            r#"
[[artifact]]
tier = "language"
content = { kind = "copy" }
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

        // One concrete path per shipped language, never a `*` that would also
        // admit a project's own `api-conventions.md`.
        let resolved = a.resolved_destinations();
        assert_eq!(resolved.len(), PermissionProfile::languages().count());
        assert!(resolved.contains(&PathBuf::from(".claude/rules/rust-conventions.md")));
        assert!(resolved.iter().all(|d| !d.to_string_lossy().contains('*')));
    }

    #[test]
    fn foundation_artifacts_resolve_without_a_language() {
        let m = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
content = { kind = "managed" }
template = "common/CLAUDE.md"
destination = "CLAUDE.md"
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
content = { kind = "copy" }
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
                    "[[artifact]]\ntier = \"foundation\"\ncontent = {{ kind = \"copy\" }}\n{other}\n{field} = \"{escape}\"\n"
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
            "[[artifact]]\ntier = \"foundation\"\ncontent = { kind = \"copy\" }\ntemplate = \"common/a..b.md\"\ndestination = \"a..b.md\"\n",
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
content = { kind = "merge", key = "permissions.deny" }
template = "common/CLAUDE.md"
destination = "CLAUDE.md"
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
content = { kind = "managed" }
template = "{lang}/rules/{lang}-conventions.md"
destination = ".claude/rules/{lang}-conventions.md"
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
                "[[artifact]]\ntier = \"foundation\"\ncontent = {{ kind = \"merge\", key = \"{merge}\" }}\ntemplate = \"common/hooks.json\"\ndestination = \".claude/settings.json\"\n"
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("empty merge key path"),
                "merge '{merge}' must be refused: {err}"
            );
        }
    }

    #[test]
    fn an_artifact_declares_exactly_one_content_kind() {
        // Independent flags could spell "merged and managed", which named
        // nothing and was rejected at load. One discriminator makes it
        // unrepresentable, and an unknown kind is refused at the boundary
        // rather than defaulting to the mildest reading.
        let err = manifest_from(
            r#"
[[artifact]]
tier = "foundation"
content = { kind = "mergeed", key = "hooks" }
template = "common/hooks.json"
destination = ".claude/settings.json"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, Error::ConfigInvalid { .. }), "{err:?}");
    }

    #[test]
    fn rejects_an_empty_manifest() {
        let err = manifest_from("").unwrap_err();
        assert!(err.to_string().contains("no artifacts"), "{err}");
    }
}
