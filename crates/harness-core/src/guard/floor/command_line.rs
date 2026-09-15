//! Quote-aware splitting of a shell command line into simple commands.
//!
//! Argv is reconstructed as the shell assembles it — quotes stripped,
//! escapes resolved, ANSI-C bodies decoded — because the bypass check reads
//! the words git receives, not the bytes the operator typed. A stripped
//! `$'\x6e'` would read as the literal `x6e` while the shell delivers `n`,
//! which is a silent pass rather than a visible skip.
//!
//! Parsing boundaries (deliberate, all fail-safe — the caller turns a
//! [`SplitError`] into a visible skip, never a block or a silent pass):
//!
//! - Unterminated quoting and an ANSI-C code point the shell cannot deliver
//!   are [`SplitError`]s.
//! - `$(…)` / backtick substitution text stays inside its enclosing word and
//!   is not re-parsed.
//! - Redirections are read as the shell reads them (maximal munch, optional
//!   fd prefix): the operator terminates the current word and its target is
//!   dropped, so `2>&1` binds as one redirection rather than splitting at its
//!   `&`, and `--no-verify>log` reads as a flag plus a redirection rather
//!   than one opaque word.
//! - A heredoc body is read as the shell reads it, which its delimiter
//!   decides. Quoting any part of the delimiter (`<<'EOF'`) suppresses every
//!   expansion, so the body reaches the command's stdin as data and the scan
//!   steps over it — that is how a document *about* a bypass is written. An
//!   unquoted `<<EOF` keeps `$(…)` live, so its body stays in the scan, and
//!   with each newline a separator a prose line beginning `git commit
//!   --no-verify` there still false-blocks: a surfaced block on a document
//!   whose author can quote the delimiter, never a silent pass. The skip
//!   rests on the delimiter alone, never on guessing which lines look like
//!   prose, and a delimiter that never arrives ends the body at the end of
//!   input, exactly where the shell ends it.

use std::fmt;

/// The command line could not be read as the shell would read it. The caller
/// fails open with a visible skip note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitError {
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    UnterminatedAnsiCQuote,
    UnterminatedSubstitution,
    UnterminatedBacktick,
    /// A code point the shell cannot deliver as a character — past the
    /// Unicode maximum, or a surrogate no Rust string can carry.
    AnsiCCodePoint,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::UnterminatedSingleQuote => "unterminated single quote",
            Self::UnterminatedDoubleQuote => "unterminated double quote",
            Self::UnterminatedAnsiCQuote => "unterminated ANSI-C quote",
            Self::UnterminatedSubstitution => "unterminated command substitution",
            Self::UnterminatedBacktick => "unterminated backtick substitution",
            Self::AnsiCCodePoint => "ANSI-C code point the shell cannot deliver",
        };
        f.write_str(text)
    }
}

/// Redirection operators, longest first so the scan is maximal-munch. `|&` is
/// deliberately absent — it is a *pipeline* operator, and the `|` separator
/// branch is what splits `foo |& git commit --no-verify` into two simple
/// commands.
const REDIRECTION_OPERATORS: [&str; 12] = [
    "&>>", "<<<", "<<-", ">>", ">&", "<&", ">|", "<>", "&>", "<<", ">", "<",
];

/// A file-descriptor prefix bound to a redirection operator: a digit run
/// (`2>`) or bash's varname allocation form (`{fd}>`). Both belong to the
/// redirection, so neither may be read as the command word or as an argument.
fn is_fd_prefix(word: &str) -> bool {
    if !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let Some(inner) = word.strip_prefix('{').and_then(|w| w.strip_suffix('}')) else {
        return false;
    };
    let mut chars = inner.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn redirection_at(input: &str, i: usize) -> Option<&'static str> {
    REDIRECTION_OPERATORS
        .into_iter()
        .find(|op| input[i..].starts_with(op))
}

/// A heredoc the line has opened, waiting for the body that follows the
/// newline.
struct Heredoc {
    /// The delimiter after quote removal, as the shell compares it against
    /// each body line.
    delimiter: String,
    /// `<<-`, which strips leading tabs from the body and the delimiter line.
    strip_tabs: bool,
    /// Whether any part of the delimiter word was quoted, which is what
    /// decides whether the body is data or text the shell still expands.
    quoted: bool,
}

