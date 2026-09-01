//! Detection of git invocations that skip the hook stack.
//!
//! Two forms: `git commit|push|merge|pull --no-verify` (plus the `commit -n`
//! shorthand), and the direct `core.hooksPath` reroutes (`git -c
//! core.hooksPath=…`, `git config … core.hooksPath …`). `--no-verify` skips
//! the entire pre-commit stack *including the secret scan*; a hook's own
//! escape hatch skips one check and never the stack. Detection reaches into
//! compound commands (`&&` / `;` chains, subshells, brace groups) — its value
//! over flat `permissions.deny` prefixes — and sees through the bare wrappers
//! in [`COMMAND_PREFIX_WORDS`] to the git word.
//!
//! Out of scope — obfuscated bypass, left to the project's own server-side
//! re-run: a git/shell alias whose value carries the flag, argument
//! indirection (`xargs git …`), a wrapper carrying its own options
//! (`nice -n10 git …`), `sh -c` / `$(…)` nesting, and env-var config
//! injection (`GIT_CONFIG_KEY_n`, `--config-env`). Value-consuming git
//! options are not modelled either, so a flag *value* that is literally
//! `--no-verify` (`git commit -m --no-verify`) blocks too — accepted:
//! contrived input, and the failure direction is a block that surfaces to
//! the operator, never a silent pass.

use super::command_line::{SplitError, split_commands};

/// git subcommands whose hook execution `--no-verify` skips.
const HOOKED_SUBCOMMANDS: [&str; 4] = ["commit", "push", "merge", "pull"];

/// Bare (option-less) wrappers that may legitimately precede the command
/// word, skipped through to reach the real command. A wrapper carrying its
/// own options (`nice -n10 git …`) is out of scope — the option word breaks
/// the skip, and modelling every wrapper's flag grammar is the arms race
/// this avoids.
const COMMAND_PREFIX_WORDS: [&str; 6] = ["env", "command", "nice", "nohup", "time", "setsid"];

/// Whether a token spells `--no-verify` as git's own option parser reads it.
///
/// Git accepts any **unambiguous prefix** of a long option, so `--no-verif`
/// is `--no-verify` (measured; `--no-ver` is rejected as ambiguous against
/// `--no-verbose`). Matching the whole prefix chain from `--no-v` up covers
/// every spelling git accepts; the shorter ones git rejects itself, so a
/// block there costs an error message on a command that was already going
/// to fail.
fn is_no_verify_option(word: &str) -> bool {
    word.starts_with("--no-v") && "--no-verify".starts_with(word)
}

/// Whether a short-option token requests `--no-verify`, reading a cluster the
/// way git reads it: characters are flags left to right until one takes a
/// value, after which the rest of the token IS that value. So `-nm x` is
/// `-n -m x` (measured: the pre-commit hook does not run) while `-mn` is
/// `-m` with the message "n" (measured: it does).
///
/// The value-consuming set is `git commit`'s — mandatory for `-F -m -c -C -t
/// -U`, optional-but-attached for `-S -u` (measured against `git commit -h`).
/// `-n` means `--no-verify` on `commit` alone — on `push` it is `--dry-run`
/// and on `merge` it suppresses the diffstat, which is why the caller
/// scopes it.
fn is_short_no_verify(word: &str) -> bool {
    let Some(cluster) = word.strip_prefix('-') else {
        return false;
    };
    if cluster.is_empty() || !cluster.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    for c in cluster.chars() {
        if c == 'n' {
            return true;
        }
        if matches!(c, 'F' | 'm' | 'c' | 'C' | 't' | 'U' | 'S' | 'u') {
            return false;
        }
    }
    false
}

