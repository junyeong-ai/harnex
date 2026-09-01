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

/// Shell reserved words that stand where a command does and are followed by
/// one: negation (`! git …`), the condition of a compound command (`if` /
/// `while` / `until` git …), and its body (`then` / `elif` / `else` / `do`
/// git …). The splitter has already cut the line at `;` / newline, so the
/// keyword leads its own simple command; skipping it reaches the git word the
/// keyword introduces. A `then git commit --no-verify` whose branch never runs
/// is blocked anyway — the check is static, and the safe direction is to
/// assume the branch is taken.
const RESERVED_PREFIX_WORDS: [&str; 8] =
    ["!", "if", "elif", "then", "else", "while", "until", "do"];

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
        && (is_env_assignment(&words[idx])
            || COMMAND_PREFIX_WORDS.contains(&words[idx].as_str())
            || RESERVED_PREFIX_WORDS.contains(&words[idx].as_str()))
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
/// The invocation is parsed once, the way git parses it: the POSIX `--`
/// terminator ends option scanning, and a value-consuming option swallows the
/// next token. Both matter to the read/write question because a read flag can
/// masquerade as the *value* of another token — `git config --add --
/// core.hooksPath --get` (a write whose value is `--get`, past the terminator)
/// and `git config set --comment --get core.hooksPath /evil` (a write whose
/// comment is `--get`) both write on git 2.55, and both would read as
/// diagnostics without this split.
///
/// Options are matched as git resolves them — the exact spelling or any
/// unambiguous prefix (`--unset-a` is `--unset-all`, a valueless write that an
/// exact-match set would miss and read as the bare key). The match is over a
/// subset of git's options, but it is sound: whenever git accepts an
/// abbreviation it is unambiguous in git's full set, hence in this subset too,
/// so the resolution agrees; when it is ambiguous git rejects it and nothing
/// is written, so the classification cannot matter.
///
/// Not modelled: an invocation that un-routes hooks without naming the key —
/// `git config --remove-section core` drops `core.hooksPath` with the whole
/// section. Detecting it needs the live config, which this syntactic check
/// does not read; it is out of scope, backstopped by the server-side re-run.
fn detect_config_reroute(rest: &[String]) -> Option<String> {
    // Options that take the next token as a value — read-scoping (`--file`,
    // `--blob`, `--type`, `--default`) and write-filtering (`--comment`,
    // `--value`) alike. Skipping the value is what stops a read flag spelled as
    // that value from settling the classification.
    const VALUE_OPTS: [&str; 6] = [
        "--file",
        "--blob",
        "--type",
        "--default",
        "--comment",
        "--value",
    ];
    const VALUE_SHORT: [&str; 2] = ["-f", "-t"];
    const WRITE_FLAGS: [&str; 4] = ["--unset", "--unset-all", "--add", "--replace-all"];
    const READ_FLAGS: [&str; 6] = [
        "--get",
        "--get-all",
        "--get-regexp",
        "--get-urlmatch",
        "--list",
        "-l",
    ];

    let mut option_flags: Vec<&str> = Vec::new();
    let mut positionals: Vec<&str> = Vec::new();
    let mut k = 0;
    let mut terminated = false;
    while k < rest.len() {
        let w = rest[k].as_str();
        if terminated {
            positionals.push(w);
            k += 1;
        } else if VALUE_SHORT.contains(&w) || abbreviates(w, &VALUE_OPTS) {
            k += 2; // the option and its value, neither an operative token
        } else if w == "--" {
            terminated = true;
            k += 1;
        } else if w.starts_with('-') {
            option_flags.push(w);
            k += 1;
        } else {
            positionals.push(w);
            k += 1;
        }
    }

    // core.hooksPath is operated on only as a positional — the key argument.
    // Named anywhere else (a `--default core.hooksPath` fallback while reading
    // another key, the value written to a different key) it is not the subject,
    // and the command reroutes nothing.
    positionals.iter().find(|w| is_hooks_path_key(w))?;

    // A write flag in option position is a definitive write; a read flag there
    // is a definitive read (it names what to fetch). Write wins the tie so
    // `--get --unset` is a write.
    if option_flags.iter().any(|w| abbreviates(w, &WRITE_FLAGS)) {
        return Some(reroute_reason());
    }
    // No write flag reached here, so a read flag settles it as a read.
    if option_flags.iter().any(|w| abbreviates(w, &READ_FLAGS)) {
        return None;
    }
    // No decisive flag: the leading positional is the subcommand or the key.
    // `get` / `list` read; `set` / `unset` and every other head falls through
    // to the block.
    if matches!(positionals.first(), Some(&"get" | &"list")) {
        return None;
    }
    // Classic bare read: the key is the only positional, with no value to write
    // (`git config core.hooksPath`). A value after it is a write.
    if matches!(positionals.first(), Some(head) if is_hooks_path_key(head))
        && positionals.len() == 1
    {
        return None;
    }
    Some(reroute_reason())
}

