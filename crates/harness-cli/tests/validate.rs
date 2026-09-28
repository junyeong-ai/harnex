use std::process::Command;

use harness_core::path_guard::write_atomic;

#[test]
fn always_loaded_measures_from_the_config_root_and_gates_only_on_a_declared_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let config = |budget: &str| {
        format!(
            "[meta]\nharnex_version = {:?}\n{budget}",
            env!("CARGO_PKG_VERSION")
        )
    };
    for (path, text) in [
        ("harness.toml", config("")),
        ("CLAUDE.md", "twelve chars\n".into()),
        ("src/lib.rs", String::new()),
    ] {
        write_atomic(&root.join(path), text.as_bytes()).unwrap();
    }
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
            .current_dir(root.join("src"))
            .args(["validate", "always-loaded"])
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code(), json)
    };

    let (exit, json) = run();
    assert_eq!(exit, Some(0));
    assert_eq!(json["data"]["total_chars"], 12);
    assert_eq!(json["data"]["members"][0]["path"], "CLAUDE.md");
    assert_eq!(json["data"]["max_chars"], serde_json::Value::Null);
    assert_eq!(json["data"]["findings"], serde_json::json!([]));

    write_atomic(
        &root.join("harness.toml"),
        config("[validate.always_loaded]\nmax_chars = 10\n").as_bytes(),
    )
    .unwrap();
    let (exit, json) = run();
    assert_eq!(exit, Some(1));
    assert_eq!(json["data"]["max_chars"], 10);
    assert_eq!(
        json["data"]["findings"][0]["slug"],
        "always-loaded-over-budget"
    );
}
