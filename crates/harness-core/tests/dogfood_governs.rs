//! harnex runs the harness it ships (the adopted-scaffold test's charter),
//! and two of this repo's own `governs:` declarations are hand-maintained
//! mirrors of sets the tree owns. Deletion is already guarded — a vanished
//! truth fires `governs-truth-missing` — but an ADDITION escapes governance
//! in silence: a twelfth module directory, or a new workspace crate, would
//! simply not be declared. Constitution IX: the tree is the owner, the
//! declaration is the projection, and this test is the drift guard.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::governs::GovernsDecl;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn live_truth_of(rule: &str) -> BTreeSet<String> {
    let path = repo_root().join(".claude/rules").join(rule);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let declarations =
        GovernsDecl::from_rule(&content, &path).unwrap_or_else(|e| panic!("{rule}: {e}"));
    assert!(!declarations.is_empty(), "{rule} declares no governs");
    declarations
        .into_iter()
        .flat_map(|decl| decl.live_truth)
        .collect()
}

/// `module-doc.md` is truth about every module directory in harness-core.
#[test]
fn module_doc_declares_every_module_directory() {
    let src = repo_root().join("crates/harness-core/src");
    let dirs: BTreeSet<String> = std::fs::read_dir(&src)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            format!(
                "crates/harness-core/src/{}",
                e.file_name().to_string_lossy()
            )
        })
        .collect();
    assert_eq!(
        live_truth_of("module-doc.md"),
        dirs,
        "module-doc.md live_truth drifted from the module directories on disk"
    );
}

/// `jiff-time.md` is truth about every manifest that could pull a time crate.
#[test]
fn jiff_time_declares_every_workspace_manifest() {
    let root = repo_root();
    let mut manifests: BTreeSet<String> = ["Cargo.toml".to_string()].into();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().join("Cargo.toml").is_file() {
            manifests.insert(format!(
                "crates/{}/Cargo.toml",
                entry.file_name().to_string_lossy()
            ));
        }
    }
    assert_eq!(
        live_truth_of("jiff-time.md"),
        manifests,
        "jiff-time.md live_truth drifted from the workspace manifests on disk"
    );
}