/// Whether `token` is how git would name one of `canon` — the exact spelling,
/// or a prefix that exactly one of them carries (git's unambiguous-abbreviation
/// rule). A short `-x` option is exact-only; a `--long` one abbreviates.
fn abbreviates(token: &str, canon: &[&str]) -> bool {
    if canon.contains(&token) {
        return true;
    }
    if !token.starts_with("--") {
        return false;
    }
    let mut matches = canon.iter().filter(|c| c.starts_with(token));
    matches.next().is_some() && matches.next().is_none()
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
            // `--default` supplies a fallback value and reads without `--get`
            // (measured); its value must not be read as the key.
            &["config", "--default", "x", "core.hooksPath"],
            &["config", "--default", "x", "--get", "core.hooksPath"],
            // core.hooksPath named only as the `--default` fallback while a
            // different key is read — not the subject, so not a reroute.
            &["config", "--default", "core.hooksPath", "user.missing"],
            // An unrelated write must not be dragged in by the flag scan.
            &["config", "--unset", "user.name"],
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
    fn sees_through_reserved_word_command_position_prefixes() {
        // Negation, the condition of a compound command, and its body all stand
        // where a command does and run the git that follows; a static check
        // blocks the git word regardless of whether the branch is taken.
        for prefix in [
            &["!"][..],
            &["if"],
            &["while"],
            &["until"],
            &["then"],
            &["elif"],
            &["else"],
            &["do"],
        ] {
            let mut argv = vec![];
            argv.extend_from_slice(prefix);
            argv.extend_from_slice(&["git", "commit", "--no-verify"]);
            assert!(
                detect_hook_bypass(&words(&argv)).is_some(),
                "missed reserved-word prefix: {prefix:?}"
            );
        }
        // A reserved word before a non-git command is not a false positive.
        assert!(detect_hook_bypass(&words(&["!", "ls"])).is_none());
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
            // A write whose value-consuming option swallows the read flag —
            // `--comment` / `--value` take the next token, so `--get` there is
            // the value, not a diagnostic (both write on git 2.55).
            &[
                "config",
                "set",
                "--comment",
                "--get",
                "core.hooksPath",
                "/evil",
            ],
            &[
                "config",
                "set",
                "--value",
                "--get",
                "core.hooksPath",
                "/evil",
            ],
            // A valueless write reached by git's unambiguous-prefix abbreviation
            // — `--unset-a` is `--unset-all`, which an exact-match set would miss
            // and read as the bare key (both write on git 2.55).
            &["config", "--unset-a", "core.hooksPath"],
            &["config", "--unset-al", "core.hooksPath"],
            &["config", "--ad", "core.hooksPath", "/x"],
            // A write whose abbreviated value-consuming option swallows the read
            // flag: `--com` is `--comment`, so `--get` is its value, not a read.
            &["config", "set", "--com", "--get", "core.hooksPath", "/evil"],
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
    fn an_ambiguous_abbreviation_git_rejects_is_not_forced_to_a_verdict() {
        // `--unse` prefixes both --unset and --unset-all, so git rejects it and
        // nothing is written; the read fallthrough is harmless.
        assert!(
            detect_hook_bypass(&words(&["git", "config", "--unse", "core.hooksPath"])).is_none()
        );
        // An abbreviated read is still a read.
        assert!(
            detect_hook_bypass(&words(&["git", "config", "--get-a", "core.hooksPath"])).is_none()
        );
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
        // A group after the reserved-word prefixes `time` / `!` still runs the
        // git command; missing it would be a silent bypass.
        assert!(line("time { git commit --no-verify; }").is_some());
        assert!(line("! { git commit --no-verify; }").is_some());
        // A bare reserved-word prefix runs git directly on one line, in both
        // the long and clustered-short spellings of the skip.
        assert!(line("! git commit --no-verify").is_some());
        assert!(line("if git commit --no-verify; then :; fi").is_some());
        assert!(line("if git commit -n; then :; fi").is_some());
        assert!(line("if x; then git commit --no-verify; fi").is_some());
        // A reserved word mid-argument is not a prefix — `echo` runs, not git.
        assert!(line("echo if git commit --no-verify").is_none());
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