/// Whether `redirection` opens a heredoc, and whether it strips leading tabs.
fn heredoc_operator(redirection: &str) -> Option<bool> {
    match redirection {
        "<<" => Some(false),
        "<<-" => Some(true),
        _ => None,
    }
}

/// Characters that end the delimiter word unquoted: the shell's own
/// metacharacters, past which the word belongs to the next construct.
const DELIMITER_TERMINATORS: &[u8] = b";&|<>()";

/// Read the delimiter word at `start` (just past the operator), returning the
/// heredoc it opens and the byte index past the word.
fn read_heredoc_delimiter(
    input: &str,
    start: usize,
    strip_tabs: bool,
) -> Result<(Heredoc, usize), SplitError> {
    let bytes = input.as_bytes();
    let mut i = start;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let mut delimiter = String::new();
    let mut quoted = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                quoted = true;
                let end = input[i + 1..]
                    .find('\'')
                    .ok_or(SplitError::UnterminatedSingleQuote)?;
                delimiter.push_str(&input[i + 1..i + 1 + end]);
                i += end + 2;
            }
            b'"' => {
                quoted = true;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\'
                        && matches!(bytes.get(i + 1), Some(b'"' | b'\\' | b'$' | b'`'))
                    {
                        i += 1;
                    }
                    let ch = input[i..].chars().next().expect("in-bounds char");
                    delimiter.push(ch);
                    i += ch.len_utf8();
                }
                if i >= bytes.len() {
                    return Err(SplitError::UnterminatedDoubleQuote);
                }
                i += 1;
            }
            b'\\' if i + 1 < bytes.len() => {
                quoted = true;
                let ch = input[i + 1..].chars().next().expect("in-bounds char");
                delimiter.push(ch);
                i += 1 + ch.len_utf8();
            }
            b if b.is_ascii_whitespace() || DELIMITER_TERMINATORS.contains(&b) => break,
            _ => {
                let ch = input[i..].chars().next().expect("in-bounds char");
                delimiter.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Ok((
        Heredoc {
            delimiter,
            strip_tabs,
            quoted,
        },
        i,
    ))
}

/// Advance past the bodies `queued` on the line just ended.
///
/// A quoted delimiter makes its body data the shell never runs, so the scan
/// steps over it. An unquoted one leaves the body — and every body queued
/// behind it, since the scan no longer knows where they begin — in the scan.
fn consume_heredoc_bodies(input: &str, mut at: usize, queued: &mut Vec<Heredoc>) -> usize {
    for heredoc in queued.drain(..) {
        if !heredoc.quoted {
            break;
        }
        at = skip_heredoc_body(input, at, &heredoc);
    }
    at
}

/// The index past `heredoc`'s body: the start of the line after its delimiter
/// line, or the end of input where the delimiter never arrives, which is
/// where the shell ends that body too.
fn skip_heredoc_body(input: &str, start: usize, heredoc: &Heredoc) -> usize {
    let mut at = start;
    while at < input.len() {
        let newline = input[at..].find('\n').map(|offset| at + offset);
        let line = &input[at..newline.unwrap_or(input.len())];
        at = newline.map_or(input.len(), |index| index + 1);
        let candidate = if heredoc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if candidate == heredoc.delimiter {
            return at;
        }
    }
    input.len()
}

/// Decode a `$'…'` body starting at `start` (just past the opening quote),
/// returning the decoded text and the byte index of the closing quote.
///
/// A backslash escapes the next character, so a `\'` does not close the
/// quote — scanning for the quote alone would end the word early and leave
/// the rest of the line to be re-parsed as something else.
fn decode_ansi_c_quote(input: &str, start: usize) -> Result<(String, usize), SplitError> {
    let bytes = input.as_bytes();
    let mut value = String::new();
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'\'' {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            let ch = input[i..].chars().next().expect("in-bounds char");
            value.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let next = input[i + 1..].chars().next().expect("in-bounds char");
        if let Some(simple) = ansi_c_escape(next) {
            value.push(simple);
            i += 1 + next.len_utf8();
            continue;
        }
        if let Some((decoded, consumed)) = decode_numeric_escape(&input[i + 1..])? {
            value.push(decoded);
            i += 1 + consumed;
            continue;
        }
        // An unrecognised escape keeps its backslash, exactly as the shell
        // does. `\cX` control escapes land here: a control character can never
        // spell a flag letter, and the retained backslash keeps the word out
        // of every flag test.
        value.push('\\');
        value.push(next);
        i += 1 + next.len_utf8();
    }
    if i >= bytes.len() {
        return Err(SplitError::UnterminatedAnsiCQuote);
    }
    // The shell assembles argv as C strings, so a NUL ends the argument and
    // nothing after it reaches the command. Carrying the tail would leave the
    // word longer than the one git receives, and a longer word only
    // under-matches.
    if let Some(nul) = value.find('\0') {
        value.truncate(nul);
    }
    Ok((value, i))
}

