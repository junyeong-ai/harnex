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
use harness_core::config::{RulesPolicy, SkillsPolicy};
use harness_core::envelope::Finding;
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
}

/// `bash -n <path>` — syntax-check a generated shell script without running it.
/// On non-unix (no bash), assume OK; the unix CI lane is the gate.
fn bash_n_ok(path: &Path) -> bool {
    match std::process::Command::new("bash").arg("-n").arg(path).status() {
        Ok(status) => status.success(),
        Err(_) => true,
    }
}

/// Assemble `.claude/settings.json` from the committed JSON projection files
/// — exactly the composition `harnex scaffold` performs. Returns the absolute
/// path of the assembled file inside `project_root`.
fn assemble_settings_json(templates: &Path, lang: &str, project_root: &Path) -> PathBuf {
    let deny: Vec<String> = serde_json::from_str(
        &fs::read_to_string(templates.join("common/permissions.deny.json")).unwrap(),
    )
    .unwrap();
    let allow: Vec<String> = serde_json::from_str(
        &fs::read_to_string(templates.join(format!("{lang}/permissions.allow.json"))).unwrap(),
    )
    .unwrap();
    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(templates.join("common/hooks.json")).unwrap())
            .unwrap();
    let settings = serde_json::json!({
        "permissions": { "allow": allow, "deny": deny },
        "hooks": hooks,
    });
    let path = project_root.join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    path
}

/// Scaffold a project from the templates as the skill would, then assert every
/// oracle check passes cleanly on the result.
fn run_scaffold_validation(lang: &str) {
    let templates = plugin_templates();
    let project = TempDir::new().unwrap();
    let proj_root = project.path();

    // settings.json (assembled from JSON projections)
    let settings_path = assemble_settings_json(&templates, lang, proj_root);

    // Foundation rules + CLAUDE.md (markdown templates, dropped verbatim)
    for rule in ["constitution.md", "rules/governance.md", "rules/artifact-lifecycle.md"] {
        let src = templates.join("common").join(rule);
        let dst = proj_root
            .join(".claude/rules")
            .join(PathBuf::from(rule).file_name().unwrap());
        copy_file(&src, &dst);
    }
    copy_file(
        &templates.join("common/CLAUDE.md"),
        &proj_root.join("CLAUDE.md"),
    );

    // git pre-commit hook (the enforced half of "secrets never reach git")
    let pre_commit = proj_root.join("hooks/pre-commit");
    copy_file(&templates.join("common/git-hooks/pre-commit"), &pre_commit);
    assert!(
        bash_n_ok(&pre_commit),
        "[{lang}] generated hooks/pre-commit fails `bash -n`"
    );

    // Optional path-scoped convention rule for the language
    let lang_rule = templates.join(format!("{lang}/rules/{lang}-conventions.md"));
    if lang_rule.exists() {
        copy_file(
            &lang_rule,
            &proj_root.join(format!(".claude/rules/{lang}-conventions.md")),
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
    assert_no_findings(
        lang,
        "audit (settings-drift + managed-region)",
        &audit_outcome.findings,
    );
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

    // --- Rule validation (constitution + optional conventions rule) ---
    let rule_policy = RulesPolicy {
        max_lines: 200,
        always_loaded_slugs: vec!["constitution".into()],
    };
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
fn assert_no_findings(lang: &str, ctx: &str, findings: &[Finding]) {
    assert!(
        findings.is_empty(),
        "[{lang}] {ctx} produced findings on a fresh scaffold: {findings:?}"
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
