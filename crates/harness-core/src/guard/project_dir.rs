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
//! - Never read a single-quoted variable as a path. The shell hands the
//!   runtime the dollar sign itself, so `'$CLAUDE_PROJECT_DIR'/x` names no
//!   file this crate can resolve, and treating it as one would answer for a
//!   string the runtime never expands.
//! - Never guess whether a quoted word carrying spaces is one long filename or
//!   a nested command. `-c` settles it, and failing that a second anchor does,
//!   because one path cannot hold the project root twice. Nothing else does.
//! - Never re-lex a second level of shell. Inside `sh -c "… \"a b\" …"` the
//!   inner shell would re-quote and recover `a b`; this module reads one level
//!   and stops, so such a token truncates at the space. Truncation only ever
//!   loses a match — a shortened token cannot collide with an artifact whose
//!   name has no space in it — and modelling nested quoting is how a parser
//!   starts answering for strings it cannot see.
//! - Never treat non-ASCII whitespace as a word separator's equal. `-c` is
//!   found by ASCII-shaped splitting, so a no-break space around it reads as a
//!   separator where the shell reads a letter. It errs toward splitting, which
//!   is again the missing-match direction.
//! - Never infer the grammar from the string. The caller knows the handler's
//!   shape and says so by choosing the function — and every step below the
//!   entry points is tagged the same way, because a shared unescape is how one
//!   grammar's rule leaks into the other's answer.

/// The documented project-root variable spellings. Both braced and bare are
/// valid in a hook command, so a check that read only one would be blind to
/// half the harnesses in the wild.
///
/// The `/` separator is deliberately not part of these: `"$CLAUDE_PROJECT_DIR"/x`
/// closes the quote between the variable and the separator, and a constant
/// that welds the two together silently skips the more idiomatic of the two
/// shell spellings — quoting the variable rather than the whole word.
pub const ANCHORS: &[&str] = &["${CLAUDE_PROJECT_DIR}", "$CLAUDE_PROJECT_DIR"];

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
    scan(source, Quoting::Bare, false)
}

/// [`paths_in_command`], able to start inside a quote and inside a command.
///
/// Both facts have to cross the recursion. The quoting, because the interior
/// of a command string is still inside it — starting over in `Bare` lets the
/// apostrophe in `echo it's fine` open a quote that never closes. And
/// `commanded`, because the `-c` that proved this region is a command was read
/// at the outer level: without it the last anchored word of the region reverts
/// to the one-filename reading and swallows whatever follows it, so
/// `sh -c "…/_runner.sh fmt && …/pre-commit --check"` loses `hooks/pre-commit`.
fn scan(source: &str, initial: Quoting, commanded: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(occurrence) = next_occurrence(source, cursor, initial) {
        let tail = &source[occurrence.path_start..];
        let end = match occurrence.quote {
            Some(q) => tail.find(q).unwrap_or(tail.len()),
            None => unquoted_end(tail),
        };
        let region = &tail[..end];
        // A quoted word is either one filename that may contain spaces or a
        // whole command string, and the string says which. `sh -c` says it
        // outright; failing that, a second anchor says it, because one path
        // cannot hold the project root twice. Absent both, it is a filename.
        let commanded = commanded || occurrence.nested;
        if occurrence.quote.is_some() && (commanded || contains_variable(region)) {
            let first = unquoted_end(region);
            out.extend(shell_token(&region[..first]));
            out.extend(scan(&region[first..], Quoting::Double, commanded));
        } else {
            out.extend(shell_token(region));
        }
        cursor = occurrence.path_start + end;
    }
    out
}

/// An anchored path in a shell-interpreted string: where the path begins, the
/// quote it must run to the end of, and whether that quote holds a command.
struct Occurrence {
    path_start: usize,
    quote: Option<char>,
    nested: bool,
}

/// Whether the word before the quote at `quote_at` makes the quoted string a
/// command rather than an operand.
///
/// `-c` is the flag every shell uses for exactly that, so this reads a stated
/// fact rather than inferring one from what the quotes contain. A tool whose
/// `-c` means something else (`grep -c`) costs nothing: its operand is a
/// pattern, and a pattern is not a path either way.
///
/// The flag has to be its own word. A suffix test also accepts a binary whose
/// *name* ends in `-c` (`fmt-c "…/my hooks/run.sh"`), and would then cut a
/// legitimately quoted filename at its first space.
fn introduces_a_command(source: &str, quote_at: usize) -> bool {
    matches!(
        source[..quote_at]
            .trim_end()
            .rsplit(char::is_whitespace)
            .next(),
        Some("-c")
    )
}

