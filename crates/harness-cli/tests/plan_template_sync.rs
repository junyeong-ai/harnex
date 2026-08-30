//! The spec-workflow hook template invokes the released binary by spelling
//! its flags: `check-plan.sh` is a hand-written projection of the `plan
//! audit` clap surface, and nothing but this test holds the two together
//! (constitution IX). A renamed flag would otherwise fail open at every
//! scaffolded repo — the hook's `*` arm reads clap's exit 2 as "audit
//! skipped" and lets the commit through with a note.

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
