//! The spec-workflow hook template invokes the released binary by spelling
//! its flags: `check-plan.sh` is a hand-written projection of the `plan
//! audit` clap surface, and nothing but this test holds the two together
//! (constitution IX). A renamed flag would otherwise fail open at every
//! scaffolded repo — the hook's `*` arm reads clap's exit 2 as "audit
//! skipped" and lets the commit through with a note.

use std::path::Path;
use std::process::Command;

#[test]
fn every_flag_the_hook_template_passes_is_a_plan_audit_flag() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/harnex/templates/patterns/spec-workflow/check-plan.sh"
    );
    let script = std::fs::read_to_string(path).expect("check-plan.sh template exists");
    assert!(
        script.contains("harnex plan audit"),
        "the template must invoke `harnex plan audit`"
    );

    let mut flags: Vec<String> = Vec::new();
    for line in script.lines() {
        let Some(at) = line.find("args=(").or_else(|| line.find("args+=(")) else {
            continue;
        };
        for token in line[at..].split(|c: char| c.is_whitespace() || c == '(' || c == ')') {
            if let Some(flag) = token.strip_prefix("--") {
                flags.push(flag.to_string());
            }
        }
    }
    assert!(!flags.is_empty(), "the template builds args=(--…) arrays");

    let help = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .args(["plan", "audit", "--help"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0), "plan audit --help must exist");
    let help = String::from_utf8(help.stdout).unwrap();
    for flag in flags {
        assert!(
            help.contains(&format!("--{flag}")),
            "check-plan.sh passes --{flag}, which `plan audit --help` does not list"
        );
    }
}

/// The arm skips a gate harnex could not run, and blocks the commit when harnex
/// refused an argument the arm passed: that is the arm and the binary
/// disagreeing, and skipping there would end the floor in silence on every
/// commit after an upgrade.
#[test]
fn the_arm_blocks_on_an_argument_harnex_refuses_and_skips_a_failure_to_run() {
    let arm = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/patterns/spec-workflow/check-plan.sh");
    for (envelope, expected) in [
        (
            r#"{"ok":false,"error":{"code":"CONFIG_INVALID","message":"invalid arguments"}}"#,
            1,
        ),
        (
            r#"{"ok":false,"error":{"code":"IO_FAILURE","message":"unreadable"}}"#,
            0,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q", "."]);
        std::fs::create_dir_all(dir.path().join("specs/t")).unwrap();
        std::fs::write(dir.path().join("specs/t/plan.md"), "# t\n").unwrap();
        git(&["add", "specs/t/plan.md"]);
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("harnex");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\nprintf '%s\\n' '{envelope}'\nexit 2\n"),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let out = Command::new("bash")
            .arg(&arm)
            .current_dir(dir.path())
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(expected), "{envelope}: {out:?}");
    }
}
