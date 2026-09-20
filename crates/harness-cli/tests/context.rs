use std::process::Command;

use harness_core::path_guard::write_atomic;

#[test]
fn context_resolves_from_nested_cwd_and_refuses_incomplete_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (path, text) in [
        (
            "harness.toml",
            format!("[meta]\nharnex_version = {:?}\n", env!("CARGO_PKG_VERSION")),
        ),
        ("CLAUDE.md", "root".into()),
        ("src/CLAUDE.md", "subtree".into()),
        (
            ".claude/rules/code.md",
            "---\npaths: ['src/**']\n---\ncode".into(),
        ),
    ] {
        write_atomic(&root.join(path), text.as_bytes()).unwrap();
    }
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
            .current_dir(root.join("src"))
            .args(args)
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code(), json)
    };
    let (exit, json) = run(&["context", "resolve", "src/new.rs"]);
    assert_eq!(exit, Some(0));
    assert_eq!(json["data"]["targets"], serde_json::json!(["src/new.rs"]));
    assert_eq!(
        json["data"]["required_instructions"],
        serde_json::json!(["CLAUDE.md", ".claude/rules/code.md", "src/CLAUDE.md"])
    );
    let (exit, json) = run(&["context", "resolve"]);
    assert_eq!(exit, Some(0));
    assert_eq!(
        json["data"]["required_instructions"],
        serde_json::json!(["CLAUDE.md"])
    );
    let (exit, json) = run(&["context", "resolve", "../outside.rs"]);
    assert_eq!(exit, Some(2));
    assert_eq!(json["ok"], false);
    write_atomic(
        &root.join(".claude/rules/code.md"),
        b"---\npaths: ['[']\n---\n",
    )
    .unwrap();
    let (exit, json) = run(&["context", "resolve"]);
    assert_eq!(exit, Some(2));
    assert_eq!(json["ok"], false);
    assert!(json.get("data").is_none());
}