/// Which quoting a byte of the source sits under. Carried across the whole
/// scan because the character before a variable does not determine this: in
/// `'a && $CLAUDE_PROJECT_DIR/b'` that character is a space while the variable
/// is inside single quotes and never expands at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quoting {
    Bare,
    Single,
    Double,
}

/// The earliest anchored path at or after `from`.
///
/// The scan always starts at byte zero, whatever `from` is, because quoting is
/// a property of everything to the left — resuming mid-string would forget
/// which quote is open. A variable is an anchor only when a `/` follows it,
/// either directly or across the quote that closed it.
fn next_occurrence(source: &str, from: usize, initial: Quoting) -> Option<Occurrence> {
    let mut state = initial;
    let mut escaped = false;
    let mut double_open: Option<usize> = None;

    for (i, ch) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (state, ch) {
            (Quoting::Bare | Quoting::Double, '\\') => {
                escaped = true;
                continue;
            }
            (Quoting::Bare, '\'') => {
                state = Quoting::Single;
                continue;
            }
            (Quoting::Single, '\'') => {
                state = Quoting::Bare;
                continue;
            }
            (Quoting::Bare, '"') => {
                state = Quoting::Double;
                double_open = Some(i);
                continue;
            }
            (Quoting::Double, '"') => {
                state = Quoting::Bare;
                double_open = None;
                continue;
            }
            // Nothing expands inside single quotes, so the runtime receives
            // the dollar sign itself and no path is named here.
            (Quoting::Single, _) => continue,
            _ => {}
        }
        if i < from || ch != '$' {
            continue;
        }
        let Some(variable) = ANCHORS.iter().find(|a| source[i..].starts_with(**a)) else {
            continue;
        };
        let after = i + variable.len();
        let rest = &source[after..];

        // `"$VAR"/path` closes the quote around the variable alone, so the
        // path that follows it is bare and ends where the shell ends a word.
        if state == Quoting::Double && rest.starts_with("\"/") {
            return Some(Occurrence {
                path_start: after + 2,
                quote: None,
                nested: false,
            });
        }
        if rest.starts_with('/') {
            return Some(Occurrence {
                path_start: after + 1,
                quote: (state == Quoting::Double).then_some('"'),
                nested: double_open.is_some_and(|q| introduces_a_command(source, q)),
            });
        }
    }
    None
}

