//! Meta-test: the artifact a `harnex scaffold` run would produce from the
//! committed templates must itself pass every validator the oracle ships.
//!
//! This is the structural lock for Constitution IX as applied to the plugin:
//! `policy_template_sync.rs` guarantees the permission JSON templates mirror
//! `profiles.rs`, but the templates *as a whole* (hooks, settings shape, rule
//! frontmatter, sentinel-block presence) need their own drift guard. This test
//! materialises a project from the templates exactly as the skill would compose
//! it, then runs `SettingsValidator`, `RuleValidator`, `SkillValidator`, and
//! the `audit` settings-drift check on the result. Any drift between template
//! and oracle vocabulary fails the build.

use std::fs;
use std::path::{Path, PathBuf};

use harness_core::audit::{AuditCheckKind, ProjectAuditor};
use harness_core::config::SkillsPolicy;
use harness_core::envelope::Finding;
use harness_core::scaffold::{Content, ScaffoldManifest, Tier};
use harness_core::validate::{RuleValidator, SettingsScope, SettingsValidator, SkillValidator};
use tempfile::TempDir;

fn plugin_templates() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates")
}

fn copy_file(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkdir {parent:?}: {e}"));
    }
    fs::copy(src, dst).unwrap_or_else(|e| panic!("copy {src:?} -> {dst:?}: {e}"));
    resolve_fill_markers(dst);
}

/// Stand in for the step the skill performs from its project analysis.
///
/// The fixture asserts a *delivered* harness is clean, not that raw templates
/// are. Leaving the markers would make every scaffold report the fill-marker
/// findings that exist to catch a skipped analysis — and filling them here is
/// what proves the auditor stays silent once the step has run.
fn resolve_fill_markers(path: &Path) {
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        return;
    }
    let Ok(body) = fs::read_to_string(path) else {
        return;
    };
    let mut out = String::with_capacity(body.len());
    let mut rest = body.as_str();
    while let Some(start) = rest.find("<!-- harnex-fill:") {
        let Some(end) = rest[start..].find("-->") else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str("observed from the project");
        rest = &rest[start + end + "-->".len()..];
    }
    out.push_str(rest);
    fs::write(path, out).unwrap_or_else(|e| panic!("fill {path:?}: {e}"));
}

/// `bash -n <path>` — syntax-check a generated shell script without running it.
/// On non-unix (no bash), assume OK; the unix CI lane is the gate.
fn bash_n_ok(path: &Path) -> bool {
    match std::process::Command::new("bash")
        .arg("-n")
        .arg(path)
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => true,
    }
}

/// Materialise a project from `templates/scaffold.toml` exactly as the skill
/// would: copy each artifact to its declared destination, merge each JSON
/// fragment into the key path it declares, and set the executable bit where
/// the manifest says the artifact is a script.
///
/// The manifest is the only statement of the composition, so this fixture
/// cannot drift from what a real scaffold emits. Building the file list by
/// hand here is what previously let the fixture omit every hook while the
/// test still reported a clean scaffold.
fn emit_tier(
    manifest: &ScaffoldManifest,
    tier: Tier,
    templates: &Path,
    lang: Option<&str>,
    proj_root: &Path,
    settings: &mut serde_json::Value,
) {
    for artifact in manifest.tier(tier) {
        let (Some(template), Some(destination)) =
            (artifact.template_for(lang), artifact.destination_for(lang))
        else {
            panic!(
                "artifact {} needs a language the fixture did not supply",
                artifact.template
            );
        };
        let src = templates.join(&template);
        let dst = proj_root.join(&destination);
        match &artifact.content {
            Content::Merge { key } => {
                let fragment: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&src).unwrap())
                        .unwrap_or_else(|e| panic!("parse {src:?}: {e}"));
                merge_at(settings, key, fragment);
            }
            Content::Copy | Content::Seed | Content::Managed => {
                copy_file(&src, &dst);
                if artifact.executable {
                    set_executable(&dst);
                }
            }
        }
    }
}

