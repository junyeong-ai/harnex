//! The operator's break-glass grant, read live from the one file that
//! carries it.
//!
//! `HARNEX_ALLOW_FLOOR_EDIT: "1"` in the `env` block of the **main**
//! checkout's `.claude/settings.local.json` grants protected-path edits for
//! as long as the entry stands. A session environment variable would be a
//! *copy* of the entry rather than a second witness: Claude Code hot-reloads
//! a settings `env` block into the running process, so anything that can
//! write the file can mint the variable too. Reading the file is also what
//! makes revocation immediate — deleting the entry closes a session already
//! running.
//!
//! A grant that cannot be read is no grant. Break-glass fails closed, the
//! opposite direction from the violation checks, whose inability to evaluate
//! allows: there the fail-safe answer is "not proven guilty", here it is
//! "not proven authorised". The two are told apart, though — a file the
//! operator wrote and mis-typed is not the same event as a file they never
//! wrote, and only one of them is fixed by adding the entry again.
//!
//! The grant is read from the main repository's file because that is where
//! Claude Code reads its own: a linked worktree's `.git` is a pointer, and
//! following it to `commondir` is what keeps the hook and the engine looking
//! at one file. A worktree-local copy would otherwise honour a grant the
//! canonical file had already revoked.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// The settings `env` key that grants protected-path edits.
pub const FLOOR_EDIT_GRANT_KEY: &str = "HARNEX_ALLOW_FLOOR_EDIT";

/// Whether the operator's floor-edit override stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorGrant {
    Granted,
    Absent,
    Unreadable { reason: String },
}

pub fn floor_edit_grant(root: &Path) -> FloorGrant {
    let Some(canonical) = canonical_repo_root(root) else {
        return FloorGrant::Unreadable {
            reason: format!(
                "{} points at a git directory this checkout cannot follow — the override \
                 lives in the main checkout, and reading a local one instead would honour \
                 an authority the repository has lost (run `git worktree repair`)",
                root.join(".git").display()
            ),
        };
    };
    let path = canonical.join(".claude").join("settings.local.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return FloorGrant::Absent;
    };
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return FloorGrant::Unreadable {
                reason: format!("{} does not parse: {e}", path.display()),
            };
        }
    };
    match parsed
        .get("env")
        .and_then(|env| env.get(FLOOR_EDIT_GRANT_KEY))
    {
        Some(serde_json::Value::String(v)) if v == "1" => FloorGrant::Granted,
        _ => FloorGrant::Absent,
    }
}

/// The main repository's root, or `None` when a pointer says one exists and
/// this checkout cannot follow it. Inside a linked worktree `.git` is a file
/// naming that worktree's git directory, whose `commondir` leads back to the
/// shared one; a submodule's pointer names a git directory with no
/// `commondir`, and its own root is the answer. Read from the filesystem
/// rather than from `git`, so the check costs no subprocess and cannot be
/// answered by a `git` on the path.
///
/// A pointer that leads nowhere — the main checkout moved and nobody ran
/// `git worktree repair` — is `None` rather than the local root: falling
/// back would read a worktree-local file as though it carried the
/// repository's authority, which is exactly the grant this resolution
/// exists to keep in one place.
pub fn canonical_repo_root(root: &Path) -> Option<PathBuf> {
    let pointer = root.join(".git");
    let Ok(meta) = std::fs::metadata(&pointer) else {
        return Some(root.to_path_buf());
    };
    if meta.is_dir() {
        return Some(root.to_path_buf());
    }
    let Ok(contents) = std::fs::read_to_string(&pointer) else {
        return None;
    };
    let gitdir = contents
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|target| !target.is_empty())?;
    let gitdir = lexical_resolve(root, Path::new(gitdir));
    // A linked worktree's git directory carries a `commondir` naming the
    // shared one; a submodule's carries none and is its own checkout, so its
    // root is the answer. Only when the pointer's own directory sits under
    // the `worktrees/` git writes them into does a missing `commondir` mean
    // a worktree that cannot find its repository, rather than a layout that
    // never had one.
    let commondir = match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(text) => text.trim().to_string(),
        Err(_) => {
            let under_worktrees = gitdir
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "worktrees");
            return if under_worktrees {
                None
            } else {
                Some(root.to_path_buf())
            };
        }
    };
    let common = lexical_resolve(&gitdir, Path::new(&commondir));
    // A bare repository has linked worktrees and no main checkout, so there
    // is no file to read an override from — and its directory's parent is an
    // ordinary directory whose settings would then be read as this
    // repository's authority. git records the fact in the repository's own
    // config rather than in its name, which a repository named `.git` would
    // otherwise satisfy.
    match git_config_boolean(&common, &gitdir, "core", "bare") {
        Some(false) => common.parent().map(Path::to_path_buf),
        _ => None,
    }
}

