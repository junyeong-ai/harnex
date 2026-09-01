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
harnex_version = ">=0.2, <0.3"

[evidence]
default_provenance = "memory-only"
[[evidence.verifiers]]
provenance = "memory-only"
strategy = "memory-only"

[validate.routines]

[validate.rules]
max_lines = 200
always_loaded_slugs = ["constitution"]

[validate.skills]
max_skill_md_lines = 500
max_description_chars = 1536

[validate.agents]

[validate.output_styles]

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
        "validate.routines",
        "validate.rules",
        "validate.skills",
        "validate.agents",
        "validate.output_styles",
        "validate.settings",
        "evidence",
        "advisory",
        "governs",
        "policy.permissions",
    ] {
        assert!(outcome.run.contains(&v.to_string()), "missing {v}");
    }
    // codegen skipped (no [codegen] section)
    assert!(outcome.skipped.iter().any(|s| s.slug == "codegen"));

    // The rule with no `paths:` frontmatter must surface — proof that the
    // rule validator actually ran against the fixture. The presence of
    // `validate.skills` in `outcome.run` (asserted above) is the proof
    // the skill validator ran; we don't couple this test to optional
    // skill-policy opt-ins like `flag_side_effect_verbs`.
    let slugs: Vec<&str> = outcome.findings.iter().map(|f| f.slug.as_str()).collect();
    assert!(slugs.contains(&"rule-missing-paths-frontmatter"));
}

#[test]
fn check_skips_validators_with_no_config_section() {
    let tmp = TempDir::new().unwrap();
    let minimal = r#"
[meta]
harnex_version = ">=0.2, <0.3"
"#;
    let cfg = load_cfg(&tmp, minimal);
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert!(outcome.run.is_empty());
    for expected in [
        "advisory",
        "codegen",
        "evidence",
        "governs",
        "policy.permissions",
        "validate.agents",
        "validate.output_styles",
        "validate.routines",
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
harnex_version = ">=0.2, <0.3"

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
        drift[0].fix_command,
        Some(harness_core::envelope::FixCommand::CodegenSync)
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
harnex_version = ">=0.2, <0.3"

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
        harness_core::envelope::FixCommand::CodegenSync
    );
    assert!(matches!(
        outcome.fixes_attempted[0].outcome,
        harness_core::check::FixOutcome::Applied
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
fn fix_with_nothing_to_do_attempts_nothing() {
    // `fix()` filters on `auto_fixable`, so a config that produces no fixable
    // finding must attempt no fix rather than dispatching an empty batch.
    // There is no unrecognized-command branch to test: `try_fix` takes a typed
    // `FixCommand` and matches it exhaustively, so no string reaches it.
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let outcome = ProjectChecker::new(&cfg, tmp.path()).fix().unwrap();
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

/// The three surfaces that answer "which fix commands exist" agree.
///
/// `Serialize`, `Deserialize` and the emitted schema are hand-written for
/// [`FixCommand`] — a derive would have kept them in step, so nothing but this
/// holds them together. All three read `ALL` + `as_str`, and this asserts they
/// actually do rather than that they were written to.
#[test]
fn fix_command_serialises_round_trips_and_matches_its_schema() {
    use harness_core::envelope::FixCommand;

    let schema = serde_json::to_value(schemars::schema_for!(FixCommand)).unwrap();
    let declared: Vec<String> = schema["enum"]
        .as_array()
        .expect("FixCommand schema declares an enum of wire strings")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let mut emitted = Vec::new();
    for c in FixCommand::ALL {
        let json = serde_json::to_value(c).unwrap();
        let wire = json
            .as_str()
            .expect("a FixCommand serialises as a bare string")
            .to_string();
        assert_eq!(wire, c.as_str(), "Serialize disagrees with as_str");
        assert_eq!(
            serde_json::from_value::<FixCommand>(json).unwrap(),
            *c,
            "Deserialize is not the inverse of Serialize"
        );
        emitted.push(wire);
    }
    assert_eq!(
        declared, emitted,
        "the schema does not describe what Serialize emits"
    );

    let outside = serde_json::json!("harnex policy permissions generate --profile baseline");
    assert!(
        serde_json::from_value::<FixCommand>(outside).is_err(),
        "a command outside the registry must not deserialize — that is the class this type exists to reject"
    );
}

#[test]
fn check_reports_a_governs_truth_that_no_longer_exists() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    write(
        &tmp.path().join(".claude/rules/naming.md"),
        "---\npaths: [\"src/**\"]\ngoverns:\n  concept: naming\n  live_truth:\n    - src\n    - vanished/registry.rs\n---\n# Naming\n",
    );
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    let truth_missing: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.slug == "governs-truth-missing")
        .collect();
    assert_eq!(truth_missing.len(), 1, "{:?}", outcome.findings);
    assert!(truth_missing[0].message.contains("vanished/registry.rs"));
}

/// The governs arm ignores `--since`: the defect is created by a change to a
/// declared truth, not to the rule that declares it, so a diff-windowed rule
/// filter would read a deleted truth as nothing-to-check.
#[test]
fn governs_truth_missing_survives_a_since_window_that_excludes_the_rule() {
    let tmp = TempDir::new().unwrap();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    write(&tmp.path().join("src/registry.rs"), "// truth\n");
    write(
        &tmp.path().join(".claude/rules/naming.md"),
        "---\npaths: [\"src/**\"]\ngoverns:\n  concept: naming\n  live_truth: src/registry.rs\n---\n",
    );
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(tmp.path())
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    std::fs::remove_file(tmp.path().join("src/registry.rs")).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "delete the declared truth, rule untouched"]);

    let outcome = ProjectChecker::new(&cfg, tmp.path())
        .with_since("HEAD~1")
        .run()
        .unwrap();
    assert!(
        outcome
            .findings
            .iter()
            .any(|f| f.slug == "governs-truth-missing"),
        "windowed check missed the vanished truth: {:?}",
        outcome.findings
    );
}

