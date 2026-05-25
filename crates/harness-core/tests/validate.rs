//! Integration tests for validate.

use harness_core::config::{RulesPolicy, SkillsPolicy};
use harness_core::envelope::Severity;
use harness_core::validate::{RuleValidator, SettingsScope, SettingsValidator, SkillValidator};
use std::path::Path;
use tempfile::TempDir;

#[test]
fn rule_validator_flags_missing_paths_frontmatter() {
    let policy = RulesPolicy {
        max_lines: 200,
        always_loaded_slugs: vec!["constitution".into()],
    };
    let v = RuleValidator::new(&policy);
    let md = "# Body without frontmatter\n";
    let findings = v.validate_text(md, Path::new("my-rule.md"));
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "rule-missing-paths-frontmatter")
    );
}

#[test]
fn rule_validator_accepts_constitution_without_paths() {
    let policy = RulesPolicy {
        max_lines: 200,
        always_loaded_slugs: vec!["constitution".into()],
    };
    let v = RuleValidator::new(&policy);
    let md = "# I. Some article\n\nText.";
    let findings = v.validate_text(md, Path::new("constitution.md"));
    assert!(findings.is_empty(), "unexpected: {findings:?}");
}

#[test]
fn rule_validator_flags_overlong_rule() {
    let policy = RulesPolicy {
        max_lines: 5,
        always_loaded_slugs: vec![],
    };
    let v = RuleValidator::new(&policy);
    let md = "---\npaths: [\"x\"]\n---\n".to_string()
        + &(1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
    let findings = v.validate_text(&md, Path::new("rule.md"));
    assert!(findings.iter().any(|f| f.slug == "rule-too-long"));
}

#[test]
fn skill_validator_flags_missing_frontmatter() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "# No frontmatter SKILL\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-missing-frontmatter")
    );
}

#[test]
fn skill_validator_flags_bad_name_shape() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: Bad_Name\ndescription: short\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("bad_name/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(findings.iter().any(|f| f.slug == "skill-name-shape"));
}

#[test]
fn skill_validator_flags_name_mismatch() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: other-name\ndescription: short\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("dir-name/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(findings.iter().any(|f| f.slug == "skill-name-mismatch"));
}

#[test]
fn skill_validator_flags_description_over_budget() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 50,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let long = "x".repeat(100);
    let md = format!("---\nname: my-skill\ndescription: {long}\n---\nBody\n");
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(&md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-description-over-budget")
    );
}

#[test]
fn skill_validator_silent_on_side_effect_verbs_by_default() {
    // The heuristic is opt-in. With `flag_side_effect_verbs: false` (default),
    // a description full of side-effect verbs produces no finding — matching
    // prose to intent is the kind of advisory check the keep-soften-cut policy
    // pushes to opt-in advisory.
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md =
        "---\nname: deploy-app\ndescription: Deploy the application to production\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("deploy-app/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-side-effect-no-disable"),
        "default-off must not flag side-effect verbs: {findings:?}"
    );
}

#[test]
fn skill_validator_recommends_disable_for_side_effect_verbs_when_opted_in() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: true,
    };
    let v = SkillValidator::new(&policy);
    let md =
        "---\nname: deploy-app\ndescription: Deploy the application to production\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("deploy-app/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-side-effect-no-disable"
                && matches!(f.severity, Severity::Minor))
    );
}

#[test]
fn skill_validator_accepts_disable_on_side_effect_when_opted_in() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: true,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: deploy-app\ndescription: Deploy the application\ndisable-model-invocation: true\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("deploy-app/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-side-effect-no-disable")
    );
}

#[test]
fn skill_validator_ignores_substring_matches_when_opted_in() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: true,
    };
    let v = SkillValidator::new(&policy);
    for word in &[
        "committee",
        "sender",
        "publisher",
        "released",
        "deployed",
        "deleted",
        "submitted",
    ] {
        let md = format!("---\nname: my-skill\ndescription: The {word} handles data\n---\nBody\n");
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("my-skill/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let findings = v.validate_text(&md, &path);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "skill-side-effect-no-disable"),
            "'{word}' should not trigger side-effect warning"
        );
    }
}

