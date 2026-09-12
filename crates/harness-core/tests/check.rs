//! Integration tests for the unified `check` gate.

use std::fs;
use std::path::Path;

use harness_core::check::ProjectChecker;
use harness_core::config::Config;
use tempfile::TempDir;

mod common;

fn write(p: &Path, contents: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, contents).unwrap();
}

/// A project the gate can run over: a harness is a git repository by
/// construction, and the evidence arm asks git which `CLAUDE.md` files are
/// the project's own.
fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    let status = common::git(dir.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success(), "git init");
    dir
}

fn minimal_config_toml() -> String {
    r#"
[meta]
harnex_version = ">=0.16, <0.17"

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
    let tmp = project();
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
    let tmp = project();
    let minimal = r#"
[meta]
harnex_version = ">=0.16, <0.17"
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
    let tmp = project();
    let src = tmp.path().join("enums.toml");
    fs::write(&src, "[k]\nallowed = [\"a\", \"b\"]\n").unwrap();
    let target = tmp.path().join("nodex.toml");
    fs::write(&target, "# BEGIN x\nallowed = [\"stale\"]\n# END x\n").unwrap();

    let toml_body = r##"
[meta]
harnex_version = ">=0.16, <0.17"

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
    let tmp = project();
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
    let tmp = project();
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
    let tmp = project();
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
    let tmp = project();
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
harnex_version = ">=0.16, <0.17"

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
    let tmp = project();
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
    let tmp = project();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let outcome = ProjectChecker::new(&cfg, tmp.path()).fix().unwrap();
    assert!(outcome.fixes_attempted.is_empty());
}

#[test]
fn files_scanned_counts_only_passing_filter() {
    let tmp = project();
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
    let tmp = project();
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
    let tmp = project();
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
    let tmp = project();
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

fn evidence_config() -> &'static str {
    r#"
[meta]
harnex_version = ">=0.16, <0.17"

[evidence]
default_provenance = "internal"
[[evidence.verifiers]]
provenance = "internal"
strategy = "file-path-line"

[validate.rules]
max_lines = 200
[validate.skills]
[validate.agents]
[validate.output_styles]
[validate.routines]
"#
}

/// The path each `evidence-internal` finding names, after `no/such/`.
fn cited(outcome: &harness_core::check::CheckOutcome) -> Vec<String> {
    let mut cited: Vec<String> = outcome
        .findings
        .iter()
        .filter(|f| f.slug == "evidence-internal")
        .filter_map(|f| {
            f.message
                .split('\'')
                .nth(1)
                .map(|path| path.trim_start_matches("no/such/").to_string())
        })
        .collect();
    cited.sort();
    cited
}

#[test]
fn check_reads_a_claim_from_every_shape_validated_surface() {
    // Each surface carries a claim into a file that does not exist, so the
    // finding names the surface that was read. A surface `run` validates for
    // shape and this list does not read is the silence this test exists for:
    // add a validator, and its glob has to appear in `run_evidence` too.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(
        &tmp.path().join("CLAUDE.md"),
        "Owner: [file: no/such/root.rs:1].\n",
    );
    write(
        &tmp.path().join("crates/x/CLAUDE.md"),
        "Owner: [file: no/such/nested.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/rules/r.md"),
        "---\npaths: [\"src/**\"]\n---\n\nOwner: [file: no/such/rule.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/skills/s/SKILL.md"),
        "---\nname: s\ndescription: d\n---\n\nOwner: [file: no/such/skill.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/agents/a.md"),
        "---\nname: a\ndescription: d\n---\n\nOwner: [file: no/such/agent.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/output-styles/o.md"),
        "---\nname: o\ndescription: d\n---\n\nOwner: [file: no/such/style.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/routines/t.md"),
        "---\nname: t\n---\n\nOwner: [file: no/such/routine.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(
        cited(&outcome)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "agent.rs",
            "nested.rs",
            "root.rs",
            "routine.rs",
            "rule.rs",
            "skill.rs",
            "style.rs"
        ],
        "{:#?}",
        outcome.findings
    );
}