/// Single-character ANSI-C escapes, as the shell decodes them.
fn ansi_c_escape(c: char) -> Option<char> {
    Some(match c {
        'a' => '\x07',
        'b' => '\x08',
        'e' | 'E' => '\x1b',
        'f' => '\x0c',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\x0b',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        '?' => '?',
        _ => return None,
    })
}

/// A numeric ANSI-C escape at the start of `rest` (past the backslash):
/// `x`+hex, `u`/`U`+hex, or octal digits. Returns the decoded character and
/// the bytes consumed, or `None` when `rest` starts no numeric escape.
fn decode_numeric_escape(rest: &str) -> Result<Option<(char, usize)>, SplitError> {
    let bytes = rest.as_bytes();
    let (radix_len, max_digits, is_octal) = match bytes.first() {
        Some(b'x') => (1, 2, false),
        Some(b'u') => (1, 4, false),
        Some(b'U') => (1, 8, false),
        Some(b) if (b'0'..=b'7').contains(b) => (0, 3, true),
        _ => return Ok(None),
    };
    let digits: usize = bytes[radix_len..]
        .iter()
        .take(max_digits)
        .take_while(|b| {
            if is_octal {
                (b'0'..=b'7').contains(b)
            } else {
                b.is_ascii_hexdigit()
            }
        })
        .count();
    if digits == 0 {
        return Ok(None);
    }
    let text = &rest[radix_len..radix_len + digits];
    let code = u32::from_str_radix(text, if is_octal { 8 } else { 16 }).expect("bounded digits");
    // Past the Unicode maximum — or a surrogate, which no Rust string can
    // carry — is not a character the shell can deliver; treating it as
    // unparseable keeps the failure on the visible skip path rather than
    // surfacing as an internal fault.
    let decoded = char::from_u32(code).ok_or(SplitError::AnsiCCodePoint)?;
    Ok(Some((decoded, radix_len + digits)))
}

/// The argv under assembly. A redirection's target is consumed rather than
/// pushed, and an fd prefix touching its operator is part of the redirection
/// rather than a word of its own.
#[derive(Default)]
struct Accumulator {
    commands: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    has_word: bool,
    /// The next word is a redirection target (a filename or fd), not an argument.
    drop_next_word: bool,
}

impl Accumulator {
    fn push_char(&mut self, c: char) {
        self.word.push(c);
        self.has_word = true;
    }

    fn push_str(&mut self, s: &str) {
        self.word.push_str(s);
        self.has_word = true;
    }

    fn push_word(&mut self) {
        if !self.has_word {
            return;
        }
        if self.drop_next_word {
            self.drop_next_word = false;
            self.word.clear();
        } else {
            self.words.push(std::mem::take(&mut self.word));
        }
        self.has_word = false;
    }

    fn push_command(&mut self) {
        self.push_word();
        self.drop_next_word = false;
        if !self.words.is_empty() {
            self.commands.push(std::mem::take(&mut self.words));
        }
    }

    /// A pending word touching the operator is the fd prefix — a digit run
    /// (`2>&1`) or a varname allocation (`{fd}>…`) — part of the redirection
    /// rather than an argument, and never the command word. A word separated
    /// by whitespace is an argument and stays (`echo 2 > f`).
    fn start_redirection(&mut self) {
        if self.has_word && is_fd_prefix(&self.word) {
            self.word.clear();
            self.has_word = false;
        } else {
            self.push_word();
        }
        self.drop_next_word = true;
    }

    /// The redirection's target was read by the scan itself, as a heredoc
    /// delimiter is, so no later word is that target.
    fn target_consumed(&mut self) {
        self.drop_next_word = false;
    }
}

