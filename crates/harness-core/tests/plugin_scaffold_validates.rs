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
use harness_core::config::{AgentsPolicy, SkillsPolicy};
use harness_core::envelope::Finding;
use harness_core::scaffold::{Content, ScaffoldManifest, Tier};
use harness_core::validate::{
    AgentValidator, RuleValidator, SettingsScope, SettingsValidator, SkillValidator,
};
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
                assert!(
                    merge_at(settings, key, fragment),
                    "the incumbent's shape at '{key}' blocked {}; the skill reports that \
                     rather than replacing what the project wrote",
                    artifact.template
                );
            }
            // An incumbent is the project's file, and every non-merge kind
            // keeps it. `managed` included: its partition governs a file
            // harnex created and will regenerate, and is not standing to
            // append to one it did not. Contributing a region to an existing
            // file would be right for a `CLAUDE.md` that is three lines of
            // notes and wrong for a `constitution.md` the project already
            // wrote, and a rule that needs the file's meaning to decide is
            // not a rule.
            Content::Copy | Content::Seed | Content::Managed if dst.exists() => {}
            Content::Copy | Content::Seed | Content::Managed => {
                copy_file(&src, &dst);
                if artifact.executable {
                    set_executable(&dst);
                }
            }
        }
    }
}

/// Contribute `value` at a dotted key path, unioning with whatever is there.
///
/// Returns false when a segment of the path is occupied by something that is
/// not an object — `{"permissions": []}` merged at `permissions.deny` — in
/// which case nothing is written. Walking into it would have panicked; writing
/// through it would have replaced whatever the project had there.
fn merge_at(root: &mut serde_json::Value, key_path: &str, value: serde_json::Value) -> bool {
    let mut cursor = root;
    let keys: Vec<&str> = key_path.split('.').collect();
    for key in &keys[..keys.len() - 1] {
        if cursor.is_null() {
            *cursor = serde_json::json!({});
        }
        let Some(object) = cursor.as_object_mut() else {
            return false;
        };
        cursor = object
            .entry((*key).to_string())
            .or_insert_with(|| serde_json::json!({}));
    }
    let last = keys[keys.len() - 1].to_string();
    if cursor.is_null() {
        *cursor = serde_json::json!({});
    }
    let Some(object) = cursor.as_object_mut() else {
        return false;
    };
    union_into(object.entry(last).or_insert(serde_json::Value::Null), value)
}

