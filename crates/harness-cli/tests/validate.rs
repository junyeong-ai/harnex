use std::process::Command;

use harness_core::path_guard::write_atomic;

#[test]
fn skill_directory_validation_resolves_relative_paths_and_honors_scope() {
    let temp = tempfile::tempdir().unwrap();
    let config = format!(
        "[meta]\nharnex_version = {:?}\n[validate.skills]\n",
        env!("CARGO_PKG_VERSION")
    );
    write_atomic(&temp.path().join("harness.toml"), config.as_bytes()).unwrap();
    for (directory, scope, exit) in [
        ("review", "project", 0),
        ("bad\u{200b}name", "project", 1),
        ("bad\u{200b}name", "user", 1),
        ("bad\u{200b}name", "plugin", 0),
    ] {
        let dir = temp.path().join(".claude/skills").join(directory);
        write_atomic(&dir.join("SKILL.md"), b"---\nname: review\n---\nBody\n").unwrap();
        for path in [
            std::path::PathBuf::from("SKILL.md"),
            "./SKILL.md".into(),
            dir.join("SKILL.md"),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_harnex"))
                .current_dir(&dir)
                .args(["validate", "skills", "--scope", scope])
                .arg(&path)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(exit),
                "{scope} {path:?}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            if exit == 1 {
                assert_eq!(
                    json["data"]["items"][0]["location"]["path"],
                    path.to_string_lossy().as_ref()
                );
            }
        }
    }
}

#[test]
fn always_loaded_takes_the_home_directory_from_the_environment() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let config = format!("[meta]\nharnex_version = {:?}\n", env!("CARGO_PKG_VERSION"));
    for (path, text) in [
        ("app/harness.toml", config.as_str()),
        ("app/CLAUDE.md", "app\n"),
        (
            ".claude/skills/top/SKILL.md",
            "---\ndescription: top\n---\n",
        ),
    ] {
        write_atomic(&root.join(path), text.as_bytes()).unwrap();
    }
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::create_dir(root.join("elsewhere")).unwrap();
    let members = |home: &std::path::Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
            .current_dir(root.join("app"))
            .env("HOME", home)
            .args(["validate", "always-loaded"])
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        json["data"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["path"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    let skill = "../.claude/skills/top/SKILL.md".to_string();
    assert!(members(&root.join("elsewhere")).contains(&skill));
    assert!(
        !members(&root).contains(&skill),
        "a skill in the home directory's `.claude/` is the user's"
    );
}

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
    // A repository of its own, so no directory above the temp dir is read.
    std::fs::create_dir(root.join(".git")).unwrap();
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

/// The scaffold declares no `[validate.commit_msg]`, and the shipped arm reports
/// every exit other than 0 and 1 as a harnex that could not run, so validation
/// that is off has to answer as success or every commit of a fresh scaffold says
/// something false.
#[test]
fn the_commit_msg_arm_is_silent_while_validation_is_off_and_says_when_harnex_cannot_run() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let arm = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/harnex/templates/common/git-hooks/commit-msg");
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_harnex"), bin.join("harnex")).unwrap();
    write_atomic(&root.join("MSG"), b"subject\n").unwrap();
    let pinned = format!("[meta]\nharnex_version = {:?}\n", env!("CARGO_PKG_VERSION"));
    let required =
        format!("{pinned}[[validate.commit_msg.trailers]]\nkey = \"Unit\"\nrequired = true\n");

    for (config, exit, said) in [
        (pinned.clone(), 0, &[][..]),
        (required, 1, &["commit message validation failed"][..]),
        (
            "[meta]\nharnex_version = \">=0.1, <0.2\"\n".to_string(),
            0,
            &["commit message not checked", "CONFIG_VERSION_MISMATCH"][..],
        ),
    ] {
        write_atomic(&root.join("harness.toml"), config.as_bytes()).unwrap();
        let out = Command::new("bash")
            .arg(&arm)
            .arg("MSG")
            .current_dir(root)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env_remove("HARNEX_SKIP_COMMITMSG")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(exit), "{config}: {stderr}");
        assert_eq!(said.is_empty(), stderr.is_empty(), "{config}: {stderr}");
        for phrase in said {
            assert!(stderr.contains(phrase), "{config}: {stderr}");
        }
    }

    write_atomic(&root.join("harness.toml"), pinned.as_bytes()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_harnex"))
        .current_dir(root)
        .args(["validate", "commit-msg", "MSG"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["data"]["items"], serde_json::json!([]));
    assert_eq!(json["warnings"][0]["code"], "commit-msg-validation-off");
}