/// Split a shell command line into simple commands (each a word list).
///
/// Handles single/double quotes and backslash escapes; treats unquoted
/// `&& || ; | & \n ( )` and a brace-group `{` (a standalone `{` followed by
/// whitespace) as command separators; keeps `$(…)` / backtick substitution
/// text inside the enclosing word (module boundary note).
pub fn split_commands(input: &str) -> Result<Vec<Vec<String>>, SplitError> {
    let bytes = input.as_bytes();
    let mut acc = Accumulator::default();
    let mut heredocs: Vec<Heredoc> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            if i + 1 < bytes.len() {
                // Backslash-newline is a line continuation: the shell removes
                // both characters before word splitting, so `--no-ver\⏎ify`
                // reaches git as one flag. Keeping the newline would leave a
                // token no check recognises while the shell still assembles
                // the flag.
                if bytes[i + 1] == b'\n' {
                    i += 2;
                    continue;
                }
                let ch = input[i + 1..].chars().next().expect("in-bounds char");
                acc.push_char(ch);
                i += 1 + ch.len_utf8();
            } else {
                i += 1;
            }
            continue;
        }
        if b == b'\'' {
            let end = input[i + 1..]
                .find('\'')
                .ok_or(SplitError::UnterminatedSingleQuote)?;
            acc.push_str(&input[i + 1..i + 1 + end]);
            i += end + 2;
            continue;
        }
        if b == b'"' {
            i += 1;
            let mut buf = String::new();
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
                    i += 2; // line continuation — removed inside double quotes too
                } else if bytes[i] == b'\\'
                    && matches!(bytes.get(i + 1), Some(b'"' | b'\\' | b'$' | b'`'))
                {
                    buf.push(bytes[i + 1] as char);
                    i += 2;
                } else {
                    let ch = input[i..].chars().next().expect("in-bounds char");
                    buf.push(ch);
                    i += ch.len_utf8();
                }
            }
            if i >= bytes.len() {
                return Err(SplitError::UnterminatedDoubleQuote);
            }
            i += 1;
            acc.push_str(&buf);
            continue;
        }
        if b == b'$' && bytes.get(i + 1) == Some(&b'\'') {
            // ANSI-C quoting: the `$` introduces the quote rather than joining
            // the word, so the body decodes to the flag itself rather than to
            // a token beginning with `$`.
            let (decoded, end) = decode_ansi_c_quote(input, i + 2)?;
            acc.push_str(&decoded);
            i = end + 1;
            continue;
        }
        if b == b'$' && bytes.get(i + 1) == Some(&b'"') {
            // Locale-translation quoting: `$"…"` is a double-quoted string the
            // shell would translate, and with no catalogue it delivers the
            // string verbatim — so the body is the flag, exactly as `$'…'` is.
            // The `$` introduces the quote; drop it and read the double-quote
            // body from its opening quote.
            i += 1;
            continue;
        }
        if b == b'$' && bytes.get(i + 1) == Some(&b'(') {
            let mut depth = 1;
            let mut j = i + 2;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            if depth > 0 {
                return Err(SplitError::UnterminatedSubstitution);
            }
            acc.push_str(&input[i..j]);
            i = j;
            continue;
        }
        if b == b'`' {
            let end = input[i + 1..]
                .find('`')
                .ok_or(SplitError::UnterminatedBacktick)?;
            acc.push_str(&input[i..i + end + 2]);
            i += end + 2;
            continue;
        }
        if let Some(redirection) = redirection_at(input, i) {
            acc.start_redirection();
            i += redirection.len();
            if let Some(strip_tabs) = heredoc_operator(redirection) {
                let (heredoc, past_delimiter) = read_heredoc_delimiter(input, i, strip_tabs)?;
                acc.target_consumed();
                heredocs.push(heredoc);
                i = past_delimiter;
            }
            continue;
        }
        if matches!(b, b'\n' | b';' | b'(' | b')') {
            acc.push_command();
            i += 1;
            if b == b'\n' {
                i = consume_heredoc_bodies(input, i, &mut heredocs);
            }
            continue;
        }
        if b == b'&' || b == b'|' {
            acc.push_command();
            i += 1;
            if bytes.get(i) == Some(&b) {
                i += 1;
            }
            continue;
        }
        if b == b'#' && !acc.has_word {
            // `#` opens a comment only at a word boundary (`echo a #c` comments,
            // `echo a#b` is one word) — the shell drops the rest of the line, so
            // a flag mentioned past it never reaches argv. The comment runs to
            // the newline, which stays a separator: leaving `i` on it lets the
            // newline arm close the command already accumulated.
            i += input[i..].find('\n').unwrap_or(input.len() - i);
            continue;
        }
        if b == b'{'
            && !acc.has_word
            && bytes
                .get(i + 1)
                .is_none_or(|next| next.is_ascii_whitespace())
        {
            // A brace-group `{` opens a fresh command wherever a command word is
            // expected: at a boundary the splitter already made, and after the
            // reserved-word prefixes `time` / `!` that keep that position. The
            // `!has_word` guard alone splits there and also, deliberately, at an
            // ordinary argument `{` (`printf %s { x`) — a false block on an
            // unusual line, the safe direction, taken so no `time { git commit
            // --no-verify; }` group is ever missed. `{}` (a `find -exec`
            // placeholder), `${VAR}` and `a{1,2}` glue the brace to a word, which
            // the trailing-whitespace and `has_word` guards keep. `}` is never a
            // separator: a group close is always preceded by the `;` / newline
            // that already split, so a bare `}` (`echo } x`) is a literal word.
            acc.push_command();
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            // The shell splits words on IFS, whose default is space / tab /
            // newline — ASCII, deliberately. A non-breaking space is an ordinary
            // word character to the shell, so treating it as a separator would
            // split a word the shell keeps whole; ASCII-only matches bash.
            acc.push_word();
            i += 1;
            continue;
        }
        let ch = input[i..].chars().next().expect("in-bounds char");
        acc.push_char(ch);
        i += ch.len_utf8();
    }
    acc.push_command();
    Ok(acc.commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(input: &str) -> Vec<Vec<String>> {
        split_commands(input).expect("splits")
    }

    fn owned(words: &[&[&str]]) -> Vec<Vec<String>> {
        words
            .iter()
            .map(|w| w.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn splits_words_on_unquoted_whitespace() {
        assert_eq!(split("git  status\t-s"), owned(&[&["git", "status", "-s"]]));
    }

    #[test]
    fn keeps_quoted_spans_as_one_word_with_quotes_stripped() {
        assert_eq!(
            split("git commit -m 'a b' -m \"c d\""),
            owned(&[&["git", "commit", "-m", "a b", "-m", "c d"]])
        );
    }

    #[test]
    fn splits_commands_on_every_separator() {
        assert_eq!(
            split("a && b || c; d | e\nf & g"),
            owned(&[&["a"], &["b"], &["c"], &["d"], &["e"], &["f"], &["g"]])
        );
    }

    #[test]
    fn keeps_command_substitution_inside_the_enclosing_word() {
        assert_eq!(
            split("echo $(git commit --no-verify)"),
            owned(&[&["echo", "$(git commit --no-verify)"]])
        );
        assert_eq!(split("echo `git log`"), owned(&[&["echo", "`git log`"]]));
    }

    #[test]
    fn opens_a_fresh_command_at_a_brace_group_but_keeps_every_glued_brace() {
        assert_eq!(
            split("{ git commit --no-verify; }"),
            owned(&[&["git", "commit", "--no-verify"], &["}"]])
        );
        assert_eq!(
            split("find . -exec rm {} +"),
            owned(&[&["find", ".", "-exec", "rm", "{}", "+"]])
        );
        assert_eq!(split("echo ${VAR}"), owned(&[&["echo", "${VAR}"]]));
        assert_eq!(split("echo a{1,2}"), owned(&[&["echo", "a{1,2}"]]));
    }

    #[test]
    fn treats_a_standalone_close_brace_as_a_literal_word() {
        assert_eq!(split("echo } x"), owned(&[&["echo", "}", "x"]]));
    }

    #[test]
    fn a_brace_at_a_word_boundary_opens_a_group_including_after_time_and_bang() {
        // The reserved-word prefixes `time` / `!` keep command position, so the
        // group — and the git command inside it — must be seen. Missing it would
        // be a silent bypass; the splitter opens a fresh command at the `{`.
        assert_eq!(
            split("time { git commit --no-verify; }"),
            owned(&[&["time"], &["git", "commit", "--no-verify"], &["}"]])
        );
        assert_eq!(
            split("! { git commit; }"),
            owned(&[&["!"], &["git", "commit"], &["}"]])
        );
        assert_eq!(
            split("cmd; { git commit; }"),
            owned(&[&["cmd"], &["git", "commit"], &["}"]])
        );
        // A `{` used as an ordinary argument splits too — a false command on an
        // unusual line, taken as the safe cost of never missing a group.
        assert_eq!(split("printf %s { x"), owned(&[&["printf", "%s"], &["x"]]));
    }

    #[test]
    fn a_hash_at_a_word_boundary_opens_a_comment_to_end_of_line() {
        assert_eq!(
            split("git push origin main # never --no-verify"),
            owned(&[&["git", "push", "origin", "main"]])
        );
        // Comment ends at the newline, which still separates the next command.
        assert_eq!(
            split("echo a # c\ngit status"),
            owned(&[&["echo", "a"], &["git", "status"]])
        );
        // A `#` glued to a word is a literal, not a comment.
        assert_eq!(split("echo a#b"), owned(&[&["echo", "a#b"]]));
    }

    #[test]
    fn reads_locale_translation_quoting_as_a_double_quoted_body() {
        assert_eq!(
            split("git commit $\"--no-verify\" -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn splits_words_on_ascii_whitespace_only_as_the_shell_does() {
        // A non-breaking space (U+00A0) is an ordinary word character to the
        // shell, not an IFS separator — one word, matching bash.
        assert_eq!(split("echo a\u{a0}b"), owned(&[&["echo", "a\u{a0}b"]]));
    }

    #[test]
    fn honors_backslash_escapes() {
        assert_eq!(split("echo a\\ b"), owned(&[&["echo", "a b"]]));
    }

    #[test]
    fn reports_unterminated_quoting() {
        assert_eq!(
            split_commands("echo 'oops"),
            Err(SplitError::UnterminatedSingleQuote)
        );
        assert_eq!(
            split_commands("echo \"oops"),
            Err(SplitError::UnterminatedDoubleQuote)
        );
        assert_eq!(
            split_commands("echo $(oops"),
            Err(SplitError::UnterminatedSubstitution)
        );
        assert_eq!(
            split_commands("echo `oops"),
            Err(SplitError::UnterminatedBacktick)
        );
        assert_eq!(
            split_commands("echo $'oops"),
            Err(SplitError::UnterminatedAnsiCQuote)
        );
    }

    #[test]
    fn keeps_a_pipeline_amp_a_separator() {
        assert_eq!(
            split("foo |& git commit --no-verify"),
            owned(&[&["foo"], &["git", "commit", "--no-verify"]])
        );
    }

    #[test]
    fn treats_a_whitespace_separated_digit_as_an_argument_not_an_fd_prefix() {
        assert_eq!(split("echo 2 > f"), owned(&[&["echo", "2"]]));
    }

    #[test]
    fn recognises_heredoc_operators_whole() {
        assert_eq!(split("cat <<EOF"), owned(&[&["cat"]]));
        assert_eq!(split("cat <<-EOF"), owned(&[&["cat"]]));
        assert_eq!(split("cat <<<'git commit'"), owned(&[&["cat"]]));
    }

    #[test]
    fn reads_a_quoted_delimiter_body_as_data_rather_than_as_commands() {
        assert_eq!(
            split("cat > gate.md <<'EOF'\ngit commit --no-verify -m x\nEOF"),
            owned(&[&["cat"]])
        );
        assert_eq!(
            split("cat <<\"EOF\"\ngit commit --no-verify -m x\nEOF"),
            owned(&[&["cat"]])
        );
        assert_eq!(
            split("cat <<E'O'F\ngit commit --no-verify -m x\nEOF"),
            owned(&[&["cat"]])
        );
        assert_eq!(
            split("cat <<\\EOF\ngit commit --no-verify -m x\nEOF"),
            owned(&[&["cat"]])
        );
    }

    #[test]
    fn keeps_an_unquoted_delimiter_body_in_the_scan() {
        assert_eq!(
            split("cat <<EOF\ngit commit --no-verify -m x\nEOF"),
            owned(&[
                &["cat"],
                &["git", "commit", "--no-verify", "-m", "x"],
                &["EOF"]
            ])
        );
    }

    #[test]
    fn strips_leading_tabs_from_a_dash_heredoc_delimiter_line() {
        assert_eq!(
            split("cat <<-'EOF'\n\tgit commit --no-verify -m x\n\tEOF\ngit status"),
            owned(&[&["cat"], &["git", "status"]])
        );
        assert_eq!(
            split("cat <<'EOF'\n\tEOF\ngit commit --no-verify -m x\nEOF\ngit status"),
            owned(&[&["cat"], &["git", "status"]])
        );
    }

    #[test]
    fn resumes_scanning_after_a_quoted_body_ends() {
        assert_eq!(
            split("cat <<'EOF'\nprose\nEOF\ngit commit --no-verify -m x"),
            owned(&[&["cat"], &["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn stops_stepping_over_bodies_at_the_first_unquoted_delimiter() {
        assert_eq!(
            split("cat <<'A' <<B\nprose\nA\ngit commit --no-verify -m x\nB"),
            owned(&[
                &["cat"],
                &["git", "commit", "--no-verify", "-m", "x"],
                &["B"]
            ])
        );
    }

    #[test]
    fn ends_a_body_whose_delimiter_never_arrives_at_the_end_of_input() {
        assert_eq!(
            split("cat <<'EOF'\ngit commit --no-verify -m x"),
            owned(&[&["cat"]])
        );
    }

    #[test]
    fn drops_the_redirection_target_rather_than_folding_it_into_the_arguments() {
        assert_eq!(split("git status > out.txt"), owned(&[&["git", "status"]]));
        assert_eq!(
            split("git status 2>/dev/null"),
            owned(&[&["git", "status"]])
        );
    }

    #[test]
    fn binds_an_fd_prefix_to_its_redirection() {
        assert_eq!(
            split("git commit 2>&1 --no-verify"),
            owned(&[&["git", "commit", "--no-verify"]])
        );
        assert_eq!(
            split("{fd}>/dev/null git commit --no-verify -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn a_redirection_terminates_the_word_it_touches() {
        assert_eq!(
            split("git commit --no-verify>log"),
            owned(&[&["git", "commit", "--no-verify"]])
        );
    }

    #[test]
    fn removes_a_line_continuation_before_word_splitting() {
        assert_eq!(
            split("git commit --no-ver\\\nify -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
        assert_eq!(
            split("git commit \"--no-ver\\\nify\" -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn decodes_ansi_c_escapes_rather_than_stripping_them() {
        assert_eq!(
            split("git commit $'-\\x6e' -m x"),
            owned(&[&["git", "commit", "-n", "-m", "x"]])
        );
        assert_eq!(
            split("git commit $'--no-\\166erify' -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn an_escaped_apostrophe_does_not_end_the_ansi_c_word_early() {
        assert_eq!(
            split("git commit -m $'it\\'s'"),
            owned(&[&["git", "commit", "-m", "it's"]])
        );
    }

    #[test]
    fn a_bare_dollar_not_introducing_a_quote_is_untouched() {
        assert_eq!(
            split("git commit -m $HOME"),
            owned(&[&["git", "commit", "-m", "$HOME"]])
        );
    }

    #[test]
    fn truncates_an_ansi_c_word_at_the_first_nul() {
        assert_eq!(
            split("git commit $'--no-verify\\0zzz' -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
        assert_eq!(
            split("git commit $'--no-verify\\x00zzz' -m x"),
            owned(&[&["git", "commit", "--no-verify", "-m", "x"]])
        );
    }

    #[test]
    fn reports_an_undeliverable_code_point_as_unparseable_not_a_crash() {
        assert_eq!(
            split_commands("git commit -m $'\\UFFFFFFFF'"),
            Err(SplitError::AnsiCCodePoint)
        );
        assert_eq!(
            split_commands("git commit -m $'\\uD800'"),
            Err(SplitError::AnsiCCodePoint)
        );
    }

    #[test]
    fn does_not_assemble_a_flag_from_a_decoded_control_character() {
        assert_eq!(
            split("git commit $'--no-\\verify' -m x"),
            owned(&[&["git", "commit", "--no-\x0berify", "-m", "x"]])
        );
    }

    #[test]
    fn keeps_an_unrecognised_ansi_c_escape_with_its_backslash() {
        assert_eq!(split("echo $'a\\qb'"), owned(&[&["echo", "a\\qb"]]));
    }
}
