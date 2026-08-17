//! The `${CLAUDE_PROJECT_DIR}` anchor in hook command strings.
//!
//! Claude Code exports the anchor to every spawned hook process, which makes
//! it the one token form in a `settings.json` hook that denotes a path in the
//! repository *by construction*. Everything else in a `command` — a binary on
//! `PATH`, an absolute path, a `$HOME`-relative one — is a word this crate
//! cannot resolve without guessing.
//!
//! One reader, because two would drift: the auditor that flags a hook pointing
//! at a missing script and the manifest test that checks the foundation tier
//! wires only foundation artifacts ask the same question of the same grammar.
//!
//! Two grammars, because a hook handler's strings are not all parsed alike,
//! and which applies is a property of the **handler's shape**. With `args`
//! present the runtime spawns `command` directly with args as the argument
//! vector and no shell, so every string is one literal path and a space
//! belongs to the filename. With `args` absent, `command` is a shell string
//! and a token ends at an unescaped metacharacter or at its quote.
//!
//! Never infer this from where the anchor sits inside the string: that
//! coincidence reads `${CLAUDE_PROJECT_DIR}/hooks/run.sh --verbose` as a
//! filename ending in `--verbose`, and disagrees with the same command
//! written `bash ...`.
//!
//! ## What this module refuses to do
//!
//! - Never claim a token the anchor does not cover. A token carrying a second
//!   variable is refused by both grammars, since neither can say what the
//!   runtime expands it to; a glob is refused only by the shell grammar, where
//!   it is a pattern rather than a path.
//! - Never infer the grammar from the string. The caller knows the handler's
//!   shape and says so by choosing the function — and every step below the
//!   entry points is tagged the same way, because a shared unescape is how one
//!   grammar's rule leaks into the other's answer.

/// The documented project-root anchor spellings, with their separator. Both
/// braced and bare are valid in a hook command, so a check that read only one
/// would be blind to half the harnesses in the wild.
pub const ANCHORS: &[&str] = &["${CLAUDE_PROJECT_DIR}/", "$CLAUDE_PROJECT_DIR/"];

/// Shell metacharacters that end an unquoted path token. `$` is deliberately
/// absent: a token carrying another variable is rejected whole rather than
/// truncated at the variable, so the quoted and unquoted spellings of the same
/// command reach the same answer.
const TERMINATORS: &[char] = &[
    '"', '\'', ' ', '\t', '\n', ')', ';', '&', '|', '>', '<', '`',
];

/// Every project-relative path a **shell-interpreted** string anchors at
/// `${CLAUDE_PROJECT_DIR}` — a hook handler's `command`, or any blob of such
/// strings. One string can carry several (a wrapper plus the verifier it
/// dispatches), so the scan continues past the first match, and each token
/// ends where the shell would end it.
pub fn paths_in_command(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    let mut consumed = 0usize;
    while let Some((idx, anchor)) = next_anchor(rest) {
        let anchor_start = consumed + idx;
        let tail = &rest[idx + anchor.len()..];
        let quote = source[..anchor_start]
            .chars()
            .next_back()
            .filter(|c| *c == '"' || *c == '\'');
        let end = match quote {
            Some(q) => tail.find(q).unwrap_or(tail.len()),
            None => unquoted_end(tail),
        };
        out.extend(shell_token(&tail[..end]));
        consumed = anchor_start + anchor.len() + end;
        rest = &tail[end..];
    }
    out
}

/// The earliest anchor occurrence and which spelling it was. The braced form
/// is a prefix-superset of the bare one at the same offset, so the longer
/// match wins where both start together.
fn next_anchor(haystack: &str) -> Option<(usize, &'static str)> {
    ANCHORS
        .iter()
        .filter_map(|a| haystack.find(a).map(|i| (i, *a)))
        .min_by_key(|(i, a)| (*i, std::cmp::Reverse(a.len())))
}

/// End of an unquoted token: the first terminator that a backslash does not
/// escape. `bash ${CLAUDE_PROJECT_DIR}/my\ dir/x.sh` names one path, and
/// cutting at the escaped space would report a truncated name as missing.
fn unquoted_end(tail: &str) -> usize {
    let bytes = tail.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += if i + 1 < bytes.len() { 2 } else { 1 };
            continue;
        }
        if !tail.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if tail[i..]
            .chars()
            .next()
            .is_some_and(|c| TERMINATORS.contains(&c))
        {
            return i;
        }
        i += 1;
    }
    tail.len()
}