fn contains_variable(source: &str) -> bool {
    ANCHORS.iter().any(|a| source.contains(a))
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
    // No shell reads an `args` element, so a quote here is a character in a
    // filename rather than a delimiter: only the bare separator forms anchor.
    //
    // Every occurrence is tried, not just the earliest. A mention of the root
    // that is not followed by a separator (`--prefix=$CLAUDE_PROJECT_DIR:…`)
    // names no path, and stopping there would lose the real one later in the
    // same argument.
    let mut from = 0usize;
    while from < value.len() {
        let (start, variable) = ANCHORS
            .iter()
            .filter_map(|a| value[from..].find(a).map(|i| (from + i, *a)))
            .min_by_key(|(i, a)| (*i, std::cmp::Reverse(a.len())))?;
        let after = start + variable.len();
        if let Some(path) = value[after..].strip_prefix('/') {
            return literal_token(path);
        }
        from = after;
    }
    None
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
    fn quoting_the_variable_anchors_as_well_as_quoting_the_word() {
        // The idiomatic shell spelling. Welding `/` onto the variable made
        // these invisible, so a hook wired this way was never checked at all.
        for source in [
            "bash \"$CLAUDE_PROJECT_DIR\"/hooks/_runner.sh session-start.sh",
            "bash \"${CLAUDE_PROJECT_DIR}\"/hooks/_runner.sh session-start.sh",
            "\"$CLAUDE_PROJECT_DIR\"/hooks/_runner.sh",
        ] {
            assert_eq!(
                paths_in_command(source),
                vec!["hooks/_runner.sh"],
                "'{source}'"
            );
        }
    }

    #[test]
    fn a_nested_command_yields_every_path_it_names() {
        // `bash -c "…"` is a command inside a word. One filename cannot hold
        // the project root twice, so the second anchor is what proves the
        // quoted region is not a single long name.
        assert_eq!(
            paths_in_command(
                "bash -c \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh fmt && ${CLAUDE_PROJECT_DIR}/hooks/pre-commit\""
            ),
            vec!["hooks/_runner.sh", "hooks/pre-commit"]
        );
    }

    #[test]
    fn a_single_quoted_variable_is_a_literal_not_a_path() {
        // The shell hands the runtime the dollar sign itself, so nothing here
        // can say which file this names. The last case is why quoting is
        // tracked across the whole string rather than read off the character
        // before the variable: there that character is a space, and the
        // variable is still inside the quotes opened much earlier.
        for source in [
            "bash '$CLAUDE_PROJECT_DIR'/hooks/run.sh",
            "bash '${CLAUDE_PROJECT_DIR}/hooks/run.sh'",
            "bash -c '${CLAUDE_PROJECT_DIR}/a.sh && ${CLAUDE_PROJECT_DIR}/b.sh'",
        ] {
            assert!(paths_in_command(source).is_empty(), "'{source}'");
        }
    }

    #[test]
    fn an_argument_mentioning_the_root_twice_still_yields_its_path() {
        // The first mention names no path — no separator follows it — and
        // giving up there loses the real one later in the same argument.
        assert_eq!(
            path_in_argument("--prefix=$CLAUDE_PROJECT_DIR:${CLAUDE_PROJECT_DIR}/hooks/x.sh"),
            Some("hooks/x.sh".to_string())
        );
        assert_eq!(
            path_in_argument("$CLAUDE_PROJECT_DIR ${CLAUDE_PROJECT_DIR}/hooks/x.sh"),
            Some("hooks/x.sh".to_string())
        );
    }

    #[test]
    fn an_apostrophe_inside_a_command_string_is_a_letter() {
        // Inside double quotes `'` is not a quote opener. Lexing the interior
        // from a fresh `Bare` state opens a quote that never closes and every
        // anchor after it vanishes — and `don't` in an `echo` is ordinary.
        assert_eq!(
            paths_in_command(
                "bash -c \"${CLAUDE_PROJECT_DIR}/hooks/a.sh && echo it's fine && ${CLAUDE_PROJECT_DIR}/hooks/b.sh\""
            ),
            vec!["hooks/a.sh", "hooks/b.sh"]
        );
        assert_eq!(
            paths_in_command(
                "bash -c \"${CLAUDE_PROJECT_DIR}/hooks/a.sh; don't; ${CLAUDE_PROJECT_DIR}/hooks/pre-commit\""
            ),
            vec!["hooks/a.sh", "hooks/pre-commit"]
        );
    }

    #[test]
    fn a_command_string_naming_one_path_is_still_a_command() {
        // The commoner `-c` shape. With only one anchor there is no second
        // one to prove the region is a command, so the flag has to say it —
        // otherwise the argument list is read as part of the filename.
        assert_eq!(
            paths_in_command("bash -c \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh post-format.sh\""),
            vec!["hooks/_runner.sh"]
        );
        assert_eq!(
            paths_in_command("sh -c \"${CLAUDE_PROJECT_DIR}/hooks/pre-commit || true\""),
            vec!["hooks/pre-commit"]
        );
        // Without the flag the same quoted word is one name, spaces included.
        assert_eq!(
            paths_in_command("bash \"${CLAUDE_PROJECT_DIR}/my hooks/run.sh\""),
            vec!["my hooks/run.sh"]
        );
    }

    #[test]
    fn the_command_fact_survives_the_recursion() {
        // The `-c` is read at the outer level. Dropping it on the way in makes
        // the last anchored word of the region revert to the one-filename
        // reading, so a chained hook loses its final path.
        for source in [
            "bash -c \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh ${CLAUDE_PROJECT_DIR}/hooks/pre-commit --check\"",
            "bash -c \"${CLAUDE_PROJECT_DIR}/hooks/_runner.sh fmt && ${CLAUDE_PROJECT_DIR}/hooks/pre-commit && echo x\"",
        ] {
            assert_eq!(
                paths_in_command(source),
                vec!["hooks/_runner.sh", "hooks/pre-commit"],
                "'{source}'"
            );
        }
    }

    #[test]
    fn a_later_word_does_not_inherit_the_command_fact() {
        // `commanded` travels down into the region the flag proved, never
        // sideways to the next one. The whole guarantee is that the binding
        // is scoped to one iteration: hoisted above the loop it would latch,
        // and every quoted filename after a `-c` would be cut at its space.
        for source in [
            "bash -c \"${CLAUDE_PROJECT_DIR}/a.sh\" \"${CLAUDE_PROJECT_DIR}/my file.sh\"",
            "bash -c \"${CLAUDE_PROJECT_DIR}/a.sh\" ; cp \"${CLAUDE_PROJECT_DIR}/my file.sh\" /tmp",
            "sh -c \"${CLAUDE_PROJECT_DIR}/a.sh\" && cat \"${CLAUDE_PROJECT_DIR}/my file.sh\"",
        ] {
            assert_eq!(
                paths_in_command(source),
                vec!["a.sh", "my file.sh"],
                "'{source}'"
            );
        }
    }

    #[test]
    fn only_the_flag_itself_marks_a_command_string() {
        // A binary whose name merely ends in `-c` is not the flag, and reading
        // it as one cuts a legitimately quoted filename at its first space.
        for source in [
            "/usr/local/bin/fmt-c \"${CLAUDE_PROJECT_DIR}/my hooks/run.sh\"",
            "/usr/local/bin/fmt \"${CLAUDE_PROJECT_DIR}/my hooks/run.sh\"",
        ] {
            assert_eq!(
                paths_in_command(source),
                vec!["my hooks/run.sh"],
                "'{source}'"
            );
        }
    }

    #[test]
    fn a_quote_that_closed_leaves_the_rest_bare() {
        // Adjacent-quote concatenation. Reading the closing `'` as an opening
        // one ran the token past the `;` and named a file with a command in it.
        assert_eq!(
            paths_in_command("bash 'pre'${CLAUDE_PROJECT_DIR}/x.sh; echo done"),
            vec!["x.sh"]
        );
    }

    #[test]
    fn a_nested_command_survives_escapes_and_repeated_separators() {
        assert_eq!(
            paths_in_command(
                "bash -c \"${CLAUDE_PROJECT_DIR}/a\\ b.sh && ${CLAUDE_PROJECT_DIR}/c.sh\""
            ),
            vec!["a b.sh", "c.sh"],
            "an escaped space belongs to the first name, not to the separator"
        );
        assert_eq!(
            paths_in_command(
                "bash -c \"${CLAUDE_PROJECT_DIR}/a.sh; ${CLAUDE_PROJECT_DIR}/b.sh; ${CLAUDE_PROJECT_DIR}/c.sh\""
            ),
            vec!["a.sh", "b.sh", "c.sh"]
        );
        assert_eq!(
            paths_in_command("bash \"${CLAUDE_PROJECT_DIR}/x.sh\" \"${CLAUDE_PROJECT_DIR}/y.sh\""),
            vec!["x.sh", "y.sh"],
            "two separately quoted words are two names, not one nested command"
        );
    }

    #[test]
    fn every_short_combination_of_shell_atoms_terminates() {
        // The scan advances on a cursor and recurses on quoted regions, so
        // both have to be shown to make progress on input built to defeat
        // them. A hang here would freeze whatever hook fired the audit.
        let atoms = [
            "${CLAUDE_PROJECT_DIR}",
            "$CLAUDE_PROJECT_DIR",
            "/",
            "\"",
            "'",
            "\\",
            " ",
            "&&",
            "x.sh",
            "$",
            "*",
            ";",
            "훅",
        ];
        for a in atoms {
            for b in atoms {
                for c in atoms {
                    for d in atoms {
                        let source = format!("{a}{b}{c}{d}");
                        let _ = paths_in_command(&source);
                        let _ = path_in_argument(&source);
                    }
                }
            }
        }
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
