//! The floor held in Claude Code's Bash sandbox.
//!
//! What a shell command writes is decided by every program it starts, so no
//! reading of the command line closes over it. `sandbox.filesystem.denyWrite`
//! is enforced by the operating system on each of those processes, which makes
//! it the place a Bash write into the floor is refused: every floor entry is
//! covered by one of its entries, and `check` reports the ones that are not.
//!
//! An entry covers a floor path when it names that path or a directory above
//! it, read as the sandbox reads an entry in project settings: `./` or no
//! prefix is the project root, and a trailing `/` or `/**` names the directory
//! itself. Two spellings the sandbox accepts are not coverage here. An absolute
//! or `~/` path names this checkout on one machine, while
//! `.claude/settings.json` is every developer's; and an entry carrying `*`, `?`
//! or `[` is skipped by the Linux and WSL2 sandbox.

/// The `denyWrite` entry that holds one floor entry. The trailing `/` is
/// dropped because a sandbox before Claude Code 2.1.224 passed it through, and
/// an entry spelled with one guarded nothing there.
pub fn deny_write_entry(floor_entry: &str) -> String {
    format!("./{}", floor_entry.trim_end_matches('/'))
}

/// The floor entries no `deny_write` entry covers, in floor order.
pub fn uncovered<'a>(
    floor: impl IntoIterator<Item = &'a str>,
    deny_write: &[&str],
) -> Vec<&'a str> {
    let covering: Vec<Vec<&str>> = deny_write
        .iter()
        .filter_map(|entry| project_components(entry))
        .collect();
    floor
        .into_iter()
        .filter(|entry| {
            let path: Vec<&str> = entry.trim_end_matches('/').split('/').collect();
            !covering.iter().any(|dir| path.starts_with(dir))
        })
        .collect()
}

/// A `denyWrite` entry as components below the project root, empty for the
/// root itself, or `None` for an entry that covers nothing in every
/// developer's checkout.
fn project_components(entry: &str) -> Option<Vec<&str>> {
    if entry.is_empty() || entry.starts_with('/') || entry == "~" || entry.starts_with("~/") {
        return None;
    }
    let entry = entry.strip_suffix("/**").unwrap_or(entry);
    if entry.contains(['*', '?', '[']) {
        return None;
    }
    let mut components = Vec::new();
    for part in entry.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            part => components.push(part),
        }
    }
    Some(components)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left(floor: &[&'static str], deny_write: &[&str]) -> Vec<&'static str> {
        uncovered(floor.iter().copied(), deny_write)
    }

    #[test]
    fn the_projection_names_each_entry_from_the_root_without_a_trailing_slash() {
        assert_eq!(deny_write_entry("hooks/"), "./hooks");
        assert_eq!(
            deny_write_entry(".claude/settings.json"),
            "./.claude/settings.json"
        );
        let floor = ["harness.toml", "hooks/", ".github/workflows/"];
        let projected: Vec<String> = floor.iter().map(|e| deny_write_entry(e)).collect();
        let projected: Vec<&str> = projected.iter().map(String::as_str).collect();
        assert!(left(&floor, &projected).is_empty());
    }

    #[test]
    fn an_entry_covers_its_own_path_and_everything_below_it() {
        let floor = ["hooks/", ".gitleaks.toml", ".github/workflows/"];
        for deny in [
            vec!["./hooks", "./.gitleaks.toml", "./.github/workflows"],
            vec!["hooks/", ".gitleaks.toml", ".github/"],
            vec!["./hooks/**", "./.gitleaks.toml", "./.github/workflows/**"],
            vec!["./"],
            vec!["."],
            vec!["./docs/../hooks", "./.gitleaks.toml", "./.github"],
        ] {
            assert!(left(&floor, &deny).is_empty(), "{deny:?}");
        }
    }

    #[test]
    fn an_entry_below_or_beside_a_floor_path_covers_none_of_it() {
        assert_eq!(left(&["hooks/"], &["./hooks/pre-commit"]), ["hooks/"]);
        assert_eq!(left(&["hooks/"], &["./hook"]), ["hooks/"]);
        assert_eq!(left(&["hooks/"], &["./hooks-old"]), ["hooks/"]);
        assert_eq!(left(&["harness.toml"], &["./harness"]), ["harness.toml"]);
    }

    #[test]
    fn a_spelling_that_does_not_hold_for_every_developer_is_not_coverage() {
        for deny in [
            "/Users/dev/repo/hooks",
            "//Users/dev/repo/hooks",
            "~/repo/hooks",
            "~",
            "./hooks/*",
            "./hook?",
            "./[h]ooks",
            "../repo/hooks",
            "",
        ] {
            assert_eq!(left(&["hooks/"], &[deny]), ["hooks/"], "{deny}");
        }
    }

    #[test]
    fn matching_is_exact_in_case() {
        assert_eq!(left(&["hooks/"], &["./Hooks"]), ["hooks/"]);
    }
}