#[test]
fn settings_validator_flags_unknown_hook_event() {
    let v = SettingsValidator::new();
    let json = r#"{
        "hooks": {
            "InstructionsLoaded": [],
            "MadeUpEvent": []
        }
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    let unknowns: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "settings-unknown-hook-event")
        .collect();
    assert_eq!(unknowns.len(), 1);
    assert!(unknowns[0].message.contains("MadeUpEvent"));
}

#[test]
fn settings_validator_warns_on_empty_deny() {
    let v = SettingsValidator::new();
    let json = r#"{"permissions": {"allow": ["Bash(ls *)"]}}"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(findings.iter().any(|f| f.slug == "settings-no-deny-rules"));
}

#[test]
fn settings_validator_accepts_well_formed() {
    let v = SettingsValidator::new();
    let json = r#"{
        "hooks": {"SessionStart": [], "Stop": [], "PreCompact": []},
        "permissions": {"allow": [], "deny": ["Bash(sudo *)"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(findings.is_empty(), "unexpected: {findings:?}");
}

// ---------- Skill frontmatter expansion (Part 1) ----------

#[test]
fn skill_validator_flags_invalid_context_value() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\ncontext: inline\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-context-invalid" && matches!(f.severity, Severity::Major)),
        "expected skill-context-invalid finding: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_valid_context_fork() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\ncontext: fork\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings.iter().any(|f| f.slug == "skill-context-invalid"),
        "context: fork should be accepted: {findings:?}"
    );
}

#[test]
fn skill_validator_flags_invalid_effort_value() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\neffort: ultra\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-effort-invalid" && matches!(f.severity, Severity::Major)),
        "expected skill-effort-invalid finding: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_valid_effort_levels() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    for level in &["low", "medium", "high", "xhigh", "max"] {
        let md = format!("---\nname: my-skill\ndescription: a skill\neffort: {level}\n---\nBody\n");
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("my-skill/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let findings = v.validate_text(&md, &path);
        assert!(
            !findings.iter().any(|f| f.slug == "skill-effort-invalid"),
            "effort: {level} should be accepted: {findings:?}"
        );
    }
}

#[test]
fn skill_validator_accepts_string_and_array_allowed_tools() {
    // The spec accepts allowed-tools as a space-separated STRING or a YAML
    // list — neither form is a finding.
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for at in ["Bash(gh *) Read", "[Bash, Read, Edit]"] {
        let md =
            format!("---\nname: my-skill\ndescription: a skill\nallowed-tools: {at}\n---\nBody\n");
        let findings = v.validate_text(&md, &path);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "skill-allowed-tools-invalid"),
            "allowed-tools {at:?} is valid, should not be flagged: {findings:?}"
        );
    }
}

#[test]
fn skill_validator_flags_non_string_non_array_allowed_tools() {
    // A scalar that is neither a string nor a list (here a number) is invalid.
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nallowed-tools: 42\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-allowed-tools-invalid"
                && matches!(f.severity, Severity::Major)),
        "expected skill-allowed-tools-invalid for a numeric value: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_array_allowed_tools() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nallowed-tools:\n  - Bash\n  - Read\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-allowed-tools-invalid"),
        "array of strings should be accepted: {findings:?}"
    );
}

#[test]
fn skill_validator_flags_invalid_user_invocable() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nuser-invocable: yes-please\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-user-invocable-invalid"),
        "expected skill-user-invocable-invalid: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_any_agent_and_model_without_finding() {
    // agent and model are valid free-form fields; flagging them is CUT-tier
    // noise (a finding that implies no action). A custom agent and an explicit
    // model produce no agent/model finding.
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nagent: custom-bot\nmodel: claude-opus-4-20250514\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-agent-unknown" || f.slug == "skill-model-override"),
        "agent/model must not produce a finding: {findings:?}"
    );
}

#[test]
fn skill_validator_flags_unknown_hook_event_in_skill() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nhooks:\n  MadeUpEvent: []\n  PreToolUse: []\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-hooks-unknown-event"),
        "expected skill-hooks-unknown-event: {findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-hooks-unknown-event" && f.message.contains("PreToolUse")),
        "PreToolUse should be accepted"
    );
}

#[test]
fn skill_validator_flags_invalid_paths_glob() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\npaths:\n  - \"src/**/*.rs\"\n  - \"[invalid\"\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings.iter().any(|f| f.slug == "skill-paths-invalid"),
        "expected skill-paths-invalid for bad glob: {findings:?}"
    );
}

