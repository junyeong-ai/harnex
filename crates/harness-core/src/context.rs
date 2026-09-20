//! Resolve repository-owned instructions for explicit file targets.
//!
//! The result names files to read; it neither injects prompts nor interprets
//! prose, imports, user settings, permissions, or workflow state.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::validate::{frontmatter, path_globs, rules};

#[derive(Debug, Serialize)]
pub struct Resolution {
    pub targets: Vec<String>,
    pub required_instructions: Vec<String>,
}

fn invalid(path: &Path, message: impl Into<String>) -> Error {
    Error::ConfigInvalid {
        message: message.into(),
        location: Some(crate::envelope::Location::file(path.to_path_buf())),
    }
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::IoFailure {
        path: path.to_path_buf(),
        source,
    }
}

fn optional_metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn contained(root: &Path, path: &Path) -> Result<PathBuf> {
    let mut existing = path;
    loop {
        match optional_metadata(existing)? {
            Some(_) => {
                let mut real = existing.canonicalize().map_err(|e| io(existing, e))?;
                if !real.starts_with(root) {
                    return Err(invalid(
                        path,
                        "context paths must resolve inside the repository",
                    ));
                }
                real.extend(
                    path.strip_prefix(existing)
                        .expect("ancestor of input")
                        .components(),
                );
                return Ok(real);
            }
            None => {
                existing = existing
                    .parent()
                    .ok_or_else(|| invalid(path, "no existing ancestor"))?;
            }
        }
    }
}

fn query(root: &Path, input: &str) -> Result<(String, String)> {
    let path = Path::new(input);
    if input.is_empty() || path.is_absolute() || input.contains(['\\', '\0']) {
        return Err(invalid(path, "expected a repository-relative file path"));
    }
    crate::path_guard::reject_traversal(path)?;
    let normalized: PathBuf = path
        .components()
        .filter(|c| *c != Component::CurDir)
        .collect();
    if normalized.as_os_str().is_empty() {
        return Err(invalid(
            path,
            "expected a file target, not the repository root",
        ));
    }
    let absolute = root.join(&normalized);
    let real = contained(root, &absolute)?;
    if optional_metadata(&real)?.is_some_and(|meta| !meta.is_file()) {
        return Err(invalid(path, "context targets must be files"));
    }
    let canonical = real
        .strip_prefix(root)
        .expect("contained target")
        .to_str()
        .ok_or_else(|| invalid(path, "instruction target is not UTF-8"))?
        .to_owned();
    Ok((
        normalized
            .to_str()
            .expect("normalized UTF-8 input")
            .to_owned(),
        canonical,
    ))
}

fn memory(root: &Path, relative: &Path, output: &mut Vec<String>) -> Result<()> {
    let path = root.join(relative);
    let real = contained(root, &path)?;
    if let Some(meta) = optional_metadata(&real)? {
        if !meta.is_file() {
            return Err(invalid(&path, "instruction path must be a file"));
        }
        let name = real
            .strip_prefix(root)
            .expect("contained memory")
            .to_str()
            .ok_or_else(|| invalid(&path, "instruction path is not UTF-8"))?
            .to_owned();
        if !output.contains(&name) {
            output.push(name);
        }
    }
    Ok(())
}