#[test]
fn a_claude_md_the_project_ignores_is_not_its_own_and_one_it_tracks_is() {
    // A vendored package ships a CLAUDE.md whose paths mean nothing here; the
    // project's ignore rules are what say it is not the project's. Committing
    // one anyway makes it the project's, and its claims resolve like any
    // other from the project root — that is the boundary, and it is not a
    // heuristic about directory names.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(&tmp.path().join(".gitignore"), "vendor/\n");
    write(&tmp.path().join("CLAUDE.md"), "root\n");
    write(
        &tmp.path().join("vendor/pkg/CLAUDE.md"),
        "Owner: [file: no/such/ignored.rs:1].\n",
    );
    write(
        &tmp.path().join("vendor/kept/CLAUDE.md"),
        "Owner: [file: no/such/tracked.rs:1].\n",
    );
    let status = common::git(tmp.path())
        .args(["add", "-f", "vendor/kept/CLAUDE.md"])
        .status()
        .unwrap();
    assert!(status.success());
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(cited(&outcome), ["tracked.rs"], "{:#?}", outcome.findings);
}

#[test]
fn a_changed_file_named_outside_ascii_is_still_in_the_since_window() {
    // `git diff --name-only` quotes a path outside ASCII as octal escapes
    // unless asked not to, so a changed `한글.md` never equalled the candidate
    // the gate discovered: a windowed run scanned nothing and reported clean.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(&tmp.path().join("CLAUDE.md"), "root\n");
    let rule = tmp.path().join(".claude/rules/한글.md");
    write(&rule, "---\npaths: [\"src/**\"]\n---\n\nbase\n");
    for args in [
        vec!["add", "harness.toml", "CLAUDE.md", ".claude"],
        vec![
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "base",
        ],
    ] {
        assert!(
            common::git(tmp.path())
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    write(
        &rule,
        "---\npaths: [\"src/**\"]\n---\n\nOwner: [file: no/such/korean.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path())
        .with_since("HEAD")
        .run()
        .unwrap();
    assert_eq!(cited(&outcome), ["korean.rs"], "{:#?}", outcome.findings);
}

#[test]
fn personal_git_excludes_do_not_decide_what_the_project_owns() {
    // A global gitignore listing `CLAUDE.md` is a common habit, and
    // `--exclude-standard` read it: an untracked memory file vanished from
    // the set on one machine and not another, for the same commit. Only the
    // project's own ignore files decide; `.git/info/exclude` stands in for
    // the personal layer here because it is per-clone and not per-project.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(&tmp.path().join(".git/info/exclude"), "CLAUDE.md\n");
    write(
        &tmp.path().join("CLAUDE.md"),
        "Owner: [file: no/such/root.rs:1].\n",
    );
    write(
        &tmp.path().join("crates/x/CLAUDE.md"),
        "Owner: [file: no/such/nested.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(
        cited(&outcome),
        ["nested.rs", "root.rs"],
        "{:#?}",
        outcome.findings
    );
}

#[test]
fn a_config_below_the_git_top_level_still_sees_its_changed_files() {
    // A monorepo keeps `apps/web/harness.toml`. `git diff --name-only` spells
    // a change from the repository root and the candidates are spelled from
    // the config's directory, so a windowed run matched nothing and reported
    // clean over two broken claims.
    let tmp = project();
    let web = tmp.path().join("apps/web");
    write(&web.join("harness.toml"), evidence_config());
    write(&web.join("CLAUDE.md"), "base\n");
    write(
        &web.join(".claude/rules/r.md"),
        "---\npaths: [\"src/**\"]\n---\n\nbase\n",
    );
    for args in [
        vec!["add", "apps"],
        vec![
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "base",
        ],
    ] {
        assert!(
            common::git(tmp.path())
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    write(
        &web.join("CLAUDE.md"),
        "Owner: [file: no/such/root.rs:1].\n",
    );
    write(
        &web.join(".claude/rules/r.md"),
        "---\npaths: [\"src/**\"]\n---\n\nOwner: [file: no/such/rule.rs:1].\n",
    );
    let cfg = Config::load_from(&web.join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, &web)
        .with_since("HEAD")
        .run()
        .unwrap();
    assert_eq!(
        cited(&outcome),
        ["root.rs", "rule.rs"],
        "{:#?}",
        outcome.findings
    );
}

#[test]
fn the_runtimes_own_exclude_list_is_honored() {
    // `claudeMdExcludes` is the runtime's list of memory files it never loads.
    // A tracked example carrying its own example's paths would otherwise be
    // a Blocker about a file the runtime never reads.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(
        &tmp.path().join(".claude/settings.json"),
        "{\"claudeMdExcludes\": [\"examples/**/CLAUDE.md\"]}\n",
    );
    write(
        &tmp.path().join("CLAUDE.md"),
        "Owner: [file: no/such/root.rs:1].\n",
    );
    write(
        &tmp.path().join("examples/demo/CLAUDE.md"),
        "Owner: [file: no/such/example.rs:1].\n",
    );
    write(
        &tmp.path().join("crates/x/CLAUDE.md"),
        "Owner: [file: no/such/nested.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(
        cited(&outcome),
        ["nested.rs", "root.rs"],
        "{:#?}",
        outcome.findings
    );
}

#[test]
fn a_file_in_two_candidate_sources_is_read_once() {
    // `.claude/rules/CLAUDE.md` is a rule and a memory file; both sources
    // listed it, and the same Blocker was emitted twice at the same line.
    let tmp = project();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(&tmp.path().join("CLAUDE.md"), "root\n");
    write(
        &tmp.path().join(".claude/rules/CLAUDE.md"),
        "---\npaths: [\"src/**\"]\n---\n\nOwner: [file: no/such/dup.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(cited(&outcome), ["dup.rs"], "{:#?}", outcome.findings);
    // `files_scanned` counts every validator's pass: the rule validator reads
    // the file once, and evidence reads root and rule once each — four is the
    // count that meant evidence read the rule twice.
    assert_eq!(outcome.files_scanned, 3);
}

#[test]
fn without_git_the_nested_set_is_declared_unmeasured_and_the_rest_is_read() {
    // A tarball export or a container whose checkout git refuses to read still
    // has a root memory file and rules. The nested set is what git answers
    // for, so that set is what is declared unmeasured — and nothing else
    // goes unread because of it.
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("harness.toml"), evidence_config());
    write(
        &tmp.path().join("CLAUDE.md"),
        "Owner: [file: no/such/root.rs:1].\n",
    );
    write(
        &tmp.path().join(".claude/rules/r.md"),
        "---\npaths: [\"src/**\"]\n---\n\nOwner: [file: no/such/rule.rs:1].\n",
    );
    let cfg = Config::load_from(&tmp.path().join("harness.toml")).unwrap();
    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert_eq!(
        cited(&outcome),
        ["root.rs", "rule.rs"],
        "{:#?}",
        outcome.findings
    );
    assert!(
        outcome
            .skipped
            .iter()
            .any(|s| s.slug == "evidence.nested-memory"),
        "the nested set must be declared unmeasured: {:#?}",
        outcome.skipped
    );
}

/// Every glob `check` enumerates, with a path under it. A file the gate
/// discovers and cannot read is a fact about the project, so the run reports
/// it and reaches the arms behind it — the alternative is one bad file
/// deciding what every other arm was allowed to say.
const GLOB_SURFACES: [(&str, &str); 5] = [
    (".claude/rules/**/*.md", ".claude/rules/unreadable.md"),
    (
        ".claude/skills/*/SKILL.md",
        ".claude/skills/unreadable/SKILL.md",
    ),
    (".claude/agents/**/*.md", ".claude/agents/unreadable.md"),
    (
        ".claude/output-styles/*.md",
        ".claude/output-styles/unreadable.md",
    ),
    (".claude/routines/*.md", ".claude/routines/unreadable.md"),
];

/// Not discovered by a glob, and read by two arms rather than one.
const SETTINGS: &str = ".claude/settings.json";

fn write_unreadable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"\xff\xfe\x00not utf-8\n").unwrap();
}

#[test]
fn an_unreadable_file_is_reported_and_the_run_continues_past_it() {
    let tmp = project();
    let cfg = load_cfg(&tmp, &minimal_config_toml());

    // The control: a readable defect behind every unreadable file. Its absence
    // from the findings is what a run that gave up looks like.
    write(
        &tmp.path().join(".claude/rules/api.md"),
        "# Rule without paths frontmatter\n",
    );
    let planted: Vec<std::path::PathBuf> = GLOB_SURFACES
        .iter()
        .map(|(_, path)| *path)
        .chain(std::iter::once(SETTINGS))
        .map(|rel| tmp.path().join(rel))
        .collect();
    for path in &planted {
        write_unreadable(path);
    }

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();

    let slugs: Vec<&str> = outcome.findings.iter().map(|f| f.slug.as_str()).collect();
    assert!(
        slugs.contains(&"rule-missing-paths-frontmatter"),
        "the run stopped at an unreadable file instead of reporting it: {slugs:?}"
    );
    for path in &planted {
        let named = outcome
            .findings
            .iter()
            .filter(|f| f.slug == "file-unreadable" && f.location.path == *path)
            .count();
        assert_eq!(named, 1, "{} reported {named} times", path.display());
    }
    assert_eq!(
        outcome
            .skipped
            .iter()
            .filter(|s| s.slug == "policy.permissions")
            .count(),
        1,
        "an arm that could not read its input says so where a reader looks: {:?}",
        outcome.skipped
    );
}

#[test]
fn the_covered_surfaces_are_every_surface_a_validator_declares() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/validate");
    let mut declared: Vec<String> = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let body = fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in body.lines() {
            if let Some(rest) = line.trim().strip_prefix("const GLOB: &'static str = ")
                && let Some(glob) = rest.trim_end_matches(';').trim().strip_prefix('"')
            {
                declared.push(glob.trim_end_matches('"').to_string());
            }
        }
    }
    declared.sort();
    let mut covered: Vec<String> = GLOB_SURFACES
        .iter()
        .map(|(glob, _)| (*glob).to_string())
        .collect();
    covered.sort();
    assert_eq!(
        declared, covered,
        "a validator declares a surface the unreadable-file case does not plant a file under"
    );
}

#[test]
fn an_unreadable_file_is_reported_where_its_own_validator_is_disabled() {
    // Which arms a project enables is its own choice, and the evidence arm
    // reads the union of the surface globs whatever that choice was. Leaving
    // the file to "the validator that covers it" leaves it to nobody here.
    let tmp = project();
    let cfg = load_cfg(
        &tmp,
        r#"
[meta]
harnex_version = ">=0.16, <0.17"

[evidence]
default_provenance = "memory-only"
[[evidence.verifiers]]
provenance = "memory-only"
strategy = "memory-only"
"#,
    );
    let skill = tmp.path().join(".claude/skills/deploy/SKILL.md");
    write(&skill, "---\nname: deploy\ndescription: d\n---\nbody\n");
    write_unreadable(&skill);

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    assert!(
        outcome
            .skipped
            .iter()
            .any(|s| s.slug == "validate.skills" && s.reason.contains("no [validate.skills]")),
        "this case is only the case while the covering validator is off: {:?}",
        outcome.skipped
    );
    assert_eq!(
        outcome
            .findings
            .iter()
            .filter(|f| f.slug == "file-unreadable" && f.location.path == skill)
            .count(),
        1,
        "an unreadable file reached no reporter: {:?}",
        outcome.findings
    );
}

#[test]
fn a_path_that_is_not_a_regular_file_is_named_rather_than_opened() {
    // What the path names is settled before it is opened, because opening the
    // wrong kind of thing does not fail — reading a fifo waits for a writer
    // and the gate never returns. A directory reaches the same branch without
    // a case that hangs when the branch is gone.
    let tmp = project();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let path = tmp.path().join(".claude/rules/a-directory.md");
    fs::create_dir_all(&path).unwrap();

    let outcome = ProjectChecker::new(&cfg, tmp.path()).run().unwrap();
    let finding = outcome
        .findings
        .iter()
        .find(|f| f.location.path == path)
        .unwrap_or_else(|| panic!("nothing reported: {:?}", outcome.findings));
    assert_eq!(finding.slug, "file-unreadable");
    assert!(
        finding.message.contains("not a regular file"),
        "the kind is what was wrong, and the message says so: {}",
        finding.message
    );
}

/// When a rule loads is declared in two places, and the costly direction is the
/// one a number does not show: a rule the project pinned as always-loaded that
/// scopes itself stops reaching the sessions everything else was written for.
#[test]
fn a_rule_both_pinned_always_loaded_and_scoped_is_a_contradiction() {
    let tmp = project();
    let cfg = load_cfg(&tmp, &minimal_config_toml());
    let rule = tmp.path().join(".claude/rules/constitution.md");

    write(&rule, "---\npaths:\n- src/**\n---\n# Constitution\n");
    let scoped: Vec<String> = ProjectChecker::new(&cfg, tmp.path())
        .run()
        .unwrap()
        .findings
        .iter()
        .map(|f| f.slug.clone())
        .collect();
    assert!(
        scoped.iter().any(|s| s == "rule-always-loaded-but-scoped"),
        "a pinned rule that scoped itself passed: {scoped:?}"
    );

    write(&rule, "# Constitution\n");
    let unscoped: Vec<String> = ProjectChecker::new(&cfg, tmp.path())
        .run()
        .unwrap()
        .findings
        .iter()
        .map(|f| f.slug.clone())
        .collect();
    assert!(
        !unscoped
            .iter()
            .any(|s| s == "rule-always-loaded-but-scoped" || s == "rule-missing-paths-frontmatter"),
        "the pin is what makes an unscoped rule correct: {unscoped:?}"
    );
}
