//! Drift guard over how this project publishes itself — the release workflow,
//! the installer, and the manifests that name where both come from.
//!
//! `release.yml` decides which targets become release assets and what those
//! assets are called; `install.sh` decides which asset a machine asks for.
//! That is one fact in two files, and a disagreement is silent — the workflow
//! keeps publishing, the installer keeps getting 404s, and every user on the
//! dropped platform loses the path that needs no Rust toolchain. Both
//! directions, because dropping a target and adding one fail differently.
//! Constitution IX.

use std::collections::BTreeSet;
use std::path::PathBuf;

const WORKFLOW: &str = ".github/workflows/release.yml";
const INSTALLER: &str = "scripts/install.sh";

/// Documents that hand a reader a command to fetch the installer. A slug that
/// rots here points a `curl | bash` at a repository this project does not own.
const INSTALL_URL_DOCS: &[&str] = &["README.md", "plugins/harnex/commands/measure.md"];

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The targets the release matrix builds an archive for.
fn released_targets(workflow: &str) -> BTreeSet<String> {
    workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- { target: "))
        .filter_map(|rest| rest.split(',').next())
        .map(|target| target.trim().to_string())
        .collect()
}

/// The targets `host_target` can name, as the cross product of the operating
/// systems and the architectures it maps a `uname` answer onto.
fn requested_targets(installer: &str) -> BTreeSet<String> {
    let body = installer
        .split_once("host_target() {")
        .expect("install.sh defines host_target")
        .1
        .split_once("\n}")
        .expect("host_target closes at column zero")
        .0;

    let mapped = |key: &str| -> Vec<String> {
        let assignment = format!("{key}=");
        body.lines()
            .filter_map(|line| line.split_once(&assignment))
            .filter_map(|(_, value)| value.split_whitespace().next())
            .map(str::to_string)
            .collect()
    };

    let systems = mapped("os");
    let architectures = mapped("arch");
    assert!(
        !systems.is_empty() && !architectures.is_empty(),
        "host_target maps no {} — the parse is wrong, not the script",
        if systems.is_empty() { "os" } else { "arch" }
    );

    architectures
        .iter()
        .flat_map(|arch| systems.iter().map(move |os| format!("{arch}-{os}")))
        .collect()
}

/// The value of a `key = "value"` line, in the first section starting at
/// `after`.
fn manifest_value(manifest: &str, after: &str, key: &str) -> String {
    let prefix = format!("{key} = \"");
    manifest
        .split_once(after)
        .unwrap_or_else(|| panic!("no {after} section"))
        .1
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{after} declares no {key}"))
        .to_string()
}

/// Whether the workflow sets a key on the upload step at all — used to hold
/// the action's defaults, which are what the installer's asset names assume.
fn workflow_sets(workflow: &str, key: &str) -> bool {
    let assignment = format!("{key}:");
    workflow
        .lines()
        .any(|line| line.trim().starts_with(&assignment))
}

#[test]
fn the_installer_asks_for_exactly_the_targets_the_release_builds() {
    let released = released_targets(&repo_file(WORKFLOW));
    let requested = requested_targets(&repo_file(INSTALLER));

    assert!(!released.is_empty(), "the release matrix builds nothing");
    assert_eq!(
        released, requested,
        "{WORKFLOW} and {INSTALLER} disagree about which platforms have a binary"
    );
}

#[test]
fn the_binary_is_one_name_in_the_manifest_the_workflow_and_the_installer() {
    let declared = manifest_value(
        &repo_file("crates/harness-cli/Cargo.toml"),
        "[[bin]]",
        "name",
    );

    assert!(
        repo_file(WORKFLOW)
            .lines()
            .any(|line| line.trim() == format!("bin: {declared}")),
        "{WORKFLOW} builds a binary the manifest does not declare as {declared}"
    );
    assert!(
        repo_file(INSTALLER).contains(&format!("readonly BINARY=\"{declared}\"")),
        "{INSTALLER} installs a binary the manifest does not declare as {declared}"
    );
}

#[test]
fn the_installer_and_the_manifest_name_one_repository() {
    let declared = manifest_value(
        &repo_file("Cargo.toml"),
        "[workspace.package]",
        "repository",
    );

    let installer = repo_file(INSTALLER);
    let slug = installer
        .lines()
        .find_map(|line| line.trim().strip_prefix("readonly REPO=\""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("install.sh declares REPO");

    assert_eq!(
        declared,
        format!("https://github.com/{slug}"),
        "{INSTALLER} downloads from a repository the manifest does not claim"
    );
}

#[test]
fn every_shipped_install_command_fetches_the_declared_repository() {
    let declared = manifest_value(
        &repo_file("Cargo.toml"),
        "[workspace.package]",
        "repository",
    );
    let expected = format!("{declared}/raw/main/scripts/install.sh");

    for doc in INSTALL_URL_DOCS {
        let body = repo_file(doc);
        let urls: Vec<String> = body
            .split_whitespace()
            .filter(|word| word.contains("://") && word.contains("scripts/install.sh"))
            .map(|word| word.trim_matches('`').to_string())
            .collect();

        assert!(
            !urls.is_empty(),
            "{doc} hands the reader no install command — drop it from INSTALL_URL_DOCS"
        );
        for url in urls {
            assert_eq!(url, expected, "{doc} fetches the installer from elsewhere");
        }
    }
}

#[test]
fn every_manifest_that_publishes_this_project_names_one_owner() {
    let declared = manifest_value(
        &repo_file("Cargo.toml"),
        "[workspace.package]",
        "repository",
    );
    let owner = declared
        .rsplit('/')
        .nth(1)
        .expect("the repository URL carries an owner segment");

    let json = |path: &str| -> serde_json::Value {
        serde_json::from_str(&repo_file(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    };

    let marketplace = json(".claude-plugin/marketplace.json");
    assert_eq!(
        marketplace["owner"]["name"], owner,
        "the marketplace names an owner the workspace manifest does not"
    );

    let plugin = json("plugins/harnex/.claude-plugin/plugin.json");
    assert_eq!(
        plugin["author"]["name"], owner,
        "the plugin names an author the workspace manifest does not"
    );
    assert_eq!(
        plugin["repository"], declared,
        "the plugin points at a repository the workspace manifest does not"
    );
}

#[test]
fn the_upload_step_keeps_the_defaults_the_installer_reads_asset_names_from() {
    let workflow = repo_file(WORKFLOW);

    // `$bin-$target.sha256` is the file the installer verifies against, and it
    // exists only because this is set.
    assert!(
        workflow_sets(&workflow, "checksum"),
        "{WORKFLOW} uploads no checksum, so {INSTALLER} has nothing to verify"
    );

    // Each of these would rename or restructure the archive the installer
    // fetches by name and extracts a bare binary from.
    for key in ["archive", "tar", "leading-dir", "bin-leading-dir"] {
        assert!(
            !workflow_sets(&workflow, key),
            "{WORKFLOW} sets `{key}`, which moves the asset {INSTALLER} asks for"
        );
    }
}

/// The command surface the README prints is the one a reader copies from, and
/// a target it omits is a capability nobody finds.
#[test]
fn the_readme_names_every_schema_the_binary_will_emit() {
    let readme = repo_file("README.md");
    let listed: BTreeSet<&str> = readme
        .split("harnex export schema {")
        .nth(1)
        .expect("README prints the export surface")
        .split('}')
        .next()
        .expect("the brace closes")
        .split('|')
        .map(str::trim)
        .collect();

    for target in harness_core::export::SchemaTarget::ALL {
        assert!(
            listed.contains(target.as_str()),
            "README omits `{}` from the schemas the binary emits",
            target.as_str()
        );
    }
}