/// Resolve `target` against `base` without touching the filesystem: an
/// absolute target stands alone, a relative one joins, and `.` / `..`
/// segments collapse lexically. Symlinks are deliberately not followed —
/// this layer is a tripwire, and following links would let the answer
/// depend on state an edit is about to change.
pub fn lexical_resolve(base: &Path, target: &Path) -> PathBuf {
    let joined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    };
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    resolved.push(Component::RootDir);
                }
            }
            other => resolved.push(other),
        }
    }
    resolved
}

/// A git config value read the way git reads it: section-aware, so a `bare`
/// in some other section is not `core.bare`; boolean-aware, so every
/// spelling git accepts for true is true and a valueless key is true; and
/// worktree-aware, because `extensions.worktreeConfig` moves per-worktree
/// values into `config.worktree`, where `core.bare` is exactly the key it
/// moves.
///
/// `None` means the question could not be answered — an unreadable file, a
/// key that is absent, a value git would reject — and every caller here
/// treats that as "not proven", never as a default.
fn git_config_boolean(common: &Path, gitdir: &Path, section: &str, key: &str) -> Option<bool> {
    let shared = read_config(&common.join("config"))?;
    if truth(shared.get("extensions.worktreeconfig")) == Some(true)
        && let Some(per_worktree) = read_config(&gitdir.join("config.worktree"))
        && let Some(local) = truth(per_worktree.get(&format!("{section}.{key}")))
    {
        return Some(local);
    }
    truth(shared.get(&format!("{section}.{key}")))
}

fn read_config(path: &Path) -> Option<HashMap<String, Option<String>>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut values = HashMap::new();
    let mut current = String::new();
    for line in raw.lines() {
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') || text.starts_with(';') {
            continue;
        }
        if let Some(section) = parse_section_header(text) {
            current = section.to_lowercase();
            continue;
        }
        if let Some((name, value)) = parse_key_value(text) {
            values.insert(format!("{current}.{}", name.to_lowercase()), value);
        }
    }
    Some(values)
}

/// `[section]` or `[section "subsection"]` — the section name may not carry
/// `]`, `"`, or whitespace. The subsection is irrelevant to the keys read
/// here and is dropped.
fn parse_section_header(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let (name, rest) = match inner.find(char::is_whitespace) {
        Some(split) => inner.split_at(split),
        None => (inner, ""),
    };
    if name.is_empty() || name.contains(['"', ']']) {
        return None;
    }
    let rest = rest.trim_start();
    let subsection_ok = rest.is_empty()
        || (rest.len() >= 2
            && rest.starts_with('"')
            && rest.ends_with('"')
            && !rest[1..rest.len() - 1].contains('"'));
    subsection_ok.then_some(name)
}

/// `key` or `key = value` — the key starts alphabetic and continues
/// alphanumeric or `-`. A valueless key is `None`, which [`truth`] reads as
/// git does: true.
fn parse_key_value(text: &str) -> Option<(&str, Option<String>)> {
    let mut end = 0;
    for (i, c) in text.char_indices() {
        let valid = if i == 0 {
            c.is_ascii_alphabetic()
        } else {
            c.is_ascii_alphanumeric() || c == '-'
        };
        if !valid {
            break;
        }
        end = i + c.len_utf8();
    }
    if end == 0 {
        return None;
    }
    let (name, rest) = text.split_at(end);
    let rest = rest.trim_start();
    if rest.is_empty() {
        return Some((name, None));
    }
    let value = rest.strip_prefix('=')?;
    Some((name, Some(value.trim().to_string())))
}