/// Union `incoming` into `slot`: objects key-wise **at every depth**, arrays by
/// appending what is not already present. An empty slot takes the fragment
/// whole. Returns false when the two shapes disagree, leaving the slot untouched.
///
/// The recursion is the whole contract. A one-level union looks correct
/// because the merge keys are one level deep, and then the `hooks` fragment —
/// an object whose values are arrays — replaces `hooks.SessionStart` wholesale
/// and an operator's own entry at that event disappears with no finding
/// anywhere. Depth is not a special case of the rule; it is the rule.
///
/// Appending rather than sorting is what keeps a merge from rewriting a file it
/// does not own. Hook entries under one event run in order, so normalising that
/// order would change behaviour the project chose, and the destination is
/// shared by construction. Appending is still idempotent — a second pass finds
/// every entry present — and still deterministic, because the emission order is.
///
/// A shape disagreement is the collision rule reaching the inside of a
/// fragment. An operator who wrote `"SessionStart": {…}` instead of `[{…}]`
/// has one hook entry and a malformed file; replacing it drops their hook to
/// land harnex's, and the result validates clean. Keeping theirs and reporting
/// loses only a contribution the operator can re-apply once the shape is
/// fixed, which is the direction `regenerate` already takes on a conflict.
fn union_into(slot: &mut serde_json::Value, incoming: serde_json::Value) -> bool {
    match (&mut *slot, incoming) {
        (serde_json::Value::Object(existing), serde_json::Value::Object(incoming)) => incoming
            .into_iter()
            .map(|(key, value)| {
                union_into(
                    existing.entry(key).or_insert(serde_json::Value::Null),
                    value,
                )
            })
            // Every key is attempted rather than stopping at the first clash:
            // one malformed event must not withhold the rest.
            .reduce(|ok, merged| ok && merged)
            .unwrap_or(true),
        (serde_json::Value::Array(existing), serde_json::Value::Array(incoming)) => {
            for value in incoming {
                if !existing.contains(&value) {
                    existing.push(value);
                }
            }
            true
        }
        (serde_json::Value::Null, incoming) => {
            *slot = incoming;
            true
        }
        _ => false,
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

    // Both tiers merge into `permissions.allow` the same way. The floor is
    // what carries `Edit`, `Write` and the oracle grant, so a replacing merge
    // would leave a scaffold that prompts on every edit it makes.
    let allow: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .expect("permissions.allow is an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for rule in ["Edit", "Write", "Bash(harness *)"] {
        assert!(
            allow.contains(&rule),
            "[{lang}] merging the language allow fragment dropped '{rule}': {allow:?}"
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

    // --- Validation, under the policy the scaffold itself ships ---
    // Restating a policy here would make this fixture pass against a
    // configuration no scaffolded project has: an always-loaded rule the
    // manifest emits and `harness.toml` never declares would be a finding in
    // every real project and green in this test.
    let scaffolded = harness_core::config::Config::load_from(&proj_root.join("harness.toml"))
        .unwrap_or_else(|e| panic!("[{lang}] scaffolded harness.toml: {e}"));
    let declared = scaffolded
        .validate
        .as_ref()
        .unwrap_or_else(|| panic!("[{lang}] the scaffold's harness.toml declares no validators"));
    let rule_policy = declared
        .rules
        .clone()
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

    // --- Skill validation: every skill the manifest emitted ---
    // Discovered from the tree rather than named here, so a skill added to
    // `scaffold.toml` is validated without an edit, and one dropped fails the
    // emptiness check below instead of quietly reducing coverage to zero. The
    // policy comes from the same scaffolded config for the same reason as the
    // rules above.
    let skill_policy = declared
        .skills
        .clone()
        .unwrap_or_else(|| panic!("[{lang}] the scaffold's harness.toml declares no skill policy"));
    let emitted = glob_under(&proj_root.join(".claude/skills"), "*/SKILL.md");
    assert!(
        !emitted.is_empty(),
        "[{lang}] the scaffold emitted no skill; `[validate.skills]` would have no subject and \
         the loop `governance.md` describes would have no entry point"
    );
    for skill in &emitted {
        let findings = SkillValidator::new(&skill_policy)
            .validate_file(skill)
            .unwrap();
        assert_no_findings(
            lang,
            &format!("validate.skills({})", skill.display()),
            &findings,
        );
    }

    // --- Skill validation: the harnex SKILL.md itself must validate ---
    // The skill ships with the plugin; we copy it into the project tree's
    // canonical location (mirroring how an installed plugin's skill would be
    // discovered) and run SkillValidator. This exercises the full closed-set
    // surface against the plugin's own contract. Its description names the
    // modes it drives, so the side-effect heuristic is off for this one file.
    let plugin_skill =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/SKILL.md");
    if plugin_skill.exists() {
        let dst = proj_root.join(".claude/skills/harnex/SKILL.md");
        copy_file(&plugin_skill, &dst);
        let lenient = SkillsPolicy {
            flag_side_effect_verbs: false,
            ..skill_policy.clone()
        };
        let sv = SkillValidator::new(&lenient);
        let findings = sv.validate_file(&dst).unwrap();
        assert_no_findings(lang, "validate.skills(harnex SKILL.md)", &findings);
    }

    // --- Agent validation: the sub-agents the plugin ships must validate ---
    // A sub-agent's model and tool grant are frontmatter, so a typo costs the
    // field it was meant to set and Claude Code says nothing. `reject_unknown_keys`
    // is on here because these files are ours: the list lagging upstream would
    // cost a warning on a file we control, where staying silent costs the grant.
    let plugin_agents =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/agents");
    if plugin_agents.exists() {
        let policy = AgentsPolicy {
            reject_unknown_keys: true,
        };
        let av = AgentValidator::new(&policy);
        for agent in fs::read_dir(&plugin_agents).expect("agents dir").flatten() {
            let src = agent.path();
            if src.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let dst = proj_root
                .join(".claude/agents")
                .join(src.file_name().unwrap());
            copy_file(&src, &dst);
            let findings = av.validate_file(&dst).unwrap();
            assert_no_findings(
                lang,
                &format!("validate.agents({})", src.display()),
                &findings,
            );
        }
    }

    // --- The `extend skill` scaffold template must itself validate clean ---
    // The skeleton `extend skill <name>` writes (before the operator fills the
    // body) must pass SkillValidator as-is, so a freshly scaffolded skill is
    // spec-correct from the first commit.
    // --- The `extend rule` template must itself validate clean ---
    // Same contract as the skill skeleton below: the file the verb writes is
    // held to the policy the scaffold ships, so a derived rule is spec-correct
    // from its first commit rather than from whenever someone runs the gate.
    let rule_template = plugin_templates().join("common/rule-template.md");
    let dst = proj_root.join(".claude/rules/example-derived.md");
    copy_file(&rule_template, &dst);
    let findings = rv.validate_file(&dst).unwrap();
    assert_no_findings(lang, "validate.rules(rule-template.md)", &findings);

    let skill_template = plugin_templates().join("common/skill-template.md");
    let dst = proj_root.join(".claude/skills/example-skill/SKILL.md");
    copy_file(&skill_template, &dst);
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
        ("evidence", config.evidence.is_some()),
    ] {
        assert!(
            present,
            "[{lang}] the scaffold's harness.toml declares no [{surface}], so the oracle \
             surface the foundation rules send the operator to is unreachable"
        );
    }

    // The claim shape the generated rules are told to write must resolve to a
    // registered verifier, or every pointer an operator adds reports
    // `evidence-unknown-provenance` instead of being checked.
    let evidence = config.evidence.as_ref().unwrap();
    assert!(
        evidence
            .verifiers
            .iter()
            .any(|v| v.provenance == evidence.default_provenance && v.strategy == "file-path-line"),
        "[{lang}] `path.ext:line` claims have no verifier: default provenance is '{}', \
         declared verifiers are {:?}",
        evidence.default_provenance,
        evidence
            .verifiers
            .iter()
            .map(|v| (&v.provenance, &v.strategy))
            .collect::<Vec<_>>()
    );
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
        absent.iter().any(|d| d.starts_with("hooks/post-format-")),
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
fn a_fragment_never_replaces_an_incumbent_of_a_different_shape() {
    // `"SessionStart": {…}` instead of `[{…}]` is a malformed settings file
    // with one real hook in it — an easy mistake, since most published hook
    // examples show a single entry. Replacing it to land harnex's array drops
    // the operator's hook and the result validates clean. And a scalar at an
    // intermediate segment used to panic the walk outright.
    for (incumbent, key, fragment) in [
        (
            serde_json::json!({"hooks": {"SessionStart": {"matcher": "startup"}}}),
            "hooks",
            serde_json::json!({"SessionStart": [{"matcher": "startup|resume"}]}),
        ),
        (
            serde_json::json!({"permissions": []}),
            "permissions.deny",
            serde_json::json!(["Read(.env)"]),
        ),
        (
            serde_json::json!({"permissions": "none"}),
            "permissions.allow",
            serde_json::json!(["Edit"]),
        ),
    ] {
        let mut settings = incumbent.clone();
        let merged = merge_at(&mut settings, key, fragment);
        assert!(!merged, "a shape clash at '{key}' reported success");
        assert_eq!(
            settings, incumbent,
            "a shape clash at '{key}' changed the project's file"
        );
    }
}

/// Scaffolding a repo that already has files at the manifest's destinations
/// must not destroy them.
///
/// This is the common case, not the exceptional one. Claude Code reads a root
/// `CLAUDE.md` with no `.claude/` directory at all, so "no `.claude/`" — the
/// condition scaffold mode is defined by — says nothing about whether a
/// `CLAUDE.md` exists; seven of the repositories this was measured against
/// were in exactly that state, one of them 306 lines. A repo with git hooks
/// already has `hooks/`, which is where four `copy` artifacts land.
#[test]
fn scaffolding_over_an_incumbent_preserves_it() {
    let templates = plugin_templates();
    let manifest = ScaffoldManifest::load(&templates).unwrap();
    let project = TempDir::new().unwrap();
    let proj_root = project.path();

    let claude_md = "# acme\n\nProject notes live in `.acme/memories`.\n";
    let pre_commit = "#!/bin/sh\nexec ./scripts/lint-staged\n";
    let governance = "# Governance\n\nWe decide at the Thursday retro.\n";
    for (rel, body) in [
        ("CLAUDE.md", claude_md),
        ("hooks/pre-commit", pre_commit),
        (".claude/rules/governance.md", governance),
    ] {
        let path = proj_root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
    }

    // `hooks` is the one object-shaped fragment in the manifest, and its values
    // are arrays. A union that stops at the merge key replaces
    // `hooks.SessionStart` wholesale and the operator's own entry at that event
    // disappears — structurally valid afterwards, so no validator and no audit
    // says anything. Pre-populating an array under `permissions` alone cannot
    // reach that shape.
    let incumbent_hook = serde_json::json!({
        "matcher": "startup",
        "hooks": [{"type": "command", "command": "./scripts/acme-context.sh"}],
    });
    let mut settings = serde_json::json!({
        "permissions": {"allow": ["Bash(acme *)"]},
        "env": {"ACME": "1"},
        "hooks": {"SessionStart": [incumbent_hook]},
    });
    for tier in [Tier::Foundation, Tier::Language] {
        emit_tier(
            &manifest,
            tier,
            &templates,
            Some("rust"),
            proj_root,
            &mut settings,
        );
    }

    // `copy` and `seed`: the incumbent is the project's and is left alone.
    assert_eq!(
        fs::read_to_string(proj_root.join("hooks/pre-commit")).unwrap(),
        pre_commit,
        "a `copy` artifact replaced the project's own hook"
    );
    assert_eq!(
        fs::read_to_string(proj_root.join(".claude/rules/governance.md")).unwrap(),
        governance,
        "a `seed` artifact replaced governance the project had already written"
    );

    // `managed`: a file harnex did not create is not one it may edit. Its
    // partition is for regenerating a file harnex wrote, not a licence to
    // append to the project's own.
    assert_eq!(
        fs::read_to_string(proj_root.join("CLAUDE.md")).unwrap(),
        claude_md,
        "a `managed` artifact edited a CLAUDE.md the project had written"
    );

    // `merge`: the project's own entries survive beside both tiers', at every
    // depth of the fragment.
    let allow = settings["permissions"]["allow"].as_array().unwrap();
    assert!(allow.iter().any(|v| v == "Bash(acme *)"));
    assert!(allow.iter().any(|v| v == "Bash(cargo *)"));
    assert!(allow.iter().any(|v| v == "Edit"));
    assert_eq!(settings["env"]["ACME"], "1");

    let session_start = settings["hooks"]["SessionStart"].as_array().unwrap();
    assert!(
        session_start.contains(&incumbent_hook),
        "the project's own SessionStart entry was erased by the foundation \
         fragment: {session_start:?}"
    );
    assert!(
        session_start.len() > 1,
        "harnex contributed no SessionStart entry beside the incumbent"
    );
    assert_eq!(
        session_start[0], incumbent_hook,
        "the incumbent's position moved; hook entries under one event run in \
         order, so a merge that reorders them changes behaviour the project chose"
    );

    // Emission is idempotent: a second pass changes nothing.
    let before: Vec<(PathBuf, String)> = glob_under(proj_root, "**/*")
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| {
            let body = fs::read_to_string(&p).unwrap_or_default();
            (p, body)
        })
        .collect();
    for tier in [Tier::Foundation, Tier::Language] {
        emit_tier(
            &manifest,
            tier,
            &templates,
            Some("rust"),
            proj_root,
            &mut settings,
        );
    }
    for (path, body) in before {
        assert_eq!(
            fs::read_to_string(&path).unwrap_or_default(),
            body,
            "re-running the scaffold changed {}",
            path.display()
        );
    }
}

/// A repository with two stacks gets both, and neither displaces the other.
///
/// Detection answers with a set: a lockfile is evidence a stack is present,
/// never evidence it is the only one. Two of the repositories this was measured
/// against carry `pnpm-lock.yaml` and `uv.lock` together — one of them 17,085
/// `.py` files beside 3,433 `.ts` — so resolving the pair by row order would
/// wire a formatter that silently skips most of the source and grant a
/// toolchain the majority language never uses.
#[test]
fn a_two_stack_repo_gets_both_language_tiers() {
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
    for lang in ["python", "typescript"] {
        emit_tier(
            &manifest,
            Tier::Language,
            &templates,
            Some(lang),
            proj_root,
            &mut settings,
        );
    }

    for lang in ["python", "typescript"] {
        assert!(
            proj_root
                .join(format!("hooks/post-format-{lang}.sh"))
                .exists(),
            "{lang}'s formatter is missing; one stack displaced the other"
        );
        assert!(
            proj_root
                .join(format!(".claude/rules/{lang}-conventions.md"))
                .exists(),
            "{lang}'s conventions rule is missing"
        );
    }

    // One PostToolUse entry per stack. Each script dispatches on the file
    // extension and exits 0 on anything it does not own, so they coexist
    // without arbitration.
    let entries = settings["hooks"]["PostToolUse"].as_array().unwrap();
    let args: Vec<String> = entries
        .iter()
        .filter_map(|e| e["hooks"][0]["args"][0].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        args,
        vec!["post-format-python.sh", "post-format-typescript.sh"],
        "both formatters must be wired, in a stable order"
    );

    let allow: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for rule in ["Bash(uv *)", "Bash(pnpm *)", "Bash(harness *)"] {
        assert!(allow.contains(&rule), "{rule} missing from {allow:?}");
    }

    let settings_path = proj_root.join(".claude/settings.json");
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    let findings = SettingsValidator::new()
        .validate_file(&settings_path, SettingsScope::Project)
        .unwrap();
    assert_no_findings("python+typescript", "validate.settings", &findings);

    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let auditor = || {
        ProjectAuditor::new(proj_root)
            .with_plugin_root(plugin_root.clone())
            .run()
            .unwrap()
    };
    assert_no_findings("python+typescript", "audit", &auditor().findings);
    assert_scaffold_is_operable("python+typescript", proj_root);

    // An anchored reference to a language-tier script must be judged like any
    // other. The wiring check ranged only over destinations that resolve
    // without a language, so the formatter — the one language-tier script a
    // hook points at — was the one it could never judge, and at runtime a
    // missing verifier makes `_runner.sh` skip and exit 0.
    let settings_path = proj_root.join(".claude/settings.json");
    let mut wired: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    wired["hooks"]["PostToolUse"][0]["hooks"][0]["command"] =
        serde_json::json!("${CLAUDE_PROJECT_DIR}/hooks/post-format-typescript.sh");
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&wired).unwrap(),
    )
    .unwrap();
    fs::remove_file(proj_root.join("hooks/post-format-typescript.sh")).unwrap();
    let missing = auditor().findings;
    assert!(
        missing.iter().any(|f| f.slug == "audit-hook-script-missing"
            && f.message.contains("post-format-typescript.sh")),
        "a wired language-tier script that is absent went unreported: {missing:?}"
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