#[test]
fn skill_validator_ignores_unknown_keys_by_default() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: false,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nbogus_key: x\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-unknown-frontmatter-key"),
        "unknown-key check is opt-in and must not fire by default: {findings:?}"
    );
}

#[test]
fn skill_validator_flags_unknown_key_when_enabled() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: true,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nbogus_key: x\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    let unknown: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "skill-unknown-frontmatter-key")
        .collect();
    assert_eq!(unknown.len(), 1, "expected exactly one: {findings:?}");
    assert!(unknown[0].message.contains("bogus_key"));
    assert!(matches!(unknown[0].severity, Severity::Major));
}

#[test]
fn skill_validator_accepts_unmodeled_spec_keys_when_strict() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
        reject_unknown_keys: true,
        flag_side_effect_verbs: false,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nargument-hint: \"<file>\"\narguments: \"$1\"\nshell: bash\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "skill-unknown-frontmatter-key"),
        "valid-but-unmodeled spec keys must not be flagged: {findings:?}"
    );
}

// ---------- Settings expansion (Part 2) ----------

#[test]
fn settings_validator_flags_invalid_skill_override() {
    let v = SettingsValidator::new();
    let json = r#"{
        "skillOverrides": {
            "my-skill": "turbo",
            "other-skill": "on"
        },
        "permissions": {"deny": ["x"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    let invalids: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "settings-skill-override-invalid")
        .collect();
    assert_eq!(invalids.len(), 1, "expected 1 invalid: {invalids:?}");
    assert!(invalids[0].message.contains("turbo"));
}

#[test]
fn settings_validator_accepts_valid_skill_overrides() {
    let v = SettingsValidator::new();
    let json = r#"{
        "skillOverrides": {
            "a": "on",
            "b": "name-only",
            "c": "user-invocable-only",
            "d": "off"
        },
        "permissions": {"deny": ["x"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-skill-override-invalid"),
        "all valid overrides should be accepted: {findings:?}"
    );
}

#[test]
fn settings_validator_flags_absent_permissions_as_no_deny() {
    // A settings.json with no permissions block at all has zero guardrails —
    // the no-deny advisory must fire, not silently skip.
    let v = SettingsValidator::new();
    let json = r#"{ "hooks": {} }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        findings.iter().any(|f| f.slug == "settings-no-deny-rules"),
        "absent permissions must surface settings-no-deny-rules: {findings:?}"
    );
}

#[test]
fn settings_validator_warns_overly_permissive_allow() {
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {
            "allow": ["Bash(rm:*)", "Bash(ls:*)"],
            "deny": ["Bash(ls:*)"]
        }
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    let permissive: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "settings-overly-permissive")
        .collect();
    assert_eq!(
        permissive.len(),
        1,
        "expected 1 overly-permissive: {findings:?}"
    );
    assert!(permissive[0].message.contains("Bash(rm:*)"));
}

#[test]
fn settings_validator_accepts_scoped_allow() {
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {
            "allow": ["Bash(rm:./tmp/*)"],
            "deny": ["Bash(sudo *)"]
        }
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-overly-permissive"),
        "scoped pattern should not trigger: {findings:?}"
    );
}

#[test]
fn settings_validator_flags_space_wildcard_dangerous_allow() {
    // The canonical spelling the permission dialog writes is the space form
    // `Bash(rm -rf *)`, equivalent to `Bash(rm -rf:*)`. Detection must catch it
    // regardless of wildcard style, and a colon-style deny must still excuse it.
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {
            "allow": ["Bash(rm -rf *)", "Bash(sudo *)"],
            "deny": ["Bash(sudo:*)"]
        }
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    let permissive: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "settings-overly-permissive")
        .collect();
    // `rm -rf *` is dangerous + undenied → flagged; `sudo *` is excused by the
    // colon-style `sudo:*` deny.
    assert_eq!(
        permissive.len(),
        1,
        "expected only rm -rf flagged: {findings:?}"
    );
    assert!(permissive[0].message.contains("Bash(rm -rf *)"));
}