fn truth(value: Option<&Option<String>>) -> Option<bool> {
    let value = value?;
    let Some(value) = value else {
        return Some(true);
    };
    if value.is_empty() {
        return Some(true);
    }
    match value.to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// A main checkout with a git *directory* and a settings file carrying
    /// `entry` in its env block.
    fn main_checkout(dir: &Path, entry: &str) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        write(
            &dir.join(".claude").join("settings.local.json"),
            &format!(r#"{{"env": {{"HARNEX_ALLOW_FLOOR_EDIT": {entry}}}}}"#),
        );
    }

    /// A linked worktree of `main`: the worktree root carries a `.git`
    /// pointer file, and the named git directory carries a `commondir`
    /// leading back to the main checkout's `.git`.
    fn linked_worktree(main: &Path, worktree: &Path) {
        let gitdir = main.join(".git").join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        write(
            &main.join(".git").join("config"),
            "[core]\n\tbare = false\n",
        );
        std::fs::create_dir_all(worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
    }

    #[test]
    fn grants_while_the_entry_stands_and_revokes_the_moment_it_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        main_checkout(dir.path(), r#""1""#);
        assert_eq!(floor_edit_grant(dir.path()), FloorGrant::Granted);
        write(
            &dir.path().join(".claude").join("settings.local.json"),
            r#"{"env": {}}"#,
        );
        assert_eq!(floor_edit_grant(dir.path()), FloorGrant::Absent);
    }

    #[test]
    fn reads_the_value_as_the_settings_schema_writes_it_a_string_never_a_number() {
        let dir = tempfile::tempdir().unwrap();
        main_checkout(dir.path(), "1");
        assert_eq!(floor_edit_grant(dir.path()), FloorGrant::Absent);
    }

    #[test]
    fn tells_a_file_it_cannot_parse_from_one_the_operator_never_wrote() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        assert_eq!(floor_edit_grant(dir.path()), FloorGrant::Absent);
        write(
            &dir.path().join(".claude").join("settings.local.json"),
            "{not json",
        );
        assert!(matches!(
            floor_edit_grant(dir.path()),
            FloorGrant::Unreadable { .. }
        ));
    }

    #[test]
    fn reads_the_main_repository_entry_from_inside_a_linked_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let worktree = dir.path().join("wt");
        main_checkout(&main, r#""1""#);
        linked_worktree(&main, &worktree);
        assert_eq!(canonical_repo_root(&worktree).unwrap(), main);
        assert_eq!(floor_edit_grant(&worktree), FloorGrant::Granted);
        // A worktree-local settings file is not consulted at all.
        write(
            &main.join(".claude").join("settings.local.json"),
            r#"{"env": {}}"#,
        );
        write(
            &worktree.join(".claude").join("settings.local.json"),
            r#"{"env": {"HARNEX_ALLOW_FLOOR_EDIT": "1"}}"#,
        );
        assert_eq!(floor_edit_grant(&worktree), FloorGrant::Absent);
    }

    #[test]
    fn refuses_a_bare_repository_worktree_which_has_no_checkout_to_carry_an_override() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("repo.git");
        let worktree = dir.path().join("wt");
        let gitdir = bare.join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        write(&bare.join("config"), "[core]\n\tbare = true\n");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(canonical_repo_root(&worktree), None);
        assert!(matches!(
            floor_edit_grant(&worktree),
            FloorGrant::Unreadable { .. }
        ));
    }

    #[test]
    fn reads_bare_the_way_git_does_every_true_spelling_and_only_in_its_own_section() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo.git");
        let worktree = dir.path().join("wt");
        let gitdir = repo.join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        for spelling in ["bare", "bare = yes", "bare = ON", "bare = 1"] {
            write(&repo.join("config"), &format!("[core]\n\t{spelling}\n"));
            assert_eq!(canonical_repo_root(&worktree), None, "spelling: {spelling}");
        }
        // `bare` outside [core] is not core.bare; an unanswerable value is
        // not proven false either.
        write(&repo.join("config"), "[other]\n\tbare = true\n");
        assert_eq!(canonical_repo_root(&worktree), None);
        write(&repo.join("config"), "[core]\n\tbare = maybe\n");
        assert_eq!(canonical_repo_root(&worktree), None);
        write(&repo.join("config"), "[core]\n\tbare = false\n");
        assert_eq!(canonical_repo_root(&worktree).unwrap(), dir.path());
    }

    #[test]
    fn a_subsection_header_does_not_leak_into_the_core_bare_read() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo.git");
        let worktree = dir.path().join("wt");
        let gitdir = repo.join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        // A `bare` under a foreign section/subsection is not `core.bare`; the
        // real `[core] bare = false` still governs.
        write(
            &repo.join("config"),
            "[remote \"origin\"]\n\tbare = true\n[core]\n\tbare = false\n",
        );
        assert_eq!(canonical_repo_root(&worktree).unwrap(), dir.path());
    }

    #[test]
    fn an_empty_bare_value_reads_true_as_git_does() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo.git");
        let worktree = dir.path().join("wt");
        let gitdir = repo.join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        write(&repo.join("config"), "[core]\n\tbare =\n");
        assert_eq!(canonical_repo_root(&worktree), None);
    }

    #[test]
    fn honours_a_worktree_config_bare_when_the_extension_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let worktree = dir.path().join("wt");
        let gitdir = repo.join(".git").join("worktrees").join("wt");
        write(&gitdir.join("commondir"), "../..\n");
        write(
            &repo.join(".git").join("config"),
            "[extensions]\n\tworktreeConfig = true\n[core]\n\tbare = true\n",
        );
        write(&gitdir.join("config.worktree"), "[core]\n\tbare = false\n");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(canonical_repo_root(&worktree).unwrap(), repo);
    }

    #[test]
    fn keeps_the_root_of_a_checkout_whose_git_directory_lives_elsewhere() {
        // `git init --separate-git-dir`: the pointer names a git directory
        // with no commondir, NOT under worktrees/ — the checkout is its own
        // answer.
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("co");
        let gitdir = dir.path().join("elsewhere.git");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::write(
            checkout.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(canonical_repo_root(&checkout).unwrap(), checkout);
    }

    #[test]
    fn keeps_a_submodule_root_whose_pointer_names_a_git_directory_with_no_commondir() {
        let dir = tempfile::tempdir().unwrap();
        let submodule = dir.path().join("sub");
        let gitdir = dir
            .path()
            .join("parent")
            .join(".git")
            .join("modules")
            .join("sub");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::create_dir_all(&submodule).unwrap();
        std::fs::write(
            submodule.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(canonical_repo_root(&submodule).unwrap(), submodule);
    }

    #[test]
    fn refuses_to_read_a_local_file_when_the_pointer_leads_nowhere() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".git"),
            "gitdir: /nonexistent/worktrees/wt\n",
        )
        .unwrap();
        assert_eq!(canonical_repo_root(dir.path()), None);
        write(
            &dir.path().join(".claude").join("settings.local.json"),
            r#"{"env": {"HARNEX_ALLOW_FLOOR_EDIT": "1"}}"#,
        );
        assert!(matches!(
            floor_edit_grant(dir.path()),
            FloorGrant::Unreadable { .. }
        ));
    }

    #[test]
    fn a_directory_with_no_git_at_all_is_its_own_root() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(canonical_repo_root(dir.path()).unwrap(), dir.path());
    }

    #[test]
    fn lexical_resolve_collapses_dot_segments_without_touching_the_filesystem() {
        assert_eq!(
            lexical_resolve(Path::new("/a/b"), Path::new("../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(
            lexical_resolve(Path::new("/a"), Path::new("/x/../../y")),
            PathBuf::from("/y")
        );
    }
}