pub fn resolve(root: &Path, inputs: &[String]) -> Result<Resolution> {
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    let queries: Vec<(String, String)> = inputs
        .iter()
        .map(|p| query(&root, p))
        .collect::<Result<_>>()?;
    let targets: Vec<String> = queries
        .iter()
        .map(|(logical, _)| logical.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let scope: BTreeSet<&String> = queries
        .iter()
        .flat_map(|(logical, real)| [logical, real])
        .collect();
    let mut required_instructions = Vec::new();
    memory(&root, Path::new("CLAUDE.md"), &mut required_instructions)?;
    memory(
        &root,
        Path::new(".claude/CLAUDE.md"),
        &mut required_instructions,
    )?;
    memory(
        &root,
        Path::new("CLAUDE.local.md"),
        &mut required_instructions,
    )?;

    let mut rule_paths = Vec::new();
    let directory = root.join(rules::RULE_DIRECTORY);
    let rule_directory = contained(&root, &directory)?;
    if let Some(meta) = optional_metadata(&rule_directory)? {
        if !meta.is_dir() {
            return Err(invalid(&directory, "rules directory must be a directory"));
        }
        for entry in walkdir::WalkDir::new(&directory).follow_links(true) {
            let entry =
                entry.map_err(|e| invalid(&directory, format!("rule discovery failed: {e}")))?;
            contained(&root, entry.path())?;
            if entry.file_type().is_file() && entry.file_name().as_encoded_bytes().ends_with(b".md")
            {
                rule_paths.push(entry.into_path());
            }
        }
    }
    rule_paths.sort();
    for path in rule_paths {
        contained(&root, &path)?;
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        let parsed = frontmatter::parse(&text, &path)?
            .map(|fm| {
                yaml_serde::from_str::<rules::RuleFrontmatter>(&fm.yaml_text).map_err(|e| {
                    Error::ValidateFrontmatterMalformed {
                        path: path.clone(),
                        message: e.to_string(),
                    }
                })
            })
            .transpose()?;
        let value = parsed.as_ref().and_then(|fm| fm.paths.as_ref());
        let mut matches = !path_globs::declares_scope(value);
        if let Some(value) = value {
            if !path_globs::is_glob_shaped(value) {
                return Err(invalid(
                    &path,
                    "paths must be a glob string or a list of strings",
                ));
            }
            for pattern in path_globs::globs(value) {
                let matcher = path_globs::compile_glob(&pattern)
                    .map_err(|e| invalid(&path, format!("invalid paths glob: {e}")))?
                    .compile_matcher();
                matches |= scope.iter().any(|target| matcher.is_match(target));
            }
        }
        if matches {
            memory(
                &root,
                path.strip_prefix(&root).expect("rooted rule"),
                &mut required_instructions,
            )?;
        }
    }
    let mut directories = BTreeSet::new();
    for target in scope {
        for parent in Path::new(target).ancestors().skip(1) {
            if !parent.as_os_str().is_empty() {
                let real = contained(&root, &root.join(parent))?;
                directories.insert(
                    real.strip_prefix(&root)
                        .expect("contained ancestor")
                        .to_path_buf(),
                );
            }
        }
    }
    for directory in directories {
        memory(
            &root,
            &directory.join("CLAUDE.md"),
            &mut required_instructions,
        )?;
        memory(
            &root,
            &directory.join("CLAUDE.local.md"),
            &mut required_instructions,
        )?;
    }
    Ok(Resolution {
        targets,
        required_instructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, path: &str, text: &str) {
        crate::path_guard::write_atomic(&root.join(path), text.as_bytes()).unwrap();
    }

    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        for path in [
            "CLAUDE.md",
            ".claude/CLAUDE.md",
            "apps/CLAUDE.md",
            "apps/web/CLAUDE.md",
        ] {
            put(temp.path(), path, "# Memory\n");
        }
        put(
            temp.path(),
            "apps/web/AGENTS.md",
            "# Independent native instructions\n",
        );
        put(temp.path(), ".claude/rules/always.md", "# Always\n");
        put(
            temp.path(),
            ".claude/rules/web/style.md",
            "---\npaths: ['apps/web/**/*.{ts,tsx}']\n---\n",
        );
        put(
            temp.path(),
            ".claude/rules/docs.md",
            "---\npaths: ['*.md']\n---\n",
        );
        temp
    }

    #[test]
    fn resolve_loads_local_memory_only_along_target_ancestors() {
        let temp = fixture();
        for path in [
            "CLAUDE.local.md",
            "apps/CLAUDE.local.md",
            "apps/web/CLAUDE.local.md",
            "other/CLAUDE.local.md",
            "apps/.claude/CLAUDE.md",
        ] {
            put(temp.path(), path, "# Memory\n");
        }
        assert_eq!(
            resolve(temp.path(), &[]).unwrap().required_instructions,
            [
                "CLAUDE.md",
                ".claude/CLAUDE.md",
                "CLAUDE.local.md",
                ".claude/rules/always.md"
            ]
        );
        assert_eq!(
            resolve(temp.path(), &["apps/web/new.ts".into()])
                .unwrap()
                .required_instructions,
            [
                "CLAUDE.md",
                ".claude/CLAUDE.md",
                "CLAUDE.local.md",
                ".claude/rules/always.md",
                ".claude/rules/web/style.md",
                "apps/CLAUDE.md",
                "apps/CLAUDE.local.md",
                "apps/web/CLAUDE.md",
                "apps/web/CLAUDE.local.md"
            ]
        );
    }

    #[test]
    fn rule_discovery_agrees_with_validator_for_markdown_files() {
        use crate::validate::SurfaceValidator;
        let temp = tempfile::tempdir().unwrap();
        for path in [
            ".claude/rules/.md",
            ".claude/rules/.hidden.md",
            ".claude/rules/.hidden/nested.md",
            ".claude/rules/nested/rule.md",
            ".claude/rules/not.MD",
            ".claude/rules/notes.txt",
        ] {
            put(temp.path(), path, "# Rule\n");
        }
        let pattern = crate::glob_root::rooted(temp.path(), rules::RuleValidator::GLOB).unwrap();
        let mut expected: Vec<String> = glob::glob(&pattern)
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .strip_prefix(temp.path())
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        expected.sort();
        let mut actual = resolve(temp.path(), &[]).unwrap().required_instructions;
        // Compare membership: Path orders components, whereas String orders bytes.
        actual.sort();
        assert_eq!(actual, expected);
        assert!(expected.contains(&".claude/rules/.md".into()));
    }

    #[test]
    fn resolve_loads_foundation_without_targets() {
        let temp = fixture();
        let out = resolve(temp.path(), &[]).unwrap();
        assert!(out.targets.is_empty());
        assert_eq!(
            out.required_instructions,
            ["CLAUDE.md", ".claude/CLAUDE.md", ".claude/rules/always.md"]
        );
    }

    #[test]
    fn resolve_includes_ancestors_even_beside_agents_for_future_files() {
        let temp = fixture();
        let out = resolve(
            temp.path(),
            &[
                "./apps/web/new/page.tsx".into(),
                "apps/web/new/page.tsx".into(),
            ],
        )
        .unwrap();
        assert_eq!(out.targets, ["apps/web/new/page.tsx"]);
        assert_eq!(
            out.required_instructions,
            [
                "CLAUDE.md",
                ".claude/CLAUDE.md",
                ".claude/rules/always.md",
                ".claude/rules/web/style.md",
                "apps/CLAUDE.md",
                "apps/web/CLAUDE.md"
            ]
        );
    }

    #[test]
    fn resolve_does_not_apply_root_globs_to_descendants() {
        let temp = fixture();
        for (file, expected) in [
            ("README.md", true),
            ("apps/README.md", false),
            ("readme.MD", false),
        ] {
            let out = resolve(temp.path(), &[file.into()]).unwrap();
            assert_eq!(
                out.required_instructions
                    .contains(&".claude/rules/docs.md".into()),
                expected,
                "{file}"
            );
        }
    }

    #[test]
    fn resolve_rejects_ambiguous_targets() {
        let temp = fixture();
        for path in [
            "",
            ".",
            "..",
            "apps/../README.md",
            "/etc/passwd",
            "apps",
            "apps\\web",
        ] {
            assert!(resolve(temp.path(), &[path.into()]).is_err(), "{path}");
        }
        assert!(
            resolve(
                temp.path(),
                &[temp.path().join("CLAUDE.md").to_str().unwrap().into()]
            )
            .is_err()
        );
        assert!(
            query(
                &temp.path().canonicalize().unwrap(),
                temp.path().join("CLAUDE.md").to_str().unwrap()
            )
            .is_err()
        );
        assert!(resolve(temp.path(), &["CLAUDE.md/child".into()]).is_err());
        assert_eq!(
            resolve(temp.path(), &["CLAUDE.md".into()]).unwrap().targets,
            ["CLAUDE.md"]
        );
    }

    #[test]
    fn resolve_fails_closed_on_every_malformed_rule() {
        let temp = fixture();
        for rule in [
            "---\npaths: [\n---\n",
            "---\npaths: [42]\n---\n",
            "---\npaths: ['[']\n---\n",
            "---\npaths: ['ok']\n",
        ] {
            put(temp.path(), ".claude/rules/bad.md", rule);
            assert!(resolve(temp.path(), &[]).is_err(), "{rule}");
        }
    }

    #[test]
    fn resolve_reads_pathless_and_empty_scope_consistently() {
        let temp = fixture();
        for scope in ["null", "[]", "''", "['']"] {
            put(
                temp.path(),
                ".claude/rules/empty.md",
                &format!("---\npaths: {scope}\n---\n"),
            );
            assert!(
                resolve(temp.path(), &[])
                    .unwrap()
                    .required_instructions
                    .contains(&".claude/rules/empty.md".into())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_checks_each_symlink_boundary() {
        use std::os::unix::fs::symlink;
        let temp = fixture();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), temp.path().join("outside")).unwrap();
        symlink("apps/web", temp.path().join("alias")).unwrap();
        assert!(resolve(temp.path(), &["outside/new.ts".into()]).is_err());
        let aliased = resolve(temp.path(), &["alias/new.ts".into()]).unwrap();
        assert!(
            aliased
                .required_instructions
                .contains(&"apps/web/CLAUDE.md".into())
        );
        assert!(
            aliased
                .required_instructions
                .contains(&".claude/rules/web/style.md".into())
        );
        symlink(
            outside.path().join("missing"),
            temp.path().join(".claude/rules/broken.md"),
        )
        .unwrap();
        assert!(resolve(temp.path(), &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_broken_rule_directories() {
        use std::os::unix::fs::symlink;
        for link in [".claude", ".claude/rules", ".claude/rules/shared"] {
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join(link);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            symlink("missing", &target).unwrap();
            assert!(resolve(temp.path(), &[]).is_err(), "{link}");
        }
    }

    #[test]
    fn containment_preserves_non_directory_errors() {
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), "file", "content");
        let root = temp.path().canonicalize().unwrap();
        assert!(matches!(
            contained(&root, &root.join("file/child")),
            Err(Error::IoFailure { .. })
        ));
    }

    #[test]
    fn resolve_accepts_absent_directives_and_ignores_non_markdown_entries() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            resolve(temp.path(), &[])
                .unwrap()
                .required_instructions
                .is_empty()
        );
        put(temp.path(), ".claude/rules/notes.txt", "---\npaths: [\n");
        std::fs::create_dir_all(temp.path().join(".claude/rules/empty.md")).unwrap();
        assert!(
            resolve(temp.path(), &[])
                .unwrap()
                .required_instructions
                .is_empty()
        );
    }

    #[test]
    fn resolve_rejects_directive_type_mismatches() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("CLAUDE.md")).unwrap();
        assert!(resolve(temp.path(), &[]).is_err());
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), ".claude/rules", "not a directory");
        assert!(resolve(temp.path(), &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_unions_alias_and_real_scopes_without_repeating_memory() {
        use std::os::unix::fs::symlink;
        let temp = fixture();
        symlink("apps/web", temp.path().join("alias")).unwrap();
        put(
            temp.path(),
            ".claude/rules/alias.md",
            "---\npaths: ['alias/**']\n---\n",
        );
        let result = resolve(temp.path(), &["alias/future.ts".into()]).unwrap();
        assert_eq!(result.targets, ["alias/future.ts"]);
        for required in [
            ".claude/rules/alias.md",
            ".claude/rules/web/style.md",
            "apps/CLAUDE.md",
            "apps/web/CLAUDE.md",
        ] {
            assert_eq!(
                result
                    .required_instructions
                    .iter()
                    .filter(|p| p.as_str() == required)
                    .count(),
                1,
                "{required}"
            );
        }
        assert!(
            !result
                .required_instructions
                .contains(&"alias/CLAUDE.md".into())
        );
        assert_eq!(
            result
                .required_instructions
                .iter()
                .filter(|path| path.ends_with("CLAUDE.md"))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "CLAUDE.md",
                ".claude/CLAUDE.md",
                "apps/CLAUDE.md",
                "apps/web/CLAUDE.md"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_deduplicates_instruction_aliases_across_layers() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), ".claude/rules/shared.md", "# Shared\n");
        symlink(".claude/rules/shared.md", temp.path().join("CLAUDE.md")).unwrap();
        symlink("shared.md", temp.path().join(".claude/rules/alias.md")).unwrap();
        assert_eq!(
            resolve(temp.path(), &[]).unwrap().required_instructions,
            [".claude/rules/shared.md"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_rule_directory_cycles() {
        let temp = fixture();
        std::os::unix::fs::symlink(".", temp.path().join(".claude/rules/loop")).unwrap();
        assert!(resolve(temp.path(), &[]).is_err());
    }

    #[test]
    fn resolve_scalar_and_list_patterns_agree_for_character_classes() {
        let temp = fixture();
        for pattern in [
            "docs/[],a].md",
            "docs/[!],a].md",
            "docs/[^],a].md",
            "docs/[\\].md",
        ] {
            put(
                temp.path(),
                ".claude/rules/scalar.md",
                &format!("---\npaths: '{pattern},extra.md'\n---\n"),
            );
            put(
                temp.path(),
                ".claude/rules/list.md",
                &format!("---\npaths: ['{pattern}', 'extra.md']\n---\n"),
            );
            for target in [
                "docs/a.md",
                "docs/].md",
                "docs/b.md",
                "extra.md",
                "other.md",
            ] {
                let result = resolve(temp.path(), &[target.into()]).unwrap();
                assert_eq!(
                    result
                        .required_instructions
                        .contains(&".claude/rules/scalar.md".into()),
                    result
                        .required_instructions
                        .contains(&".claude/rules/list.md".into()),
                    "{pattern}: {target}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_external_instruction_links_but_reads_internal_links() {
        use std::os::unix::fs::symlink;
        for instruction in [
            "CLAUDE.md",
            ".claude/CLAUDE.md",
            ".claude/rules/shared.md",
            "src/CLAUDE.md",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            put(outside.path(), "instruction.md", "# External\n");
            let path = temp.path().join(instruction);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(outside.path().join("instruction.md"), &path).unwrap();
            assert!(
                resolve(temp.path(), &["src/new.rs".into()]).is_err(),
                "{instruction}"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), "shared/instruction.md", "# Shared\n");
        symlink("shared/instruction.md", temp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(
            resolve(temp.path(), &[]).unwrap().required_instructions,
            ["shared/instruction.md"]
        );
    }
}
