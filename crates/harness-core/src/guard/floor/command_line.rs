//! Quote-aware splitting of a shell command line into simple commands.
//!
//! Argv is reconstructed as the shell assembles it — quotes stripped,
//! escapes resolved, ANSI-C bodies decoded — because the bypass check reads
//! the words git receives, not the bytes the operator typed. A stripped
//! `$'\x6e'` would read as the literal `x6e` while the shell delivers `n`,
//! which is a silent pass rather than a visible skip.
//!
//! Parsing boundaries, deliberate. A [`SplitError`] is the fail-safe one:
//! the caller turns it into a visible skip, never a block. The others end
//! in something passing unread, which is what a tripwire for the bypass a
//! session writes by hand buys its low false-refusal rate with.
//!
//! - Unterminated quoting and an ANSI-C code point the shell cannot deliver
//!   are [`SplitError`]s.
//! - `$(…)` text stays inside its enclosing word, which is where it
//!   expands, and its body is also read as the command list it is. A
//!   substitution runs its body whatever encloses it, so
//!   `o=$(git commit --no-verify -m x)` is that command spelled on this
//!   line rather than a mention of it. Single-quoted text is inert and is
//!   not read; `\$(…)` inside double quotes is the literal characters and
//!   is not read either.
//! - What a body's own grammar hides is not read either. The scan follows
//!   quoting and comments; it does not model a `case` pattern's `)` or a
//!   heredoc written inside the body, and both end the body early. Where
//!   the substitution is unquoted that costs nothing — the remainder is
//!   read as the commands it is — but inside a double-quoted capture the
//!   remainder is string text, and a bypass standing there passes unread.
//!   Measured on zsh 5.9 and bash 5.3, which run both forms; bash 3.2
//!   rejects them. A body the scan cannot delimit at all is left opaque
//!   instead, so the line keeps its verdict on everything outside it.
//! - A backtick body is *not* read, and the asymmetry is measured rather
//!   than stylistic: a backtick is also the code-span mark, so every
//!   document this parser meets is full of pairs, and pairing runs across
//!   the whole text — whether a mention lands inside one depends on how
//!   many backticks precede it. Reading them refused 2 of this
//!   repository's 200 most recent commit bodies where `$(…)` refused none,
//!   and the operator cannot read that refusal back to a cause. Read those
//!   counts against their population: 43 of the same 200 reach no verdict
//!   on either reading, because an apostrophe in prose leaves the line
//!   unparseable and an unparseable line is a skip. What stays
//!   open is the legacy spelling of a form `$(…)` now covers; the module
//!   is a tripwire for the bypass a session writes by hand, and the
//!   server-side re-run is the backstop for the one it does not.
//! - Redirections are read as the shell reads them (maximal munch, optional
//!   fd prefix): the operator terminates the current word and its target is
//!   dropped, so `2>&1` binds as one redirection rather than splitting at its
//!   `&`, and `--no-verify>log` reads as a flag plus a redirection rather
//!   than one opaque word.
//! - Heredoc bodies are not modelled. `<<` / `<<-` are recognised whole and
//!   the delimiter word is consumed as the operator's target — but each
//!   newline remains a separator, so a prose line beginning
//!   `git commit --no-verify` inside `cat <<EOF` still false-blocks. Whether
//!   a body is a document or a script is the receiving program's to decide,
//!   not the delimiter's: `bash <<'EOF'` runs every line of it, quoted
//!   delimiter and all, so no rule reading the command line can tell the two
//!   apart. Scanning the body false-blocks a document; skipping it passes a
//!   script. This takes the block, which surfaces, over the pass, which does
//!   not. A mention inside the line rather than at its head — the shape a
//!   document that quotes the flag actually takes — is not a command and
//!   passes, unless it is wrapped in a live substitution, which is a command
//!   wherever it sits. Quote such a mention to leave it inert.

use std::fmt;

/// What a command line read as, and the error that stopped the scan.
///
/// A scan that fails partway has still read the commands before it. Dropping
/// those would let a quote late on the line un-judge a bypass spelled out
/// early on it — a verdict lost rather than one never formed. The caller
/// judges what was read first and only then answers for the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub commands: Vec<Vec<String>>,
    pub error: Option<SplitError>,
}

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

/// Nesting no command line reaches, and the depth at which recursing the
/// parser would fault instead of answering. Past it the body is left unread,
/// which is what every body this scan cannot follow already is.
const MAX_SUBSTITUTION_DEPTH: usize = 32;