/// The project-relative path a **literal argument** anchors at
/// `${CLAUDE_PROJECT_DIR}` — one `args` element, which the runtime substitutes
/// into and passes through unsplit. Everything after the anchor is part of the
/// path, spaces included, and a prefix before it (`--config=`) is not.
pub fn path_in_argument(value: &str) -> Option<String> {
    let (idx, anchor) = next_anchor(value)?;
    literal_token(&value[idx + anchor.len()..])
}

/// What both grammars refuse: an empty token, and one carrying a second
/// variable — neither grammar can say what the runtime expands it to.
fn covered(candidate: &str) -> bool {
    !candidate.is_empty() && !candidate.contains('$')
}

/// A token from a shell-interpreted string. A glob is a pattern rather than a
/// path, so it is left alone; a backslash is an escape, so the shell hands the
/// runtime the escaped character itself and it is unwrapped here.
fn shell_token(candidate: &str) -> Option<String> {
    if !covered(candidate) || candidate.contains(['*', '?', '[']) {
        return None;
    }
    let unescaped = candidate.replace('\\', "");
    (!unescaped.is_empty()).then_some(unescaped)
}

/// A token from a literal argument, taken verbatim. Nothing interprets it, so
/// a backslash is part of the filename and a `*` is a character rather than a
/// pattern — unescaping here would delete a byte the runtime actually passes.
fn literal_token(candidate: &str) -> Option<String> {
    covered(candidate).then(|| candidate.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_every_anchored_path_from_one_command() {
        assert_eq!(
            paths_in_command(
                "bash \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh\" ${CLAUDE_PROJECT_DIR}/tools/check.ts"
            ),
            vec!["hooks/_runner.sh", "tools/check.ts"]
        );
    }

    #[test]
    fn a_command_token_ends_where_the_shell_ends_it() {
        // Trailing flags are not part of the filename. Reading the whole value
        // as a path because the anchor sits at byte 0 reports a correct hook
        // as broken — and disagrees with the same command written `bash ...`.
        for source in [
            "${CLAUDE_PROJECT_DIR}/hooks/run.sh --verbose",
            "bash ${CLAUDE_PROJECT_DIR}/hooks/run.sh --verbose",
            "${CLAUDE_PROJECT_DIR}/tools/fmt.py --check --fast",
        ] {
            let paths = paths_in_command(source);
            assert!(
                paths.iter().all(|p| !p.contains(' ')),
                "'{source}' -> {paths:?}"
            );
        }
        assert_eq!(
            paths_in_command("${CLAUDE_PROJECT_DIR}/hooks/run.sh --verbose"),
            vec!["hooks/run.sh"]
        );
    }

    #[test]
    fn an_argument_keeps_a_backslash_the_shell_would_have_eaten() {
        // Nothing interprets a literal argument, so the byte the runtime
        // actually passes must survive. A shared unescape deleted it.
        assert_eq!(
            path_in_argument("${CLAUDE_PROJECT_DIR}/hooks\\run.sh"),
            Some("hooks\\run.sh".to_string())
        );
        assert_eq!(
            path_in_argument("${CLAUDE_PROJECT_DIR}/a\\ b/c.sh"),
            Some("a\\ b/c.sh".to_string()),
            "an argument is not shell-unescaped, so the backslash stays"
        );
    }

    #[test]
    fn a_glob_is_a_pattern_only_where_something_expands_it() {
        // Shell grammar: a pattern, so not one path.
        assert!(paths_in_command("bash ${CLAUDE_PROJECT_DIR}/hooks/*.sh").is_empty());
        // Literal grammar: `*` is a character in a filename.
        assert_eq!(
            path_in_argument("${CLAUDE_PROJECT_DIR}/hooks/*.sh"),
            Some("hooks/*.sh".to_string())
        );
    }

    #[test]
    fn an_argument_is_taken_whole() {
        // An `args` element is never shell-split, so a space is part of the
        // name and a prefix before the anchor is not part of the path.
        assert_eq!(
            path_in_argument("${CLAUDE_PROJECT_DIR}/my hooks/run.sh"),
            Some("my hooks/run.sh".to_string())
        );
        assert_eq!(
            path_in_argument("--config=${CLAUDE_PROJECT_DIR}/cfg.json"),
            Some("cfg.json".to_string())
        );
        assert_eq!(path_in_argument("post-format.sh"), None);
    }

    #[test]
    fn a_quoted_path_keeps_its_spaces() {
        assert_eq!(
            paths_in_command("bash \"${CLAUDE_PROJECT_DIR}/my hooks/run.sh\""),
            vec!["my hooks/run.sh"],
            "a quote delimits the token exactly, so the space is part of the name"
        );
    }

    #[test]
    fn a_pattern_is_not_a_path() {
        for source in [
            "${CLAUDE_PROJECT_DIR}/hooks/*.sh",
            "bash \"${CLAUDE_PROJECT_DIR}/hooks/check-?.sh\"",
            "${CLAUDE_PROJECT_DIR}/hooks/[ab].sh",
        ] {
            assert!(
                paths_in_command(source).is_empty(),
                "'{source}' is a pattern, not a path"
            );
        }
    }

    #[test]
    fn a_token_carrying_another_variable_is_not_a_path() {
        for source in [
            "${CLAUDE_PROJECT_DIR}/hooks/${NAME}.sh",
            "bash \"${CLAUDE_PROJECT_DIR}/hooks/${NAME}.sh\"",
            "bash ${CLAUDE_PROJECT_DIR}/hooks/${NAME}.sh",
            "${CLAUDE_PROJECT_DIR}/${SUBDIR}/x.sh",
        ] {
            assert!(
                paths_in_command(source).is_empty(),
                "'{source}' carries an unresolved variable"
            );
        }
    }

    #[test]
    fn both_anchor_spellings_resolve() {
        assert_eq!(
            paths_in_command("bash \"$CLAUDE_PROJECT_DIR/hooks/run.sh\""),
            vec!["hooks/run.sh"]
        );
        assert_eq!(
            paths_in_command("$CLAUDE_PROJECT_DIR/hooks/run.sh"),
            vec!["hooks/run.sh"]
        );
        assert_eq!(
            paths_in_command("a ${CLAUDE_PROJECT_DIR}/x.sh b $CLAUDE_PROJECT_DIR/y.sh"),
            vec!["x.sh", "y.sh"]
        );
    }

    #[test]
    fn a_backslash_escaped_space_does_not_end_the_token() {
        // The shell hands the runtime one argument with a real space in it, so
        // cutting at the escape would report a truncated name as missing.
        assert_eq!(
            paths_in_command("bash ${CLAUDE_PROJECT_DIR}/my\\ dir/x.sh"),
            vec!["my dir/x.sh"]
        );
        assert_eq!(
            paths_in_command("bash ${CLAUDE_PROJECT_DIR}/a\\ b/c.sh --flag"),
            vec!["a b/c.sh"]
        );
    }

    #[test]
    fn an_unanchored_command_yields_nothing() {
        for source in [
            "echo hi",
            "/usr/local/bin/verify",
            "npx some-linter --check",
            "bash \"$HOME/.local/bin/verify.sh\"",
        ] {
            assert!(paths_in_command(source).is_empty(), "'{source}'");
        }
    }

    #[test]
    fn scanning_terminates_on_pathological_input() {
        for source in [
            "${CLAUDE_PROJECT_DIR}/",
            "${CLAUDE_PROJECT_DIR}/${CLAUDE_PROJECT_DIR}/",
            "a ${CLAUDE_PROJECT_DIR}/x ${CLAUDE_PROJECT_DIR}/y",
            "${CLAUDE_PROJECT_DIR}",
        ] {
            let _ = paths_in_command(source);
        }
    }

    #[test]
    fn non_ascii_paths_do_not_panic_on_a_char_boundary() {
        assert_eq!(
            paths_in_command("bash \"${CLAUDE_PROJECT_DIR}/훅/실행.sh\" 인자"),
            vec!["훅/실행.sh"]
        );
    }
}