/// Set `value` at a dotted key path, unioning object keys when a fragment
/// already occupies it — two artifacts contribute hook events to the same
/// `hooks` object, and neither may erase the other.
fn merge_at(root: &mut serde_json::Value, key_path: &str, value: serde_json::Value) {
    let mut cursor = root;
    let keys: Vec<&str> = key_path.split('.').collect();
    for key in &keys[..keys.len() - 1] {
        cursor = cursor
            .as_object_mut()
            .unwrap()
            .entry((*key).to_string())
            .or_insert_with(|| serde_json::json!({}));
    }
    let last = keys[keys.len() - 1].to_string();
    let slot = cursor
        .as_object_mut()
        .unwrap()
        .entry(last)
        .or_insert(serde_json::Value::Null);
    match (slot.as_object_mut(), value) {
        (Some(existing), serde_json::Value::Object(incoming)) => {
            for (k, v) in incoming {
                existing.insert(k, v);
            }
        }
        (_, incoming) => *slot = incoming,
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

/// Scaffold a project from the templates as the skill would, then assert every
/// oracle check passes cleanly on the result.
fn run_scaffold_validation(lang: &str) {
    let templates = plugin_templates();
    let manifest = ScaffoldManifest::load(&templates).unwrap();
    let project = TempDir::new().unwrap();
    let proj_root = project.path();

    let mut settings = serde_json::json!({});
    emit_tier(
        &manifest,
        Tier::Foundation,
        &templates,
        None,
        proj_root,
        &mut settings,
    );
    emit_tier(
        &manifest,
        Tier::Language,
        &templates,
        Some(lang),
        proj_root,
        &mut settings,
    );

    let settings_path = proj_root.join(".claude/settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    // Two artifacts merge into `hooks`, one per tier. The contribution is a
    // key union: a replacement would erase the foundation's events the moment
    // the language fragment landed, and the resulting harness would validate
    // clean while silently running no formatter or no Stop hook.
    let events: Vec<&str> = settings["hooks"]
        .as_object()
        .expect("hooks is an object")
        .keys()
        .map(String::as_str)
        .collect();
    for event in ["SessionStart", "PostToolUse", "Stop"] {
        assert!(
            events.contains(&event),
            "[{lang}] merging the language hook fragment dropped '{event}': {events:?}"
        );
    }

    for script in glob_under(&proj_root.join("hooks"), "*") {
        assert!(
            bash_n_ok(&script),
            "[{lang}] generated {} fails `bash -n`",
            script.display()
        );
    }

    // --- Settings validation (project scope: the scaffolded file is the
    //     committed `.claude/settings.json`) ---
    let settings_findings = SettingsValidator::new()
        .validate_file(&settings_path, SettingsScope::Project)
        .unwrap();
    assert_no_findings(lang, "validate.settings", &settings_findings);

    // --- Audit (spec drift + managed-region drift): on a fresh scaffold the
    //     auditor must produce zero findings. This exercises the same flow a
    //     CI consumer hits through `harness audit --plugin-root <p>`. ---
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let working_dir = proj_root.to_path_buf();
    let audit_outcome = ProjectAuditor::new(&working_dir)
        .with_plugin_root(plugin_root)
        .run()
        .unwrap();
    assert_no_findings(lang, "audit", &audit_outcome.findings);
    assert_scaffold_is_operable(lang, proj_root);
    // Every audit kind must have actually run — a silent skip means the
    // meta-test checks nothing. Sourced from the enum SSoT so adding a
    // variant forces this assertion to cover it.
    for kind in AuditCheckKind::ALL {
        let s = kind.as_str().to_string();
        assert!(
            audit_outcome.run.contains(&s),
            "[{lang}] audit kind '{}' must run; skipped: {:?}",
            kind.as_str(),
            audit_outcome.skipped
        );
    }

    // --- Rule validation, under the policy the scaffold itself ships ---
    // Restating the policy here would make this fixture pass against a
    // configuration no scaffolded project has: an always-loaded rule the
    // manifest emits and `harness.toml` never declares would be a finding in
    // every real project and green in this test.
    let scaffolded = harness_core::config::Config::load_from(&proj_root.join("harness.toml"))
        .unwrap_or_else(|e| panic!("[{lang}] scaffolded harness.toml: {e}"));
    let rule_policy = scaffolded
        .validate
        .as_ref()
        .and_then(|v| v.rules.clone())
        .unwrap_or_else(|| panic!("[{lang}] the scaffold's harness.toml declares no rule policy"));
    let rv = RuleValidator::new(&rule_policy);
    for rule_path in glob_under(&proj_root.join(".claude/rules"), "*.md") {
        let findings = rv.validate_file(&rule_path).unwrap();
        assert_no_findings(
            lang,
            &format!("validate.rules({})", rule_path.display()),
            &findings,
        );
    }

    // --- Skill validation: the harnex SKILL.md itself must validate ---
    // The skill ships with the plugin; we copy it into the project tree's
    // canonical location (mirroring how an installed plugin's skill would be
    // discovered) and run SkillValidator. This exercises the full closed-set
    // surface against the plugin's own contract.
    let plugin_skill =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/SKILL.md");
    if plugin_skill.exists() {
        let dst = proj_root.join(".claude/skills/harnex/SKILL.md");
        copy_file(&plugin_skill, &dst);
        let skill_policy = SkillsPolicy {
            max_skill_md_lines: 500,
            max_description_chars: 1536,
            reject_unknown_keys: true,
            flag_side_effect_verbs: false,
        };
        let sv = SkillValidator::new(&skill_policy);
        let findings = sv.validate_file(&dst).unwrap();
        assert_no_findings(lang, "validate.skills(harnex SKILL.md)", &findings);
    }

    // --- The `extend skill` scaffold template must itself validate clean ---
    // The skeleton `extend skill <name>` writes (before the operator fills the
    // body) must pass SkillValidator as-is, so a freshly scaffolded skill is
    // spec-correct from the first commit.
    let skill_template = plugin_templates().join("common/skill-template.md");
    let dst = proj_root.join(".claude/skills/example-skill/SKILL.md");
    copy_file(&skill_template, &dst);
    let skill_policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 1536,
        reject_unknown_keys: true,
        flag_side_effect_verbs: true,
    };
    let findings = SkillValidator::new(&skill_policy)
        .validate_file(&dst)
        .unwrap();
    assert_no_findings(lang, "validate.skills(skill-template.md)", &findings);
}

fn glob_under(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let glob_pattern = dir.join(pattern);
    let s = glob_pattern.to_string_lossy().to_string();
    glob::glob(&s)
        .map(|iter| iter.filter_map(std::result::Result::ok).collect())
        .unwrap_or_default()
}

/// Strict: a fresh scaffold MUST produce zero findings at every severity.
/// Any advisory (Minor / Info) is a template / oracle mismatch this meta-test
/// is built to catch. If a finding is intentional, encode it as an explicit
/// allowlist constant — silent severity downgrades defeat the test's purpose.
/// A scaffold must be runnable, not merely well-formed.
///
/// Zero findings says the harness is spec-correct; it says nothing about
/// whether the operator can use it. The generated `governance.md` sends its
/// reader to `harness lifecycle observe|candidates|retire` and
/// `harness telemetry report`, and every one of those answered CONFIG_NOT_FOUND
/// on a fresh scaffold — the manifest declared no `harness.toml` at all, so the
/// loop those rules describe was documented and inoperable. Auditing the
/// artifacts could never catch that, because the missing artifact was the one
/// that made the rest reachable.
fn assert_scaffold_is_operable(lang: &str, proj_root: &Path) {
    // The exact file, not an upward walk: a temp directory's ancestors are
    // not this project, and finding someone else's config would pass the
    // assertion for a scaffold that carries none.
    let config = harness_core::config::Config::load_from(&proj_root.join("harness.toml"))
        .unwrap_or_else(|e| {
            panic!("[{lang}] a scaffolded project must carry a loadable harness.toml: {e}")
        });
    for (surface, present) in [
        ("validate", config.validate.is_some()),
        ("lifecycle", config.lifecycle.is_some()),
        ("telemetry", config.telemetry.is_some()),
    ] {
        assert!(
            present,
            "[{lang}] the scaffold's harness.toml declares no [{surface}], so the oracle \
             surface the foundation rules send the operator to is unreachable"
        );
    }
}

fn assert_no_findings(lang: &str, ctx: &str, findings: &[Finding]) {
    assert!(
        findings.is_empty(),
        "[{lang}] {ctx} produced findings on a fresh scaffold: {findings:?}"
    );
}

/// A stack with no language profile still receives the foundation tier, and
/// that partial harness must be coherent on its own: the permission floor
/// applies, the foundation rules load, and every hook the foundation wires
/// points at a script the foundation emits. This is what makes an unsupported
/// stack a smaller harness rather than no harness.
#[test]
fn foundation_only_scaffold_is_coherent_without_a_language() {
    let templates = plugin_templates();
    let manifest = ScaffoldManifest::load(&templates).unwrap();
    let project = TempDir::new().unwrap();
    let proj_root = project.path();

    let mut settings = serde_json::json!({});
    emit_tier(
        &manifest,
        Tier::Foundation,
        &templates,
        None,
        proj_root,
        &mut settings,
    );
    let settings_path = proj_root.join(".claude/settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let findings = SettingsValidator::new()
        .validate_file(&settings_path, SettingsScope::Project)
        .unwrap();
    assert_no_findings("foundation", "validate.settings", &findings);

    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let working_dir = proj_root.to_path_buf();
    let outcome = ProjectAuditor::new(&working_dir)
        .with_plugin_root(plugin_root)
        .run()
        .unwrap();
    assert_no_findings("foundation", "audit", &outcome.findings);
    // A stack with no profile still gets a runnable harness: `harness.toml`
    // is foundation-tier precisely so the loop does not depend on a language.
    assert_scaffold_is_operable("foundation", proj_root);

    // Coverage reports the language tier as absent — the fact the audit skill
    // turns into "this stack has no profile", never a defect the binary claims.
    let absent: Vec<&str> = outcome
        .coverage
        .iter()
        .filter(|c| !c.present)
        .map(|c| c.destination.as_str())
        .collect();
    assert!(
        absent.contains(&"hooks/post-format.sh"),
        "the formatter hook must report absent in a foundation-only scaffold: {absent:?}"
    );
    assert!(
        outcome
            .coverage
            .iter()
            .filter(|c| c.tier == "foundation")
            .all(|c| c.present),
        "every foundation artifact must report present: {:?}",
        outcome.coverage
    );
}

#[test]
fn typescript_scaffold_passes_all_validators() {
    run_scaffold_validation("typescript");
}

#[test]
fn python_scaffold_passes_all_validators() {
    run_scaffold_validation("python");
}

#[test]
fn rust_scaffold_passes_all_validators() {
    run_scaffold_validation("rust");
}

#[test]
fn jvm_scaffold_passes_all_validators() {
    run_scaffold_validation("jvm");
}