/// Byte index just past the `)` closing a `$(` that opened before `start`.
///
/// A paren inside quoting neither nests nor closes, so the scan carries the
/// quoting state — single, double and ANSI-C alike: `$(grep -c ')' f)` ends
/// at its last paren. Counting the quoted one would cut the body short and
/// leave the rest of the line to be read as something else.
fn substitution_end(input: &str, start: usize) -> Result<usize, SplitError> {
    let bytes = input.as_bytes();
    let mut depth = 1usize;
    let mut i = start;
    let mut in_word = false;
    while i < bytes.len() {
        match bytes[i] {
            // A comment runs to the newline, and the shell drops it before it
            // is anything: a quote or a paren written there is neither.
            b'#' if !in_word => {
                i += input[i..].find('\n').unwrap_or(input.len() - i);
            }
            b'\\' => i += 2,
            // ANSI-C quoting, where `\'` does not close: reading this body as
            // ordinary single quotes leaves the quoting inverted from here to
            // the end, and the paren that ends the substitution is past it.
            b'$' if bytes.get(i + 1) == Some(&b'\'') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if i >= bytes.len() {
                    return Err(SplitError::UnterminatedAnsiCQuote);
                }
                i += 1;
            }
            b'\'' => {
                let end = input[i + 1..]
                    .find('\'')
                    .ok_or(SplitError::UnterminatedSingleQuote)?;
                i += end + 2;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if i >= bytes.len() {
                    return Err(SplitError::UnterminatedDoubleQuote);
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => i += 1,
        }
        // `#` opens a comment only at a word boundary, as it does one level
        // up: `echo a#b` is one word and comments nothing.
        in_word = !matches!(
            bytes[i - 1],
            b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'(' | b')'
        );
    }
    Err(SplitError::UnterminatedSubstitution)
}

/// The span a `$(…)` covered before its body was read: parens counted, quoting
/// ignored. [`substitution_end`] reads the body the shell would; where it
/// cannot, the scan falls back here so the substitution stays the opaque word
/// it used to be rather than costing the line the verdict on everything
/// outside it.
fn opaque_substitution_end(input: &str, start: usize) -> Result<usize, SplitError> {
    let bytes = input.as_bytes();
    let mut depth = 1usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(SplitError::UnterminatedSubstitution)
}

/// The span a `$(…)` covers and the commands its body runs, or `None` where
/// the scan cannot follow the shell's grammar far enough to say — a `case`
/// pattern's `)`, a heredoc written inside the body.
///
/// Reading a body is an addition to what this parser used to do, so `None`
/// returns each caller to what it did with a `$(` before there was one:
/// unquoted, the span the paren count gives; inside double quotes, two
/// ordinary characters. Neither costs the line a verdict it already had.
fn read_substitution(input: &str, start: usize, depth: usize) -> Option<(usize, Vec<Vec<String>>)> {
    let end = substitution_end(input, start).ok()?;
    if depth >= MAX_SUBSTITUTION_DEPTH {
        return Some((end, Vec::new()));
    }
    let (commands, _) = split_nested(&input[start..end - 1], depth + 1);
    Some((end, commands))
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

    /// Commands a substitution runs, kept apart from the word its output
    /// expands into: the text belongs to the enclosing argv, the body is a
    /// command list of its own.
    fn push_substitution(&mut self, commands: Vec<Vec<String>>) {
        self.commands.extend(commands);
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
}

/// Split a shell command line into simple commands (each a word list).
///
/// Handles single/double quotes and backslash escapes; treats unquoted
/// `&& || ; | & \n ( )` and a brace-group `{` (a standalone `{` followed by
/// whitespace) as command separators; reads a `$(…)` body as the commands
/// it runs while its text stays in the enclosing word.
pub fn split_commands(input: &str) -> Split {
    let (commands, error) = split_nested(input, 0);
    Split { commands, error }
}

fn split_nested(input: &str, depth: usize) -> (Vec<Vec<String>>, Option<SplitError>) {
    let mut acc = Accumulator::default();
    let error = scan(input, depth, &mut acc).err();
    acc.push_command();
    (acc.commands, error)
}

fn scan(input: &str, depth: usize, acc: &mut Accumulator) -> Result<(), SplitError> {
    let bytes = input.as_bytes();
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
            let mut inner = Vec::new();
            while i < bytes.len() && bytes[i] != b'"' {
                // Double quotes stop word splitting, not substitution, so
                // a `$(…)` still runs its body here and the scan reads it.
                if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'(') {
                    // Where the body cannot be read, this stays the two
                    // characters it was: the paren count the unquoted branch
                    // falls back to would run past the quote that ends this
                    // string, and take the rest of the line with it.
                    match read_substitution(input, i + 2, depth) {
                        Some((end, commands)) => {
                            buf.push_str(&input[i..end]);
                            inner.extend(commands);
                            i = end;
                        }
                        None => {
                            buf.push('$');
                            i += 1;
                        }
                    }
                } else if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
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
            acc.push_substitution(inner);
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
            let (end, commands) = match read_substitution(input, i + 2, depth) {
                Some(read) => read,
                None => (opaque_substitution_end(input, i + 2)?, Vec::new()),
            };
            acc.push_str(&input[i..end]);
            acc.push_substitution(commands);
            i = end;
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
            continue;
        }
        if matches!(b, b'\n' | b';' | b'(' | b')') {
            acc.push_command();
            i += 1;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(input: &str) -> Vec<Vec<String>> {
        split_commands(input).commands
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

    /// The text expands into the enclosing word, and the body is a command
    /// list the shell runs — so the split answers with both.
    #[test]
    fn reads_a_substitution_body_and_keeps_its_text_in_the_enclosing_word() {
        assert_eq!(
            split("echo $(git commit --no-verify)"),
            owned(&[
                &["git", "commit", "--no-verify"],
                &["echo", "$(git commit --no-verify)"]
            ])
        );
    }

    /// A backslash escapes the next character inside a body too. Read as an
    /// opening quote instead, an escaped quote puts the rest of the body
    /// inside one, and the command standing after it is never reached.
    #[test]
    fn reads_an_escape_inside_a_substitution_body() {
        for body in [
            "echo a\\' ; git commit --no-verify",
            "git commit --no-verify ; echo a\\'",
        ] {
            let commands = split(&format!("o=$({body})"));
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
                "the escape ended the body: {body} gave {commands:?}"
            );
        }
    }

    /// ANSI-C quoting inside a body: `\'` does not close it, so counting it
    /// as an ordinary quote inverts the quoting for the rest of the body and
    /// the paren that ends the substitution falls on the wrong side.
    #[test]
    fn carries_ansi_c_quoting_through_a_substitution_body() {
        for body in [
            "echo $'\\'' ; git commit --no-verify",
            "echo $'a)b' ; git commit --no-verify",
        ] {
            let commands = split(&format!("o=$({body})"));
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
                "body ended early: {body} gave {commands:?}"
            );
        }
    }

    /// Double quotes hold a backtick as a character. The parser does not read
    /// a backtick body anywhere, so demanding a partner for one here would
    /// only make an ordinary string unreadable.
    #[test]
    fn holds_a_lone_backtick_in_double_quotes_as_a_character() {
        assert_eq!(split("echo \"a ` b\""), owned(&[&["echo", "a ` b"]]));
    }

    /// A backtick body stays one word (module note): the mark is the
    /// code-span mark, so reading it turns a document into a refusal.
    #[test]
    fn keeps_a_backtick_body_inside_the_enclosing_word() {
        assert_eq!(
            split("echo `git commit --no-verify`"),
            owned(&[&["echo", "`git commit --no-verify`"]])
        );
    }

    /// Capturing output is the shape this reaches git in: `o=$(…)` is an
    /// assignment to the enclosing shell and a git command to git, and only
    /// the second of those skips a hook.
    #[test]
    fn reads_a_substitution_the_enclosing_command_only_assigns() {
        assert_eq!(
            split("o=$(git commit --no-verify -m x)"),
            owned(&[
                &["git", "commit", "--no-verify", "-m", "x"],
                &["o=$(git commit --no-verify -m x)"]
            ])
        );
    }

    /// Double quotes stop word splitting, not substitution.
    #[test]
    fn reads_a_substitution_inside_double_quotes() {
        assert_eq!(
            split("echo \"$(git commit --no-verify)\""),
            owned(&[
                &["git", "commit", "--no-verify"],
                &["echo", "$(git commit --no-verify)"]
            ])
        );
    }

    /// Single quotes make the text inert, and a backslash inside double
    /// quotes makes it literal characters. Neither runs, so reading either
    /// as a command is what turns a document into a refusal.
    #[test]
    fn leaves_inert_substitution_text_as_text() {
        assert_eq!(
            split("echo '$(git commit --no-verify)'"),
            owned(&[&["echo", "$(git commit --no-verify)"]])
        );
        assert_eq!(
            split("echo \"\\$(git commit --no-verify)\""),
            owned(&[&["echo", "$(git commit --no-verify)"]])
        );
    }

    /// A paren inside quotes neither nests nor closes. Ending the body at it
    /// would cut the command short and leave the rest to be read as another.
    #[test]
    fn ends_a_substitution_at_the_paren_that_closes_it() {
        assert_eq!(
            split("echo $(grep -c ')' f)"),
            owned(&[&["grep", "-c", ")", "f"], &["echo", "$(grep -c ')' f)"]])
        );
    }

    /// A `)` the body only carries does not end it, whichever quoting holds
    /// it. Ending there drops the rest of the body — which is exactly where
    /// a command that follows the quoted paren sits.
    #[test]
    fn does_not_end_a_substitution_at_a_paren_it_only_carries() {
        for body in [
            "grep -c \')\' f; git commit --no-verify",
            "grep -c \")\" f; git commit --no-verify",
            "printf a \\); git commit --no-verify",
        ] {
            let commands = split(&format!("x=$({body})"));
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
                "body ended early: {body} gave {commands:?}"
            );
        }
    }

    /// A body with no closing paren anywhere is the one shape the fallback
    /// cannot cover: there is no span to keep as a word.
    #[test]
    fn reports_a_substitution_with_no_end() {
        assert_eq!(
            split_commands("o=$(echo abc").error,
            Some(SplitError::UnterminatedSubstitution)
        );
    }

    /// A body the read cannot follow leaves the substitution opaque, and the
    /// commands standing beside it are still judged. Losing them is how
    /// reading a body turns from an addition into a subtraction.
    #[test]
    fn keeps_the_rest_of_the_line_when_a_body_cannot_be_read() {
        for line in [
            "x=$(cat <<'EOF'\ndon't\nEOF\n) ; git commit --no-verify -m x",
            "x=$(echo a # don't\n) ; git commit --no-verify -m x",
            "x=$(case y in y) echo z;; esac) ; git commit --no-verify -m x",
        ] {
            let commands = split(line);
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
                "the line lost its verdict: {line} gave {commands:?}"
            );
        }
    }

    /// Past the bound the body stops being read, which is what an unreadable
    /// body already is. The line keeps its own commands either way, and the
    /// recursion stays off the stack.
    #[test]
    fn stops_reading_a_substitution_nested_past_the_bound() {
        let deep = format!(
            "x={}echo a{} ; git commit --no-verify -m x",
            "$(".repeat(MAX_SUBSTITUTION_DEPTH + 2),
            ")".repeat(MAX_SUBSTITUTION_DEPTH + 2)
        );
        let commands = split(&deep);
        assert!(
            commands.contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
            "the line lost its verdict: {commands:?}"
        );
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
            split_commands("echo 'oops").error,
            Some(SplitError::UnterminatedSingleQuote)
        );
        assert_eq!(
            split_commands("echo \"oops").error,
            Some(SplitError::UnterminatedDoubleQuote)
        );
        assert_eq!(
            split_commands("echo $(oops").error,
            Some(SplitError::UnterminatedSubstitution)
        );
        assert_eq!(
            split_commands("echo `oops").error,
            Some(SplitError::UnterminatedBacktick)
        );
        assert_eq!(
            split_commands("echo $'oops").error,
            Some(SplitError::UnterminatedAnsiCQuote)
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
    }

    #[test]
    fn keeps_a_heredoc_body_in_the_scan_whatever_its_delimiter() {
        // `bash <<'EOF'` runs every line of the body, so a quoted delimiter is
        // no evidence that the body is a document.
        for delimiter in ["EOF", "'EOF'", "\"EOF\"", "\\EOF"] {
            assert_eq!(
                split(&format!(
                    "bash <<{delimiter}\ngit commit --no-verify -m x\nEOF"
                )),
                owned(&[
                    &["bash"],
                    &["git", "commit", "--no-verify", "-m", "x"],
                    &["EOF"]
                ]),
                "{delimiter}"
            );
        }
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
            split_commands("git commit -m $'\\UFFFFFFFF'").error,
            Some(SplitError::AnsiCCodePoint)
        );
        assert_eq!(
            split_commands("git commit -m $'\\uD800'").error,
            Some(SplitError::AnsiCCodePoint)
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
