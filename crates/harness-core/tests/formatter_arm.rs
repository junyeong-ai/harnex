//! The shipped formatters' file boundary and package dispatch. Input decoders
//! and formatter processes are stubs; no language toolchain is installed or run.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use harness_core::scaffold::{Content, ScaffoldManifest, Tier};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn executable(path: &Path, text: &str) {
    write(path, text);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Project {
    _directory: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Project {
    fn new(lang: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let root = base.join("프로젝트 [owner's workspace]");
        let templates =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates");
        let manifest = ScaffoldManifest::load(&templates).unwrap();
        for tier in [Tier::Foundation, Tier::Language] {
            for artifact in manifest.tier(tier).filter(|a| a.content == Content::Copy) {
                let src = templates.join(artifact.template_for(Some(lang)).unwrap());
                let dst = root.join(artifact.destination_for(Some(lang)).unwrap());
                fs::create_dir_all(dst.parent().unwrap()).unwrap();
                fs::copy(src, dst).unwrap();
            }
        }
        let root = root.canonicalize().unwrap();
        let bin = base.join("bin");
        for decoder in ["node", "python3", "jq"] {
            executable(
                &bin.join(decoder),
                "#!/bin/sh\n/bin/cat >/dev/null\nprintf '%s' \"$FORMAT_INPUT_PATH\"\n",
            );
        }
        for formatter in ["biome", "ruff", "rustfmt", "google-java-format", "ktlint"] {
            executable(&bin.join(formatter), Self::formatter());
        }
        let outside = root.with_file_name(format!(
            "{}-other",
            root.file_name().unwrap().to_str().unwrap()
        ));
        fs::create_dir_all(&outside).unwrap();
        Self {
            root,
            outside,
            bin,
            log: base.join("calls"),
            _directory: directory,
        }
    }

    fn formatter() -> &'static str {
        "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$PWD\" \"$@\" >> \"$FORMAT_LOG\"\n"
    }

    fn run(&self, lang: &str, file: &Path) -> String {
        let _ = fs::remove_file(&self.log);
        let output = Command::new("/bin/bash")
            .arg(self.root.join(format!("hooks/post-format-{lang}.sh")))
            .current_dir(&self.root)
            .env("PWD", &self.root)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("FORMAT_INPUT_PATH", file)
            .env("FORMAT_LOG", &self.log)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{lang}: {output:?}");
        assert!(output.stderr.is_empty(), "{lang}: {output:?}");
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

const PROFILES: &[(&str, &str)] = &[
    ("typescript", "ts"),
    ("python", "py"),
    ("rust", "rs"),
    ("jvm", "java"),
    ("jvm", "kt"),
];

#[test]
fn formatters_accept_regular_files_inside_the_project() {
    for &(lang, extension) in PROFILES {
        let project = Project::new(lang);
        write(&project.root.join("biome.json"), "{}");
        let file = project.root.join(format!("src/source..{extension}"));
        write(&file, "source\n");
        fs::create_dir_all(project.root.join("src/nested")).unwrap();
        symlink(project.root.join("src"), project.root.join("alias")).unwrap();
        for path in [
            file.clone(),
            PathBuf::from(format!("src/source..{extension}")),
            project
                .root
                .join(format!("src/nested/../source..{extension}")),
            project.root.join(format!("alias/source..{extension}")),
        ] {
            let calls = project.run(lang, &path);
            assert!(
                calls.contains(file.to_str().unwrap()),
                "{lang}, {path:?}: {calls}"
            );
        }
    }
}

/// The hook runs between two edits of one change, so it may only format: a
/// lint fix applied there acts on code the session has not finished writing.
#[test]
fn formatters_leave_lint_fixes_to_the_gate() {
    for &(lang, extension, required) in &[
        ("typescript", "ts", "--linter-enabled=false"),
        ("python", "py", "format"),
        ("jvm", "java", "--skip-removing-unused-imports"),
    ] {
        let project = Project::new(lang);
        write(&project.root.join("biome.json"), "{}");
        let file = project.root.join(format!("source.{extension}"));
        write(&file, "source\n");
        let calls = project.run(lang, &file);
        assert!(calls.lines().any(|arg| arg == required), "{lang}: {calls}");
        assert!(!calls.lines().any(|arg| arg == "--fix"), "{lang}: {calls}");
    }
}

#[test]
fn formatters_never_dispatch_an_external_file_or_a_symlinked_leaf() {
    for &(lang, extension) in PROFILES {
        let project = Project::new(lang);
        write(&project.root.join("biome.json"), "{}");
        let outside = project.outside.join(format!("source.{extension}"));
        write(&outside, "external\n");
        let inside = project.root.join(format!("source.{extension}"));
        write(&inside, "internal\n");
        symlink(&project.outside, project.root.join("linked")).unwrap();
        let leaf = project.root.join(format!("external.{extension}"));
        symlink(&outside, &leaf).unwrap();
        let local_leaf = project.root.join(format!("internal.{extension}"));
        symlink(&inside, &local_leaf).unwrap();
        for path in [
            outside,
            project.root.join(format!("linked/source.{extension}")),
            leaf,
            local_leaf,
            project.root.join(format!("missing.{extension}")),
            project.root.clone(),
        ] {
            assert!(project.run(lang, &path).is_empty(), "{lang}: {path:?}");
        }
    }
}

#[test]
fn biome_uses_the_edited_packages_configuration_and_binary() {
    let project = Project::new("typescript");
    let package = project.root.join("apps/web");
    let file = package.join("src/source.ts");
    write(&file, "source\n");
    assert!(project.run("typescript", &file).is_empty());
    write(&package.join("biome.json"), "{}");
    let local = package.join("node_modules/.bin/biome");
    executable(&local, Project::formatter());
    let calls = project.run("typescript", &file);
    assert!(
        calls.lines().any(|line| line == local.to_str().unwrap()),
        "{calls}"
    );
    assert!(
        calls.lines().any(|line| line == package.to_str().unwrap()),
        "{calls}"
    );
    assert!(calls.contains(file.to_str().unwrap()), "{calls}");
}

#[test]
fn biome_resolves_pnp_binaries_from_the_root_and_the_edited_workspace() {
    for (root_dependency, workspace_dependency) in [(true, false), (false, true), (true, true)] {
        let project = Project::new("typescript");
        let package = project.root.join("apps/web");
        let file = package.join("source.ts");
        write(&file, "source\n");
        write(&package.join("biome.json"), "{}");
        write(&project.root.join(".pnp.cjs"), "");
        // Yarn exec exposes the selected workspace's binary environment to
        // its child; nested exec keeps inherited binaries behind local ones.
        executable(
            &project.bin.join("yarn"),
            "#!/bin/sh\nif [ \"$1\" = --cwd ]; then cd \"$2\" || exit; shift 2; fi\n\
             [ \"$1\" = exec ] || exit 1\nshift\nPATH=\"$PWD/.yarn-bin:$PATH\" exec \"$@\"\n",
        );
        let root_bin = project.root.join(".yarn-bin/biome");
        let workspace_bin = package.join(".yarn-bin/biome");
        if root_dependency {
            executable(&root_bin, Project::formatter());
        }
        if workspace_dependency {
            executable(&workspace_bin, Project::formatter());
        }
        let expected = if workspace_dependency {
            workspace_bin
        } else {
            root_bin
        };
        let calls = project.run("typescript", &file);
        assert!(
            calls.lines().any(|line| line == expected.to_str().unwrap()),
            "{calls}"
        );
        assert!(
            calls.lines().any(|line| line == package.to_str().unwrap()),
            "{calls}"
        );
    }
}

#[test]
fn formatters_resolve_a_project_root_reached_through_a_symlink() {
    for &(lang, extension) in PROFILES {
        let mut project = Project::new(lang);
        write(&project.root.join("biome.json"), "{}");
        let file = project.root.join(format!("source.{extension}"));
        write(&file, "source\n");
        let alias = project.root.parent().unwrap().join("project-alias");
        symlink(&project.root, &alias).unwrap();
        project.root = alias;
        assert!(
            project.run(lang, &file).contains(file.to_str().unwrap()),
            "{lang}"
        );
    }
}

#[test]
fn formatters_resolve_directory_casing_on_the_host_filesystem() {
    let project = Project::new("rust");
    let file = project.root.join("MixedCase/source.rs");
    write(&file, "source\n");
    let alternate_root = project.root.with_file_name(
        project
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_uppercase(),
    );
    for path in [
        alternate_root.join("MixedCase/source.rs"),
        project.root.join("mixedcase/source.rs"),
    ] {
        let calls = project.run("rust", &path);
        // Case-sensitive volumes have no file at the alternate spelling.
        assert_eq!(!calls.is_empty(), path.is_file(), "{path:?}: {calls}");
        if path.is_file() {
            assert!(calls.contains(file.to_str().unwrap()), "{calls}");
        }
    }
}