fn is_env_assignment(word: &str) -> bool {
    let Some(eq) = word.find('=') else {
        return false;
    };
    let name = &word[..eq];
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_hooks_path_key(word: &str) -> bool {
    let lower = word.to_lowercase();
    lower == "core.hookspath" || lower.starts_with("core.hookspath=")
}

/// The floor violation a single simple command would commit, or `None`.
/// Returns a human-readable reason naming the exact bypass detected.
pub fn detect_hook_bypass(words: &[String]) -> Option<String> {
    let mut idx = 0;
    while idx < words.len()
        && (is_env_assignment(&words[idx]) || COMMAND_PREFIX_WORDS.contains(&words[idx].as_str()))
    {
        idx += 1;
    }
    let cmd = words.get(idx)?;
    if cmd.rsplit('/').next() != Some("git") {
        return None;
    }

    // Global options sit between `git` and the subcommand: `-c name=val`
    // (split or attached), `-C dir`, and long `--flag[=value]` forms.
    let mut sub: Option<usize> = None;
    let mut j = idx + 1;
    while j < words.len() {
        let w = words[j].as_str();
        if w == "-c" || w == "-C" {
            if w == "-c" && is_hooks_path_key(words.get(j + 1).map_or("", |v| v.as_str())) {
                return Some(format!(
                    "`git -c {}` overrides core.hooksPath, rerouting the git hook stack",
                    words[j + 1]
                ));
            }
            j += 2;
            continue;
        }
        if let Some(attached) = w.strip_prefix("-c")
            && attached.contains('=')
        {
            if is_hooks_path_key(attached) {
                return Some(format!(
                    "`git {w}` overrides core.hooksPath, rerouting the git hook stack"
                ));
            }
            j += 1;
            continue;
        }
        if matches!(w, "--git-dir" | "--work-tree" | "--namespace") {
            j += 2; // separated value — skip it so it is not read as the subcommand
            continue;
        }
        if w.starts_with('-') {
            j += 1;
            continue;
        }
        sub = Some(j);
        break;
    }
    let sub_idx = sub?;
    let sub = words[sub_idx].as_str();

    if sub == "config" {
        return detect_config_reroute(&words[sub_idx + 1..]);
    }

    if !HOOKED_SUBCOMMANDS.contains(&sub) {
        return None;
    }
    for w in &words[sub_idx + 1..] {
        if w == "--" {
            break; // pathspec terminator — flags cannot follow
        }
        if is_no_verify_option(w) {
            let skipped = if sub == "push" {
                "pre-push gate stack"
            } else {
                "pre-commit stack (secret scan included) and commit-msg"
            };
            return Some(format!("`git {sub} {w}` skips the {skipped}"));
        }
        if sub == "commit" && is_short_no_verify(w) {
            return Some(format!(
                "`git commit {w}` (--no-verify) skips the pre-commit stack \
                 (secret scan included) and commit-msg"
            ));
        }
    }
    None
}

/// `git config` naming core.hooksPath is treated as a WRITE (reroute) unless
/// it is unambiguously a READ — a default-deny that stays fail-safe as git
/// grows new write subcommands (a flag-enumeration approach would miss the
/// git 2.46 `unset` subcommand, a valueless write). Reads changing nothing
/// (hook-routing diagnostics) are the explicit allow-list.
///
/// The POSIX `--` terminator is honoured, because git does: a token after it
/// is a positional (a value or the key), never an option. Without that,
/// `git config --add -- core.hooksPath --get` — a write whose value happens to
/// be `--get` — would read as a diagnostic and pass (measured against git
/// 2.55; a live evasion the flag-position split closes).
fn detect_config_reroute(rest: &[String]) -> Option<String> {
    rest.iter().find(|w| is_hooks_path_key(w))?;
    let flags_end = rest.iter().position(|w| w == "--").unwrap_or(rest.len());
    let flags = &rest[..flags_end];

    // A write flag in option position is a definitive write, whatever value
    // follows — this is what the terminator protects, so it is decided from
    // `flags` alone.
    const WRITE_FLAGS: [&str; 4] = ["--unset", "--unset-all", "--add", "--replace-all"];
    if flags.iter().any(|w| WRITE_FLAGS.contains(&w.as_str())) {
        return Some(reroute_reason());
    }
    // A read flag in option position is a definitive read; it names what to
    // fetch (`--get-urlmatch` even takes a URL value), so its presence in the
    // option stream settles the invocation.
    const READ_FLAGS: [&str; 7] = [
        "--get",
        "--get-all",
        "--get-regexp",
        "--get-regex",
        "--get-urlmatch",
        "--list",
        "-l",
    ];
    if flags.iter().any(|w| READ_FLAGS.contains(&w.as_str())) {
        return None;
    }
    // No decisive flag: classify by the positional stream — the subcommand and
    // key, options and their scoped values removed, everything past the
    // terminator kept. The value-consuming options that can precede the key
    // (`--file <path>` / `-f`, `--blob <ref>`, `--type <t>` / `-t`) are dropped
    // with their value so a `--file /x core.hooksPath` read is not mistaken for
    // a keyless one.
    const VALUE_SCOPE_OPTS: [&str; 5] = ["--file", "-f", "--blob", "--type", "-t"];
    let mut positionals: Vec<&str> = Vec::new();
    let mut k = 0;
    let mut terminated = false;
    while k < rest.len() {
        let w = rest[k].as_str();
        if terminated {
            positionals.push(w);
            k += 1;
        } else if w == "--" {
            terminated = true;
            k += 1;
        } else if VALUE_SCOPE_OPTS.contains(&w) {
            k += 2;
        } else if w.starts_with('-') {
            k += 1;
        } else {
            positionals.push(w);
            k += 1;
        }
    }
    // A read subcommand changes nothing; `set` / `unset` and every other head
    // falls through to the block.
    if matches!(positionals.first(), Some(&"get" | &"list")) {
        return None;
    }
    // Classic bare read: the key is the only positional, with no value to write
    // (`git config core.hooksPath`). A value after it — whatever it is spelled,
    // `--unset` past a terminator included — is a write.
    if matches!(positionals.first(), Some(head) if is_hooks_path_key(head))
        && positionals.len() == 1
    {
        return None;
    }
    Some(reroute_reason())
}

fn reroute_reason() -> String {
    "`git config` writes core.hooksPath, rerouting the git hook stack".into()
}

/// First floor violation across every simple command in a command line, or
/// `None`. A [`SplitError`] is the caller's cue to fail open with a visible
/// skip, never to block.
pub fn detect_command_line_bypass(command_line: &str) -> Result<Option<String>, SplitError> {
    for words in split_commands(command_line)? {
        if let Some(reason) = detect_hook_bypass(&words) {
            return Ok(Some(reason));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn line(command: &str) -> Option<String> {
        detect_command_line_bypass(command).expect("parses")
    }

    #[test]
    fn flags_no_verify_on_commit_push_and_merge() {
        assert!(detect_hook_bypass(&words(&["git", "commit", "--no-verify"])).is_some());
        assert!(detect_hook_bypass(&words(&["git", "push", "--no-verify"])).is_some());
        assert!(detect_hook_bypass(&words(&["git", "merge", "--no-verify", "topic"])).is_some());
    }

    #[test]
    fn flags_the_short_n_on_commit_only_because_push_n_is_dry_run() {
        assert!(detect_hook_bypass(&words(&["git", "commit", "-n"])).is_some());
        assert!(detect_hook_bypass(&words(&["git", "push", "-n"])).is_none());
        assert!(detect_hook_bypass(&words(&["git", "merge", "-n", "topic"])).is_none());
    }

    #[test]
    fn flags_core_hooks_path_overrides_in_every_write_form() {
        assert!(
            detect_hook_bypass(&words(&["git", "-c", "core.hooksPath=/dev/null", "commit"]))
                .is_some()
        );
        assert!(
            detect_hook_bypass(&words(&["git", "-ccore.hooksPath=/dev/null", "commit"])).is_some()
        );
        assert!(
            detect_hook_bypass(&words(&["git", "config", "core.hooksPath", "/dev/null"])).is_some()
        );
        assert!(
            detect_hook_bypass(&words(&["git", "config", "set", "core.hooksPath", "x"])).is_some()
        );
        assert!(
            detect_hook_bypass(&words(&["git", "config", "unset", "core.hookspath"])).is_some()
        );
        assert!(
            detect_hook_bypass(&words(&["git", "config", "--unset", "core.hooksPath"])).is_some()
        );
    }

    #[test]
    fn allows_every_measured_read_of_core_hooks_path() {
        for read in [
            &["config", "--get", "core.hooksPath"][..],
            &["config", "--get-all", "core.hooksPath"],
            &["config", "--get-regexp", "core.hooksPath"],
            &["config", "--get-urlmatch", "core.hooksPath", "https://x"],
            &["config", "--list"],
            &["config", "-l"],
            &["config", "get", "core.hooksPath"],
            &["config", "list"],
            &["config", "core.hooksPath"],
            &["config", "--file", "/x", "get", "core.hooksPath"],
            &["config", "-f", "/x", "core.hooksPath"],
            &["config", "--type", "path", "--get", "core.hooksPath"],
            &["config", "-t", "path", "core.hooksPath"],
        ] {
            let mut argv = vec!["git"];
            argv.extend_from_slice(read);
            assert!(
                detect_hook_bypass(&words(&argv)).is_none(),
                "false-blocked read: {read:?}"
            );
        }
    }

    #[test]
    fn flags_every_measured_write_to_core_hooks_path() {
        for write in [
            &["config", "core.hooksPath", "/dev/null"][..],
            &["config", "set", "core.hooksPath", "x"],
            &["config", "unset", "core.hookspath"],
            &["config", "--unset", "core.hooksPath"],
            &["config", "--unset-all", "core.hooksPath"],
            &["config", "--add", "core.hooksPath", "/x"],
            &["config", "--replace-all", "core.hooksPath", "/x"],
            &["config", "--file", "/x", "core.hooksPath", "/dev/null"],
            // A write whose value is spelled like a read flag, hidden past the
            // POSIX terminator — the live git 2.55 evasion the split closes.
            &["config", "--add", "--", "core.hooksPath", "--get"],
            &["config", "--", "core.hooksPath", "--unset"],
        ] {
            let mut argv = vec!["git"];
            argv.extend_from_slice(write);
            assert!(
                detect_hook_bypass(&words(&argv)).is_some(),
                "missed write: {write:?}"
            );
        }
    }

    #[test]
    fn sees_through_env_assignment_and_bare_wrapper_prefixes() {
        assert!(detect_hook_bypass(&words(&["FOO=1", "git", "commit", "--no-verify"])).is_some());
        assert!(
            detect_hook_bypass(&words(&["env", "nice", "git", "commit", "--no-verify"])).is_some()
        );
        assert!(detect_hook_bypass(&words(&["/usr/bin/git", "commit", "-n"])).is_some());
    }

    #[test]
    fn does_not_read_a_separated_global_option_value_as_the_subcommand() {
        assert!(detect_hook_bypass(&words(&["git", "--git-dir", "commit", "status"])).is_none());
        assert!(
            detect_hook_bypass(&words(&[
                "git",
                "--work-tree",
                "/x",
                "commit",
                "--no-verify"
            ]))
            .is_some()
        );
    }

    #[test]
    fn never_flags_a_clean_invocation_or_an_unhooked_subcommand() {
        assert!(detect_hook_bypass(&words(&["git", "status"])).is_none());
        assert!(detect_hook_bypass(&words(&["git", "commit", "-m", "ok"])).is_none());
        assert!(detect_hook_bypass(&words(&["git", "rebase", "--no-verify"])).is_none());
        assert!(detect_hook_bypass(&words(&["ls", "--no-verify"])).is_none());
    }

    #[test]
    fn stops_flag_scanning_at_the_pathspec_terminator() {
        assert!(detect_hook_bypass(&words(&["git", "commit", "--", "--no-verify"])).is_none());
    }

    #[test]
    fn leaves_other_git_config_keys_alone() {
        assert!(detect_hook_bypass(&words(&["git", "config", "user.name", "x"])).is_none());
        assert!(detect_hook_bypass(&words(&["git", "-c", "user.name=x", "commit"])).is_none());
    }

    #[test]
    fn finds_a_bypass_buried_in_a_compound_command() {
        assert!(line("echo ok && git commit --no-verify -m x").is_some());
        assert!(line("cd x; git push --no-verify").is_some());
        assert!(line("cd x && git commit -nm y").is_some());
    }

    #[test]
    fn passes_a_quoted_mention_without_a_bypass() {
        assert!(line("echo 'git commit --no-verify'").is_none());
        assert!(line("git commit -m 'do not use --no-verify'").is_none());
    }

    #[test]
    fn finds_a_bypass_behind_a_brace_group_or_bare_wrapper() {
        assert!(line("{ git commit --no-verify; }").is_some());
        assert!(line("nohup git commit --no-verify").is_some());
        assert!(line("time git commit --no-verify").is_some());
        assert!(line("setsid git commit --no-verify").is_some());
    }

    #[test]
    fn reads_an_attached_git_dir_value_without_losing_the_subcommand() {
        // `--git-dir=.` is one token; the subcommand still follows.
        assert!(
            detect_hook_bypass(&words(&["git", "--git-dir=.", "commit", "--no-verify"])).is_some()
        );
    }

    #[test]
    fn does_not_false_block_a_literal_brace_argument() {
        assert!(line("find . -name '*.rs' -exec grep --no-verify {} +").is_none());
        assert!(line("echo } x").is_none());
    }

    #[test]
    fn detects_every_measured_redirection_spelling() {
        for command in [
            "git commit 2>&1 --no-verify",
            "git commit --no-verify>log",
            "git commit --no-verify >| log",
            "git commit --no-verify &>> log",
            "git commit --no-verify &> log",
            "git commit 1>&2 --no-verify",
            "git commit --no-verify <>f",
            "git commit --no-verify 2>>log",
            ">log git commit --no-verify",
            "git push --no-verify < /dev/null",
            "foo |& git commit --no-verify",
        ] {
            assert!(line(command).is_some(), "missed: {command}");
        }
    }

    #[test]
    fn detects_every_measured_bypass_spelling() {
        for command in [
            "git commit -nm x",
            "git commit --no-verif -m x",
            "git commit --no-v -m x",
            "git commit --no-ver\\\nify -m x",
            "{fd}>/dev/null git commit --no-verify -m x",
            "git commit \"--no-ver\\\nify\" -m x",
            "git commit $'--no-verify' -m x",
            "git commit $'-n' -m x",
            "git pull --no-verify",
            "git commit $'-\\x6e' -m x",
            "git commit $'--no-verif\\x79' -m x",
            "git commit $'--no-\\166erify' -m x",
            "git commit $'--no-verify\\0zzz' -m x",
            "git commit $\"--no-verify\" -m x",
        ] {
            assert!(line(command).is_some(), "missed: {command}");
        }
    }

    #[test]
    fn does_not_block_the_measured_clean_spellings() {
        for command in [
            "git commit -am msg",
            "git log --no-verbose",
            "git push origin main",
            "git commit -m 'literal $q'",
            "git commit -m \"a $x b\"",
            "git pull origin main",
            "git commit $'--no-\\verify' -m x",
            "git status",
        ] {
            assert!(line(command).is_none(), "false-blocked: {command}");
        }
    }
}
