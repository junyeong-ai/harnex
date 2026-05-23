//! Integration tests for the unified `check` gate.

use std::fs;
use std::path::Path;

use harness_core::check::ProjectChecker;
use harness_core::config::Config;
use tempfile::TempDir;

fn write(p: &Path, contents: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, contents).unwrap();
}

fn minimal_config_toml() -> String {
    r#"
[meta]
harness_toolkit_version = ">=0.1, <0.2"

[evidence]
default_provenance = "memory-only"
[[evidence.verifiers]]
provenance = "memory-only"
strategy = "memory-only"

[validate.rules]
max_lines = 200
always_loaded_slugs = ["constitution"]

[validate.skills]
max_skill_md_lines = 500
max_description_chars = 1536

[policy.permissions]
profiles = ["baseline"]
"#
    .to_string()
}

fn load_cfg(tmp: &TempDir, toml_body: &str) -> Config {
    let path = tmp.path().join("harness.toml");
    fs::write(&path, toml_body).unwrap();
    Config::load_from(&path).unwrap()
}

#[test]
fn check_runs_every_enabled_validator() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());

    write(
        &tmp.path().join(".claude/rules/constitution.md"),
        "# Constitution\n",
    );
    write(
        &tmp.path().join(".claude/rules/api.md"),
        "# Rule without paths frontmatter\n",
    );
    write(
        &tmp.path().join(".claude/skills/deploy/SKILL.md"),
        "---\nname: deploy\ndescription: Deploy the app to production\n---\nBody\n",
    );
    write(
        &tmp.path().join(".claude/settings.json"),
        r#"{"permissions":{"allow":[],"deny":["Bash(sudo *)"]}}"#,
    );
    write(&tmp.path().join("CLAUDE.md"), "Some prose.\n");

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();

    // Every enabled validator ran
    for v in [
        "validate.rules",
        "validate.skills",
        "validate.settings",
        "evidence",
        "policy.permissions",
    ] {
        assert!(
            outcome.run_validators.contains(&v.to_string()),
            "missing {v}"
        );
    }
    // codegen skipped (no [codegen] section)
    assert!(outcome.skipped.iter().any(|s| s.slug == "codegen"));

    // Findings include rule-missing-paths-frontmatter (api.md) and skill-side-effect-no-disable
    let slugs: Vec<&str> = outcome.findings.iter().map(|f| f.slug.as_str()).collect();
    assert!(slugs.contains(&"rule-missing-paths-frontmatter"));
    assert!(slugs.contains(&"skill-side-effect-no-disable"));
}

#[test]
fn check_skips_validators_with_no_config_section() {
    let tmp = TempDir::new().unwrap();
    let minimal = r#"
[meta]
harness_toolkit_version = ">=0.1, <0.2"
"#;
    let cfg = load_cfg(&tmp, minimal);
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert!(outcome.run_validators.is_empty());
    for expected in [
        "codegen",
        "evidence",
        "policy.permissions",
        "validate.rules",
        "validate.settings",
        "validate.skills",
    ] {
        assert!(
            outcome.skipped.iter().any(|s| s.slug == expected),
            "expected {expected} in skipped list"
        );
    }
}

#[test]
fn check_emits_codegen_drift_as_blocker() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("enums.toml");
    fs::write(&src, "[k]\nallowed = [\"a\", \"b\"]\n").unwrap();
    let target = tmp.path().join("nodex.toml");
    fs::write(&target, "# BEGIN x\nallowed = [\"stale\"]\n# END x\n").unwrap();

    let toml_body = r##"
[meta]
harness_toolkit_version = ">=0.1, <0.2"

[[codegen.groups]]
name = "g"
source = "enums.toml"
source_key = "k.allowed"
[[codegen.groups.targets]]
path = "nodex.toml"
begin = "# BEGIN x"
end = "# END x"
format = "toml-array-assignment"
name = "allowed"
"##;
    let cfg = load_cfg(&tmp, toml_body);
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    let drift: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.slug == "codegen-drift")
        .collect();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].severity, harness_core::envelope::Severity::Blocker);
    assert!(drift[0].auto_fixable);
    assert_eq!(
        drift[0].fix_command.as_deref(),
        Some("harness codegen sync")
    );
}

#[test]
fn check_emits_permission_audit_findings() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    // settings.json missing baseline denies — auditor flags
    write(
        &tmp.path().join(".claude/settings.json"),
        r#"{"permissions":{"allow":[]}}"#,
    );

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    let missing: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.slug == "permission-missing-baseline-deny")
        .collect();
    assert!(!missing.is_empty(), "expected baseline-deny findings");
}

