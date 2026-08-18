//! SCRATCH — reviewer probe. Delete after use.
use std::fs;
use std::path::{Path, PathBuf};

use harness_core::audit::ProjectAuditor;
use harness_core::scaffold::{Content, ScaffoldManifest, Tier};
use tempfile::TempDir;

fn plugin_templates() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates")
}

fn merge_at(root: &mut serde_json::Value, key_path: &str, value: serde_json::Value) {
    let mut cursor = root;
    let keys: Vec<&str> = key_path.split('.').collect();
    for key in &keys[..keys.len() - 1] {
        cursor = cursor
            .as_object_mut()
            .unwrap()
            .entry((*key).to_string())
            .or_insert_with(|| serde_json::json!({}));
    }
    let last = keys[keys.len() - 1].to_string();
    let slot = cursor
        .as_object_mut()
        .unwrap()
        .entry(last)
        .or_insert(serde_json::Value::Null);
    match (&mut *slot, value) {
        (serde_json::Value::Object(existing), serde_json::Value::Object(incoming)) => {
            for (k, v) in incoming {
                existing.insert(k, v);
            }
        }
        (serde_json::Value::Array(existing), serde_json::Value::Array(incoming)) => {
            existing.extend(incoming);
            existing.sort_by_key(serde_json::Value::to_string);
            existing.dedup();
        }
        (_, incoming) => *slot = incoming,
    }
}

fn emit_tier(
    manifest: &ScaffoldManifest,
    tier: Tier,
    templates: &Path,
    lang: Option<&str>,
    proj_root: &Path,
    settings: &mut serde_json::Value,
) {
    for artifact in manifest.tier(tier) {
        let (Some(template), Some(destination)) =
            (artifact.template_for(lang), artifact.destination_for(lang))
        else {
            panic!("needs lang");
        };
        let src = templates.join(&template);
        let dst = proj_root.join(&destination);
        match &artifact.content {
            Content::Merge { key } => {
                let fragment: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&src).unwrap()).unwrap();
                merge_at(settings, key, fragment);
            }
            Content::Copy | Content::Seed | Content::Managed if dst.exists() => {}
            Content::Copy | Content::Seed | Content::Managed => {
                fs::create_dir_all(dst.parent().unwrap()).unwrap();
                fs::copy(&src, &dst).unwrap();
            }
        }
    }
}

/// PROBE 1: an incumbent `hooks/_runner.sh` is kept, but the merge artifacts
/// still wire every hook at it.
#[test]
fn probe_incumbent_runner_gets_wired() {
    let templates = plugin_templates();
    let manifest = ScaffoldManifest::load(&templates).unwrap();
    let project = TempDir::new().unwrap();
    let root = project.path();

    let squatter = "#!/bin/sh\n# somebody else's runner\nexec ./scripts/other \"$@\"\n";
    fs::create_dir_all(root.join("hooks")).unwrap();
    fs::write(root.join("hooks/_runner.sh"), squatter).unwrap();

    let mut settings = serde_json::json!({});
    emit_tier(
        &manifest,
        Tier::Foundation,
        &templates,
        None,
        root,
        &mut settings,
    );
    emit_tier(
        &manifest,
        Tier::Language,
        &templates,
        Some("rust"),
        root,
        &mut settings,
    );

    assert_eq!(
        fs::read_to_string(root.join("hooks/_runner.sh")).unwrap(),
        squatter,
        "incumbent preserved (expected)"
    );
    let s = serde_json::to_string_pretty(&settings).unwrap();
    println!("--- settings hooks ---\n{s}");
    assert!(s.contains("hooks/_runner.sh"), "settings still wires it");

    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(root.join(".claude/settings.json"), &s).unwrap();
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let out = ProjectAuditor::new(root)
        .with_plugin_root(plugin_root)
        .run()
        .unwrap();
    println!("--- findings ---\n{:#?}", out.findings);
    println!(
        "--- coverage absent ---\n{:?}",
        out.coverage
            .iter()
            .filter(|c| !c.present)
            .map(|c| &c.destination)
            .collect::<Vec<_>>()
    );
    println!(
        "--- coverage present ---\n{:?}",
        out.coverage
            .iter()
            .filter(|c| c.present)
            .map(|c| &c.destination)
            .collect::<Vec<_>>()
    );
}