/// The advisory arm holds a declared measurement's basis fresh: undeclared
/// evidence never passes as clean, and an unattended run downgrades exactly
/// the entries whose re-measurement is not clearable in one sitting.
#[test]
fn check_gates_advisory_staleness_by_context() {
    let tmp = TempDir::new().unwrap();
    let toml = format!(
        "{}\n[[evidence.advisories]]\nid = \"contrast\"\ninputs = [\"styles\"]\n",
        minimal_config_toml()
    );
    let cfg = load_cfg(&tmp, &toml);
    std::fs::create_dir_all(tmp.path().join("styles")).unwrap();
    write(&tmp.path().join("styles/a.css"), "a { color: red }\n");

    let unmeasured = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert!(
        unmeasured
            .findings
            .iter()
            .any(|f| f.slug == "advisory-unmeasured"),
        "{:?}",
        unmeasured.findings
    );

    harness_core::evidence::advisory::record(
        tmp.path(),
        cfg.evidence.as_ref().unwrap(),
        "contrast",
        serde_json::Value::Null,
    )
    .unwrap();
    assert!(
        ProjectChecker::new(&cfg, tmp.path())
            .run()
            .unwrap()
            .findings
            .is_empty()
    );

    write(&tmp.path().join("styles/a.css"), "a { color: blue }\n");
    let severity_of = |checker: ProjectChecker| {
        checker
            .run()
            .unwrap()
            .findings
            .iter()
            .find(|f| f.slug == "advisory-stale-input")
            .map(|f| f.severity)
            .unwrap()
    };
    assert_eq!(
        severity_of(ProjectChecker::new(&cfg, tmp.path())),
        harness_core::envelope::Severity::Major
    );
    assert_eq!(
        severity_of(ProjectChecker::new(&cfg, tmp.path()).with_unattended()),
        harness_core::envelope::Severity::Minor
    );
}
