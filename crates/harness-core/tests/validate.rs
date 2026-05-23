//! Integration tests for validate.

use harness_core::config::{RulesPolicy, SkillsPolicy};
use harness_core::envelope::Severity;
use harness_core::validate::{RuleValidator, SettingsValidator, SkillValidator};
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
fn skill_validator_recommends_disable_for_side_effect_verbs() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
fn skill_validator_accepts_disable_on_side_effect() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
fn skill_validator_ignores_substring_matches() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(findings.iter().any(|f| f.slug == "settings-no-deny-rules"));
}

#[test]
fn settings_validator_accepts_well_formed() {
    let v = SettingsValidator::new();
    let json = r#"{
        "hooks": {"SessionStart": [], "Stop": [], "PreCompact": []},
        "permissions": {"allow": [], "deny": ["Bash(sudo *)"]}
    }"#;
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(findings.is_empty(), "unexpected: {findings:?}");
}

// ---------- Skill frontmatter expansion (Part 1) ----------

#[test]
fn skill_validator_flags_invalid_context_value() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
fn skill_validator_flags_non_array_allowed_tools() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nallowed-tools: Bash\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-allowed-tools-invalid"
                && matches!(f.severity, Severity::Major)),
        "expected skill-allowed-tools-invalid finding: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_array_allowed_tools() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
fn skill_validator_emits_info_for_unknown_agent() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
    };
    let v = SkillValidator::new(&policy);
    let md = "---\nname: my-skill\ndescription: a skill\nagent: custom-bot\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-agent-unknown" && matches!(f.severity, Severity::Info)),
        "expected skill-agent-unknown Info: {findings:?}"
    );
}

#[test]
fn skill_validator_accepts_known_agent_types() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
    };
    let v = SkillValidator::new(&policy);
    for agent in &["Explore", "Plan", "general-purpose"] {
        let md = format!("---\nname: my-skill\ndescription: a skill\nagent: {agent}\n---\nBody\n");
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("my-skill/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let findings = v.validate_text(&md, &path);
        assert!(
            !findings.iter().any(|f| f.slug == "skill-agent-unknown"),
            "agent {agent} should be accepted: {findings:?}"
        );
    }
}

#[test]
fn skill_validator_emits_info_for_model_override() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
    };
    let v = SkillValidator::new(&policy);
    let md =
        "---\nname: my-skill\ndescription: a skill\nmodel: claude-opus-4-20250514\n---\nBody\n";
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("my-skill/SKILL.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let findings = v.validate_text(md, &path);
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "skill-model-override" && matches!(f.severity, Severity::Info)),
        "expected skill-model-override Info: {findings:?}"
    );
}

#[test]
fn skill_validator_flags_unknown_hook_event_in_skill() {
    let policy = SkillsPolicy {
        max_skill_md_lines: 500,
        max_description_chars: 400,
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-skill-override-invalid"),
        "all valid overrides should be accepted: {findings:?}"
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
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
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-overly-permissive"),
        "scoped pattern should not trigger: {findings:?}"
    );
}

#[test]
fn settings_validator_notes_auto_memory_configured() {
    let v = SettingsValidator::new();
    let json = r#"{
        "autoMemoryEnabled": true,
        "permissions": {"deny": ["x"]}
    }"#;
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(
        findings
            .iter()
            .any(|f| f.slug == "settings-auto-memory-configured"
                && matches!(f.severity, Severity::Info)),
        "expected settings-auto-memory-configured Info: {findings:?}"
    );
}

#[test]
fn settings_validator_no_auto_memory_finding_when_absent() {
    let v = SettingsValidator::new();
    let json = r#"{
        "permissions": {"deny": ["x"]}
    }"#;
    let findings = v.validate_text(json, Path::new(".claude/settings.json"));
    assert!(
        !findings
            .iter()
            .any(|f| f.slug == "settings-auto-memory-configured"),
        "no auto-memory finding when key absent: {findings:?}"
    );
}