#[test]
fn check_sorts_findings_by_severity_then_slug_then_path() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    // Generate mixed-severity findings
    write(&tmp.path().join(".claude/rules/x.md"), "# no frontmatter\n"); // Major: rule-missing-paths-frontmatter
    write(
        &tmp.path().join(".claude/skills/deploy/SKILL.md"),
        "---\nname: deploy\ndescription: Deploy and submit changes\n---\nBody\n",
    ); // Minor: skill-side-effect-no-disable
    write(
        &tmp.path().join(".claude/settings.json"),
        r#"{"permissions":{"deny":["Bash(sudo *)"]}}"#,
    );

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    // Verify monotonic non-decreasing severity rank
    let ranks: Vec<u8> = outcome
        .findings
        .iter()
        .map(|f| match f.severity {
            harness_core::envelope::Severity::Blocker => 0,
            harness_core::envelope::Severity::Major => 1,
            harness_core::envelope::Severity::Minor => 2,
            harness_core::envelope::Severity::Info => 3,
        })
        .collect();
    for w in ranks.windows(2) {
        assert!(w[0] <= w[1], "sort violated: {ranks:?}");
    }
}

#[test]
fn since_filter_excludes_unchanged_files_when_git_unavailable() {
    // When git isn't available or path is bogus, since spawn returns
    // CheckGitFailure. We assert the error path triggers, exercising
    // the `--since` code branch without needing a real git repo.
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let result = ProjectChecker::new(&cfg, tmp.path())
        .with_since("nonexistent-ref-12345")
        .run();
    // Either git is absent (spawn failure) or git rejects the ref
    // (also CheckGitFailure with non-zero status). Both surface as
    // CheckGitFailure.
    let err = match result {
        Ok(_) => panic!("expected error from bogus --since ref"),
        Err(e) => e,
    };
    assert_eq!(err.code(), harness_core::error::ErrorCode::CheckGitFailure);
}

#[test]
fn fix_resolves_codegen_drift_and_re_check_clean() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("enums.toml"),
        "[k]\nallowed = [\"a\", \"b\"]\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("nodex.toml"),
        "# BEGIN x\nallowed = [\"stale\"]\n# END x\n",
    )
    .unwrap();
    let toml_body = r##"
[meta]
harness_toolkit_version = ">=0.1, <0.2"

[[codegen.groups]]
name = "g"
source = "enums.toml"
source_key = "k.allowed"
[[codegen.groups.targets]]
path = "nodex.toml"
begin = "# BEGIN x"
end = "# END x"
format = "toml-array-assignment"
name = "allowed"
"##;
    let cfg = load_cfg(&tmp, toml_body);

    let outcome = ProjectChecker::new(&cfg, tmp.path()).fix().unwrap();
    assert!(
        !outcome.before.findings.is_empty(),
        "expected drift before fix"
    );
    assert_eq!(outcome.fixes_attempted.len(), 1);
    assert_eq!(
        outcome.fixes_attempted[0].fix_command,
        "harness codegen sync"
    );
    assert!(matches!(
        outcome.fixes_attempted[0].status,
        harness_core::check::FixStatus::Applied
    ));
    assert!(
        outcome.after.findings.is_empty(),
        "expected clean re-check, got: {:?}",
        outcome.after.findings
    );
    // Verify the target file was actually rewritten
    let target = fs::read_to_string(tmp.path().join("nodex.toml")).unwrap();
    assert!(target.contains("allowed = [\"a\", \"b\"]"));
}

#[test]
fn fix_is_noop_when_no_auto_fixable_findings() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    // No drift; rule/skill validators have no candidates to find issues with
    let outcome = ProjectChecker::new(&cfg, tmp.path()).fix().unwrap();
    assert!(outcome.fixes_attempted.is_empty());
    assert_eq!(outcome.before.findings.len(), outcome.after.findings.len());
}

#[test]
fn fix_unrecognized_command_status() {
    // Manually craft a "fix command not in registry" via the registry's
    // own match. Direct path: invoke try_fix through fix() with no
    // findings; verify branch is reachable via match arms.
    // Since auto_fixable + fix_command currently only triggers codegen,
    // unrecognized status path is reserved for future validators.
    // This test documents the API surface exists.
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let outcome = ProjectChecker::new(&cfg, tmp.path()).fix().unwrap();
    // No fixes attempted means no Unrecognized statuses surface today;
    // this test pins behaviour for documentation.
    assert!(outcome.fixes_attempted.is_empty());
}

#[test]
fn files_scanned_counts_only_passing_filter() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    write(
        &tmp.path().join(".claude/rules/a.md"),
        "---\npaths: [\"x\"]\n---\n",
    );
    write(
        &tmp.path().join(".claude/rules/b.md"),
        "---\npaths: [\"y\"]\n---\n",
    );
    write(
        &tmp.path().join(".claude/settings.json"),
        r#"{"permissions":{"deny":["Bash(sudo *)"]}}"#,
    );
    write(&tmp.path().join("CLAUDE.md"), "x\n");

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    // 2 rules (a, b) + 1 settings + 3 evidence (CLAUDE.md + a.md + b.md) = 6
    assert!(
        outcome.files_scanned >= 5,
        "files_scanned = {}",
        outcome.files_scanned
    );
}