#[test]
fn settings_validator_no_finding_for_auto_memory() {
    // autoMemoryEnabled is a valid, intentional config; acknowledging it with
    // an Info finding is CUT-tier noise. Present or absent, it produces nothing.
    let v = SettingsValidator::new();
    let json = r#"{
        "autoMemoryEnabled": false,
        "permissions": {"deny": ["x"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-auto-memory-configured"),
        "autoMemoryEnabled must not produce a finding: {findings:?}"
    );
}

// ---------- Project-scope no-op key detection (Phase 3) ----------

#[test]
fn settings_validator_flags_project_scope_noop_keys() {
    // Each key the live /en/settings doc documents as silently ignored in
    // project / local scope produces a Major finding when present.
    let v = SettingsValidator::new();
    let json = r#"{
        "autoMemoryDirectory": "~/somewhere",
        "autoMode": {"environment": []},
        "skipDangerousModePermissionPrompt": true,
        "permissions": {"deny": ["Bash(sudo *)"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    let noop_keys: Vec<_> = findings
        .iter()
        .filter(|f| f.slug == "settings-project-scope-noop-key")
        .collect();
    assert_eq!(
        noop_keys.len(),
        3,
        "expected 3 noop-key findings: {findings:?}"
    );
    let messages: Vec<&str> = noop_keys.iter().map(|f| f.message.as_str()).collect();
    for key in [
        "autoMemoryDirectory",
        "autoMode",
        "skipDangerousModePermissionPrompt",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(key)),
            "missing finding for '{key}'"
        );
    }
}

#[test]
fn settings_validator_flags_default_mode_auto_in_project_scope() {
    // defaultMode = "auto" is valid wire syntax but silently no-ops outside
    // user/managed scope. The slug differs from the noop-key slug because the
    // KEY (`defaultMode`) is valid at every scope; only the VALUE is scope-restricted.
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {"defaultMode": "auto", "deny": ["Bash(sudo *)"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "settings-project-scope-noop-value"
                && f.message.contains("defaultMode")
                && f.message.contains("\"auto\"")),
        "expected defaultMode=auto noop value finding: {findings:?}"
    );
}

#[test]
fn settings_validator_accepts_default_mode_auto_at_user_scope() {
    // defaultMode = "auto" is fully valid at user / managed scope — the
    // scope-noop check must NOT fire there.
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {"defaultMode": "auto", "deny": ["Bash(sudo *)"]}
    }"#;
    for scope in [SettingsScope::User, SettingsScope::Managed] {
        let findings = v.validate_text(json, Path::new("settings.json"), scope);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "settings-project-scope-noop-value"),
            "{scope:?}: defaultMode=auto must be accepted: {findings:?}"
        );
    }
}

#[test]
fn settings_validator_flags_invalid_default_mode() {
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {"defaultMode": "turbo", "deny": ["Bash(sudo *)"]}
    }"#;
    let findings = v.validate_text(
        json,
        Path::new(".claude/settings.json"),
        SettingsScope::Project,
    );
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "settings-default-mode-invalid" && f.message.contains("turbo")),
        "expected settings-default-mode-invalid: {findings:?}"
    );
}

#[test]
fn settings_validator_accepts_valid_default_modes() {
    let v = SettingsValidator::new();
    for mode in &[
        "default",
        "acceptEdits",
        "plan",
        "dontAsk",
        "bypassPermissions",
    ] {
        let json = format!(
            r#"{{ "permissions": {{ "defaultMode": "{mode}", "deny": ["Bash(sudo *)"] }} }}"#
        );
        let findings = v.validate_text(
            &json,
            Path::new(".claude/settings.json"),
            SettingsScope::Project,
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "settings-default-mode-invalid"),
            "defaultMode = {mode} should be accepted: {findings:?}"
        );
    }
}

#[test]
fn settings_validator_excuses_noop_keys_at_user_and_managed_scope() {
    // User and managed scopes honor the otherwise-no-op keys; the validator
    // must remain silent on those scopes regardless of file path.
    let v = SettingsValidator::new();
    let json = r#"{
        "autoMemoryDirectory": "~/somewhere",
        "autoMode": {"environment": []},
        "skipDangerousModePermissionPrompt": true,
        "permissions": {"deny": ["Bash(sudo *)"]}
    }"#;
    for scope in [SettingsScope::User, SettingsScope::Managed] {
        let findings = v.validate_text(json, Path::new("settings.json"), scope);
        assert!(
            !findings
                .iter()
                .any(|f| f.slug == "settings-project-scope-noop-key"),
            "{scope:?}: noop keys must be honored, not flagged: {findings:?}"
        );
    }
}
