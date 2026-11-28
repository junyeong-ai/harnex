//! `harnex lifecycle` end to end, over the configuration a scaffold ships.

use std::path::Path;
use std::process::Command;

use harness_core::path_guard::write_atomic;

/// The `harness.toml` a scaffold seeds, which declares no `[[kinds]]`.
fn seed_config() -> String {
    let seed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/harness.toml");
    std::fs::read_to_string(&seed).unwrap_or_else(|e| panic!("read {}: {e}", seed.display()))
}

fn retire(root: &Path) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .current_dir(root)
        .args(["lifecycle", "retire"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn warning_codes(envelope: &serde_json::Value) -> Vec<String> {
    envelope["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap().to_string())
        .collect()
}

/// An empty verdict list reads the same whether the sweep walked every
/// artifact or had none to walk, so the second case says so.
#[test]
fn a_sweep_with_no_declared_kind_says_it_walked_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let seed = seed_config();
    assert!(
        !seed.contains("\n[[kinds]]"),
        "the seed now declares a kind, so this no longer tests the scaffold"
    );
    write_atomic(&tmp.path().join("harness.toml"), seed.as_bytes()).unwrap();

    let json = retire(tmp.path());
    assert_eq!(json["data"]["kinds_processed"], serde_json::json!([]));
    assert!(
        warning_codes(&json).contains(&"retire-no-kinds".to_string()),
        "{json}"
    );
    // The curate skill is the reader this warning exists for, and it matches
    // the code as text: a renamed code leaves it waiting for one that never
    // comes.
    let curate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/skills/harness-curate/SKILL.md");
    let curate = std::fs::read_to_string(&curate).unwrap();
    for code in warning_codes(&json) {
        if code.starts_with("retire-") {
            assert!(
                curate.contains(&format!("`{code}`")),
                "the curate skill does not name `{code}`, the warning the sweep emits"
            );
        }
    }

    let declared = format!(
        "{seed}\n[[kinds]]\nname = \"rule\"\nglob = \".claude/rules/*.md\"\nfoundation = false\n"
    );
    write_atomic(&tmp.path().join("harness.toml"), declared.as_bytes()).unwrap();
    let json = retire(tmp.path());
    assert!(
        !warning_codes(&json).contains(&"retire-no-kinds".to_string()),
        "{json}"
    );
}