/// PROBE 2: a project CLAUDE.md that already contains harnex sentinels from an
/// unrelated/older slug -> what does the auditor say after a collision-skip?
#[test]
fn probe_incumbent_claude_md_with_sentinels() {
    let templates = plugin_templates();
    let manifest = ScaffoldManifest::load(&templates).unwrap();
    let project = TempDir::new().unwrap();
    let root = project.path();

    fs::write(
        root.join("CLAUDE.md"),
        "# acme\n<!-- harnex-managed:start something-else -->\nx\n<!-- harnex-managed:end something-else -->\n",
    )
    .unwrap();

    let mut settings = serde_json::json!({});
    emit_tier(
        &manifest,
        Tier::Foundation,
        &templates,
        None,
        root,
        &mut settings,
    );
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(
        root.join(".claude/settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let out = ProjectAuditor::new(root)
        .with_plugin_root(plugin_root)
        .run()
        .unwrap();
    println!("--- findings ---\n{:#?}", out.findings);
}

/// PROBE 3: coverage on a totally empty project that merely HAS the
/// destinations as pre-existing project files.
#[test]
fn probe_coverage_counts_project_files_as_harnex_coverage() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    for rel in [
        "CLAUDE.md",
        ".claude/rules/constitution.md",
        ".claude/rules/governance.md",
        "hooks/_runner.sh",
        "hooks/pre-commit",
        "harness.toml",
    ] {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "project's own\n").unwrap();
    }
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex");
    let out = ProjectAuditor::new(root)
        .with_plugin_root(plugin_root)
        .run()
        .unwrap();
    println!(
        "--- present ---\n{:?}",
        out.coverage
            .iter()
            .filter(|c| c.present)
            .map(|c| &c.destination)
            .collect::<Vec<_>>()
    );
    println!("--- findings ---\n{:#?}", out.findings);
}

/// PROBE 4: one typo'd start sentinel (missing space before `-->`) aborts
/// parsing of the whole document.
#[test]
fn probe_typo_sentinel_swallows_the_document() {
    let plugin = TempDir::new().unwrap();
    let proj = TempDir::new().unwrap();
    for (rel, body) in [
        (
            "templates/common/CLAUDE.md",
            "<!-- harnex-managed:start enforcement-summary -->\ncanonical\n<!-- harnex-managed:end enforcement-summary -->\n",
        ),
        (
            "templates/scaffold.toml",
            "[[artifact]]\ntier = \"foundation\"\ntemplate = \"common/CLAUDE.md\"\ndestination = \"CLAUDE.md\"\ncontent = { kind = \"managed\" }\n",
        ),
    ] {
        let p = plugin.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    // A note the operator typed above the region, with the marker mis-spelled
    // (no space before `-->`). The region BELOW it is perfectly well-formed.
    let body = "# proj\n\
        <!-- harnex-managed:start notes-->\n\
        my note\n\
        <!-- harnex-managed:end notes -->\n\
        <!-- harnex-managed:start enforcement-summary -->\n\
        canonical\n\
        <!-- harnex-managed:end enforcement-summary -->\n";
    fs::write(proj.path().join("CLAUDE.md"), body).unwrap();

    println!(
        "extract_regions -> {:#?}",
        harness_core::sentinel::extract_regions(body)
    );
    let out = ProjectAuditor::new(proj.path())
        .with_plugin_root(plugin.path().to_path_buf())
        .run()
        .unwrap();
    println!(
        "findings -> {:#?}",
        out.findings
            .iter()
            .map(|f| (&f.slug, &f.message))
            .collect::<Vec<_>>()
    );
}

/// PROBE 5: multi-line fill marker is silently not a finding.
#[test]
fn probe_multiline_fill_marker_is_invisible() {
    let m = harness_core::sentinel::fill_markers("<!-- harnex-fill: the project\nname -->\n");
    println!("multiline fill markers -> {m:?}");
    let m2 = harness_core::sentinel::fill_markers("<!--harnex-fill: no space -->\n");
    println!("no-space-prefix fill markers -> {m2:?}");
}
