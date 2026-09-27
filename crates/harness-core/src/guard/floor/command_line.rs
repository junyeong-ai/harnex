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
//!   substitution runs its body wherever the shell expands it, so
//!   `o=$(git commit --no-verify -m x)` is that command spelled on this
//!   line rather than a mention of it. Where the shell expands nothing the
//!   text is not read: single-quoted, `\$(…)` inside double quotes, and a
//!   heredoc body under a quoted delimiter.
//! - What a body's own grammar hides is not read either. The scan follows
//!   quoting and comments; it models neither a `case` pattern's `)`, nor a
//!   heredoc written inside the body, nor a `${…}` carrying an unbalanced
//!   paren, and each ends the body early or not at all. What the remainder
//!   becomes then belongs to the enclosing word, not to a command of its
//!   own: after an assignment the prefix scan walks through to the git word
//!   and the bypass is still caught, after a command word it becomes that
//!   command's arguments, and inside double quotes it becomes string text.
//!   Only the first of those three still refuses. Measured on zsh 5.9 and
//!   bash 5.3, which run these lines; bash 3.2 rejects them, and the `${…}`
//!   shape ends in a visible skip rather than a silent pass. A body the
//!   scan cannot delimit at all is left opaque instead, so the line keeps
//!   its verdict on everything outside it.
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
//! - A heredoc's delimiter is read; its body is not. `<<` / `<<-` are
//!   recognised whole and the delimiter word is consumed as the operator's
//!   target, but each newline remains a separator, so a prose line beginning
//!   `git commit --no-verify` inside `cat <<EOF` still false-blocks. Whether
//!   a body is a document or a script is the receiving program's to decide,
//!   not the delimiter's: `bash <<'EOF'` runs every line of it, quoted
//!   delimiter and all, so no rule reading the command line can tell the two
//!   apart. Scanning the body false-blocks a document; skipping it passes a
//!   script. This takes the block, which surfaces, over the pass, which does
//!   not. A mention inside the line rather than at its head — the shape a
//!   document that quotes the flag actually takes — is not a command and
//!   passes. What the delimiter does settle is expansion: quote any
//!   character of it and the body is literal, so a `$(…)` written there is a
//!   mention and is read only under a bare delimiter. Reading it under a
//!   quoted one refused the documents that explain this module, and what
//!   that costs is a bypass wrapped in an assignment inside `bash <<'EOF'` —
//!   a second shell running a script, which is out of scope however it is
//!   spelled.

use std::fmt;
use std::ops::Range;

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

/// A heredoc queued on the line being scanned: where its body ends, and
/// whether the shell expands that body.
///
/// Quoting any character of the delimiter word makes the whole body literal —
/// `<<'EOF'`, `<<"EOF"`, `<<\EOF` and `<<E"OF"` alike — and an unquoted one
/// expands it before the receiving program sees a byte.
struct Heredoc {
    delimiter: String,
    literal: bool,
    /// `<<-`, which strips leading tabs from the line that ends the body.
    strip_tabs: bool,
}

/// The heredoc a `<<` / `<<-` at `start` (just past the operator) opens.
///
/// The delimiter is read from the raw text because the word itself is consumed
/// as the redirection's target and reaches no caller, and because the quoting
/// that decides the body is what word assembly strips.
fn heredoc_at(input: &str, start: usize, strip_tabs: bool) -> Option<Heredoc> {
    let bytes = input.as_bytes();
    let mut i = start;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let mut delimiter = String::new();
    let mut literal = false;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>' => break,
            b'\\' => {
                literal = true;
                let ch = input[i + 1..].chars().next()?;
                delimiter.push(ch);
                i += 1 + ch.len_utf8();
            }
            quote @ (b'\'' | b'"') => {
                literal = true;
                let end = input[i + 1..].find(char::from(quote))?;
                delimiter.push_str(&input[i + 1..i + 1 + end]);
                i += end + 2;
            }
            _ => {
                let ch = input[i..].chars().next()?;
                delimiter.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    (!delimiter.is_empty()).then_some(Heredoc {
        delimiter,
        literal,
        strip_tabs,
    })
}

/// Where a heredoc body starting at `start` ends, and where the text after its
/// delimiter line resumes. A delimiter that never arrives runs the body to the
/// end of the input, which is the unterminated heredoc the shell reports.
///
/// A delimiter is never empty, so a `start` landing mid-line or past the end
/// matches nothing there and the walk reaches the next line start regardless.
fn heredoc_body(input: &str, start: usize, heredoc: &Heredoc) -> (usize, usize) {
    let mut line_start = start;
    while line_start < input.len() {
        let line_end = input[line_start..]
            .find('\n')
            .map_or(input.len(), |n| line_start + n);
        let line = &input[line_start..line_end];
        let line = if heredoc.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if line == heredoc.delimiter {
            return (line_start, (line_end + 1).min(input.len()));
        }
        if line_end >= input.len() {
            break;
        }
        line_start = line_end + 1;
    }
    (input.len(), input.len())
}

/// Whether `i` falls in one of `ranges`, which the scan keeps two of: every
/// heredoc body, against which a `<<` is text rather than an operator, and the
/// literal ones alone, in which the shell expands nothing so a `$(…)` is text
/// rather than a command this line runs.
fn inside_body(ranges: &[Range<usize>], i: usize) -> bool {
    ranges.iter().any(|range| range.contains(&i))
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
///
/// Double quotes stop neither a `$(` nor what it holds, so in
/// `"$(cut -d'"' -f2 f)"` the middle quote is the inner body's and the string
/// closes at the last one. Where a body cannot be read that way — a backtick
/// carrying a quote, which this scan does not model, or a comment bash 3.2
/// does not take for one — it is read again with each `$(` inside double
/// quotes held as characters, the reading `scan` falls back to one level up.
fn substitution_end(input: &str, start: usize) -> Result<usize, SplitError> {
    body_end(input, start, QuotedSubstitution::Read)
        .or_else(|_| body_end(input, start, QuotedSubstitution::Text))
}

/// How [`body_end`] reads a `$(` inside double quotes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum QuotedSubstitution {
    /// As a substitution, whose quotes are its own.
    Read,
    /// As two characters, so the string closes at its next unescaped quote.
    Text,
}

/// What a substitution body has open at a point of its scan.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    /// `$(`. The paren closing it ends part of a word, so a `#` right after it
    /// is word text.
    Substitution,
    /// Any other `(`, taken for a subshell's: the paren closing it ends a
    /// command, so a `#` after it comments. `<(…)`, `>(…)`, bash's array
    /// `a=(…)` and zsh's `=(…)` also end only part of a word, and `scan`
    /// misreads them the same way: a `#` glued to one hides the commands after
    /// it.
    Subshell,
    /// `"`, inside which only a backslash, a `$(` and the closing quote act —
    /// the three `scan` reads there.
    DoubleQuote,
}

fn body_end(input: &str, start: usize, quoted: QuotedSubstitution) -> Result<usize, SplitError> {
    let bytes = input.as_bytes();
    let mut open = vec![Open::Substitution];
    let mut i = start;
    let mut in_word = false;
    while i < bytes.len() {
        if open.last() == Some(&Open::DoubleQuote) {
            match bytes[i] {
                b'"' => {
                    open.pop();
                    in_word = true;
                    i += 1;
                }
                b'\\' => i += 2,
                b'$' if quoted == QuotedSubstitution::Read && bytes.get(i + 1) == Some(&b'(') => {
                    open.push(Open::Substitution);
                    in_word = false;
                    i += 2;
                }
                _ => i += 1,
            }
            continue;
        }
        match bytes[i] {
            // A comment runs to the newline, and the shell drops it before it
            // is anything: a quote or a paren written there is neither.
            b'#' if !in_word => {
                i += input[i..].find('\n').unwrap_or(input.len() - i);
            }
            // An escaped character is word text whatever its byte, and a
            // backslash-newline is removed outright, leaving the state as it was:
            // `a\ #b` is one word, while `echo \⏎#c` still opens a comment. The
            // boundary read below would take the escaped byte for a separator.
            b'\\' => {
                if bytes.get(i + 1) != Some(&b'\n') {
                    in_word = true;
                }
                i += 2;
                continue;
            }
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
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                open.push(Open::Substitution);
                i += 2;
            }
            b'\'' => {
                let end = input[i + 1..]
                    .find('\'')
                    .ok_or(SplitError::UnterminatedSingleQuote)?;
                i += end + 2;
            }
            b'"' => {
                open.push(Open::DoubleQuote);
                i += 1;
            }
            b'(' => {
                open.push(Open::Subshell);
                i += 1;
            }
            b')' => {
                let closes_substitution = open.pop() == Some(Open::Substitution);
                i += 1;
                if open.is_empty() {
                    return Ok(i);
                }
                in_word = closes_substitution;
                continue;
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

/// The span a `$(…)` covered before its body was read: parens counted, and
/// the quoting and comments that could hide one ignored, so a `)` written
/// inside either still closes the span. [`substitution_end`] reads the body
/// the shell would; where it cannot, the scan falls back here so the
/// substitution stays the opaque word it used to be rather than costing the
/// line the verdict on everything outside it.
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

/// The span the `$(…)` at `dollar` covers and the commands its body runs, or
/// `None` where it is not a body this line runs: the scan cannot follow the
/// shell's grammar far enough to say — a `case` pattern's `)`, a heredoc
/// written inside the body — or the `$(` stands in a literal heredoc body,
/// where the shell expands nothing.
///
/// Reading a body is an addition to what this parser used to do, so `None`
/// returns each caller to what it did with a `$(` before there was one:
/// unquoted, the span the paren count gives; inside double quotes, two
/// ordinary characters. Neither costs the line a verdict it already had.
fn read_substitution(
    input: &str,
    dollar: usize,
    depth: usize,
    literal: &[Range<usize>],
) -> Option<(usize, Vec<Vec<String>>)> {
    if inside_body(literal, dollar) {
        return None;
    }
    let start = dollar + 2;
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
    let mut queued: Vec<Heredoc> = Vec::new();
    // Every body, and the literal ones again. Both stay ascending and
    // disjoint: nothing is queued from inside a body, so the bodies drained at
    // one newline follow each other and the next drain starts past them all.
    let mut bodies: Vec<Range<usize>> = Vec::new();
    let mut literal: Vec<Range<usize>> = Vec::new();
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
                    match read_substitution(input, i, depth, &literal) {
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
            let (end, commands) = match read_substitution(input, i, depth, &literal) {
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
            // A `<<` standing inside a heredoc body is that body's own text
            // and opens nothing, whether or not the body expands. The scan
            // walks a body's text, so it meets one; queuing there ends the
            // outer body early at the inner delimiter, and every line between
            // is read as the shell never reads it. Measured on bash 5.3, bash
            // 3.2 and zsh 5.9: all three run a `$(…)` written after a `<<'C'`
            // line inside an expanding body, which this scan used to skip.
            if matches!(redirection, "<<" | "<<-")
                && !inside_body(&bodies, i)
                && let Some(heredoc) =
                    heredoc_at(input, i + redirection.len(), redirection == "<<-")
            {
                queued.push(heredoc);
            }
            acc.start_redirection();
            i += redirection.len();
            continue;
        }
        if matches!(b, b'\n' | b';' | b'(' | b')') {
            acc.push_command();
            i += 1;
            if b == b'\n' {
                let mut at = i;
                for heredoc in queued.drain(..) {
                    let (body_end, resume) = heredoc_body(input, at, &heredoc);
                    bodies.push(at..body_end);
                    if heredoc.literal {
                        literal.push(at..body_end);
                    }
                    at = resume;
                }
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

    /// Inside a body a `#` comments only where it starts a word, as it does one
    /// level up. An escaped character, a closed quote and a closed substitution
    /// are word text, so a `#` after any of them belongs to the word and the
    /// command after it is read. A line continuation at a word boundary and a
    /// closed subshell leave the boundary, so a `#` there still comments out the
    /// paren that would have ended the body.
    #[test]
    fn reads_a_hash_in_a_body_as_the_shell_does() {
        let bypass = owned(&[&["git", "commit", "--no-verify"]]).remove(0);
        for body in [
            "echo a\\ #b; git commit --no-verify",
            "echo a\\;#b; git commit --no-verify",
            "echo a\\\n#b; git commit --no-verify",
            "echo \"a\"#b; git commit --no-verify",
            "echo 'a'#b; git commit --no-verify",
            "echo $(echo x)#b; git commit --no-verify",
            "echo $((1+1))#b; git commit --no-verify",
        ] {
            let commands = split(&format!("o=$({body})"));
            assert!(
                commands.contains(&bypass),
                "the body went unread: {body:?} gave {commands:?}"
            );
        }
        for line in [
            "o=$(echo \\\n#c) ; git commit --no-verify\n)",
            "o=$( (true)#c) ; git commit --no-verify\n)",
        ] {
            let commands = split(line);
            assert!(
                !commands.contains(&bypass),
                "a comment read as a command: {line:?} gave {commands:?}"
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
    /// quotes makes it literal characters. A paren no `$` opens is not a
    /// substitution wherever it sits. None of the three runs, so reading any
    /// of them as a command is what turns a document into a refusal.
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
        assert_eq!(
            split("echo \"x(git commit --no-verify)\""),
            owned(&[&["echo", "x(git commit --no-verify)"]])
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
    /// a command that follows the quoted paren sits. The paren appears alone
    /// and between characters because the two lengths break differently: a
    /// scan resuming at the wrong quote still reads a one-character run
    /// correctly, and a scan stepping by the wrong amount still reads an
    /// even-length one correctly.
    #[test]
    fn does_not_end_a_substitution_at_a_paren_it_only_carries() {
        for body in [
            "grep -c \')\' f; git commit --no-verify",
            "grep -c \")\" f; git commit --no-verify",
            "grep -c \'ab)c\' f; git commit --no-verify",
            "grep -c \"ab)c\" f; git commit --no-verify",
            "printf a \\); git commit --no-verify",
        ] {
            let commands = split(&format!("x=$({body})"));
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
                "body ended early: {body} gave {commands:?}"
            );
        }
    }

    /// Inside double quotes the body must be read to the paren that really
    /// ends it. End it anywhere else and the rest of the body becomes string
    /// text, where a command standing there is never seen — the one place
    /// getting the scan wrong hides something rather than merely skipping.
    #[test]
    fn reads_a_double_quoted_body_to_the_paren_that_ends_it() {
        for body in [
            // a nested substitution's paren closes it, not this one
            "echo $(echo a) ; git commit --no-verify",
            // `#` inside a word is not a comment
            "echo a#b ; git commit --no-verify",
            // `#` at a word boundary is, so the paren it carries is not one
            "echo x # )\ngit commit --no-verify",
            // `$` is an ANSI-C quote only before a quote
            "echo $x ; git commit --no-verify",
        ] {
            let commands = split(&format!("v=\"$({body})\""));
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
                "the body ended early: {body} gave {commands:?}"
            );
        }
    }

    /// Double quotes inside a body stop neither a `$(` nor what it holds, so a
    /// quote the inner body carries — `cut -d'"'`, `tr -d '"'`, or one a comment
    /// holds, as in `"$(#"⏎)"` — does not close the string. Closing it there
    /// leaves the rest of the body inside a string that never ends, and the
    /// command after it goes unread. An inner body the scan cannot read — a
    /// backtick holding a quote, a `#` bash 3.2 does not take for a comment —
    /// leaves the string to close at its own quote, the reading `scan` falls
    /// back to. At least one of bash 5.3, bash 3.2 and zsh 5.9 runs git in each
    /// body below, and none of them runs the escaped `\$(` as a command.
    #[test]
    fn reads_a_body_past_a_substitution_in_double_quotes() {
        let bypass = owned(&[&["git", "commit", "--no-verify"]]).remove(0);
        for body in [
            "echo \"$(cut -d'\"' -f2 f)\"; git commit --no-verify",
            "echo \"$(echo \"$(printf %s '\"')\")\"; git commit --no-verify",
            "echo \"a\\\"$(printf %s '\"')\"; git commit --no-verify",
            "echo \"$(echo x)#c\"; git commit --no-verify",
            "echo \"a)b\"; git commit --no-verify",
            "echo \"$(#\"\n)\"; git commit --no-verify",
            "echo \"$(`'`)\"; git commit --no-verify",
            "echo \"$(#)\"; git commit --no-verify",
        ] {
            let commands = split(&format!("o=$({body})"));
            assert!(
                commands.contains(&bypass),
                "the body went unread: {body:?} gave {commands:?}"
            );
        }
        let escaped = split("o=$(echo \"\\$(git commit --no-verify)\")");
        assert!(
            !escaped.contains(&bypass),
            "an escaped `$(` read as a command: {escaped:?}"
        );
    }

    /// A body with no closing paren anywhere is the one shape the fallback
    /// cannot cover: there is no span to keep as a word.
    #[test]
    fn reports_a_substitution_with_no_end() {
        for line in ["o=$(echo abc", "o=$(echo abc \\", "echo $(\\"] {
            assert_eq!(
                split_commands(line).error,
                Some(SplitError::UnterminatedSubstitution),
                "{line:?}"
            );
        }
    }

    /// A body the read cannot follow leaves the substitution opaque, and the
    /// commands standing beside it are still judged. Losing them is how
    /// reading a body turns from an addition into a subtraction.
    #[test]
    fn keeps_the_rest_of_the_line_when_a_body_cannot_be_read() {
        for line in [
            "x=$(cat <<'EOF'\ndon't\nEOF\n) ; git commit --no-verify -m x",
            "x=$(echo a # don't\n) ; git commit --no-verify -m x",
            "x=$(echo a # comment) ; git commit --no-verify -m x",
            "x=$(case y in y) echo z;; esac) ; git commit --no-verify -m x",
            "x=$(echo $'abc) ; git commit --no-verify -m x",
            "x=$(echo a #) ; git commit --no-verify -m x)",
            "result=$($(echo a) # c) ; git commit --no-verify -m x",
        ] {
            let commands = split(line);
            assert!(
                commands.contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
                "the line lost its verdict: {line} gave {commands:?}"
            );
        }
    }

    /// A `<<` written inside a heredoc body is that body's own text. The scan
    /// walks a body looking for commands, so it meets one, and a heredoc queued
    /// there ends the outer body at the inner delimiter — leaving the lines
    /// between read as the shell never reads them, or, under a quoted inner
    /// delimiter, not read at all. The second is the silent pass.
    ///
    /// Measured with `$(echo RAN >&2)` in place of the bypass: bash 5.3, bash
    /// 3.2 and zsh 5.9 all print RAN, because `<<'C'` is a line of A's body and
    /// so is the line after it.
    #[test]
    fn a_heredoc_operator_inside_a_body_opens_nothing() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "cat <<A <<'B'\n<<'C'\no=$(git commit --no-verify -m x)\nC\nA\nbody\nB",
            "cat <<A\n<<'C'\no=$(git commit --no-verify -m x)\nC\nA",
            "cat <<A\n<<C\no=$(git commit --no-verify -m x)\nC\nA",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a `<<` inside a body opened one and hid the command after it: {line} gave {:?}",
                split(line)
            );
        }
        // The outer body still ends where its own delimiter says, so what
        // follows the heredoc keeps its verdict.
        let after = "cat <<A\n<<'C'\ntext\nC\nA\ngit commit --no-verify -m x";
        assert!(
            split(after).contains(&bypass),
            "the line after the body lost its verdict: {:?}",
            split(after)
        );
    }

    /// A quoted delimiter leaves the whole body literal, so a `$(…)` written
    /// there is a mention. An unquoted one expands the body before the
    /// receiving program sees a byte, so the same text is a command. The
    /// receiving program does not enter it: the quoting on the delimiter is
    /// what the shell reads, whoever the body is for.
    #[test]
    fn reads_a_heredoc_body_only_where_the_shell_expands_it() {
        for line in [
            "git commit -q -F - <<'EOF'\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "cat > f.md <<\"EOF\"\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "cat > f.md <<\\EOF\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "cat > f.md << 'EOF'\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "bash <<'EOF'\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "git commit -q -F - <<'EOF'\no=$(git commit --no-verify -m x)\nEOF",
            "cat <<'A' <<'A'\nfirst\nA\nwrap it (o=$(git commit --no-verify -m x))\nA",
        ] {
            assert!(
                !split(line).contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
                "a literal body was read: {line} gave {:?}",
                split(line)
            );
        }
        for line in [
            "git commit -q -F - <<EOF\nwrap it (o=$(git commit --no-verify -m x))\nEOF",
            "cat > f.md <<-EOF\n\twrap it (o=$(git commit --no-verify -m x))\n\tEOF",
        ] {
            assert!(
                split(line).contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
                "an expanded body went unread: {line} gave {:?}",
                split(line)
            );
        }
    }

    /// The body ends at its delimiter line, so a substitution standing after
    /// the heredoc is read again. Running the literal region past the end
    /// would leave the rest of the command line unjudged — which is what a
    /// delimiter read wrong does, so the spellings here are the ones whose
    /// quoting has to come off for the body to end at all.
    #[test]
    fn resumes_reading_substitutions_after_the_delimiter_line() {
        for line in [
            "cat <<'EOF'\nmention (o=$(x))\nEOF\no=$(git commit --no-verify -m x)",
            "cat <<-'EOF'\n\tmention (o=$(x))\n\tEOF\no=$(git commit --no-verify -m x)",
            "cat <<'EOF'\nnot the end: <<'Z'\nEOF\no=$(git commit --no-verify -m x)",
            "cat << 'EOF'\nmention (o=$(x))\nEOF\no=$(git commit --no-verify -m x)",
            "cat <<\\EOF\nmention (o=$(x))\nEOF\no=$(git commit --no-verify -m x)",
            "cat <<E'O'F\nmention (o=$(x))\nEOF\no=$(git commit --no-verify -m x)",
            "cat <<\"E\"OF\nmention (o=$(x))\nEOF\no=$(git commit --no-verify -m x)",
        ] {
            assert!(
                split(line).contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
                "the line lost its verdict past the heredoc: {line} gave {:?}",
                split(line)
            );
        }
    }

    /// An unreadable body keeps the span the paren count gives it, which is
    /// the span it had before any body was read. The count has to carry the
    /// pairs the body only holds: end the word at the first `)` and what
    /// follows starts a command inside what was one word.
    #[test]
    fn holds_an_unreadable_body_to_the_span_the_paren_count_gives() {
        assert_eq!(
            split("x=$(echo $'a (b) c) ; git commit --no-verify -m x"),
            owned(&[
                &["x=$(echo $'a (b) c)"],
                &["git", "commit", "--no-verify", "-m", "x"],
            ])
        );
    }

    /// Past the bound the body stops being read, which is what an unreadable
    /// body already is. The line keeps its own commands either way, and the
    /// recursion stays off the stack. Both halves are read from the outside,
    /// because a bound that stopped counting would look the same from here
    /// until the nesting is deep enough to fault instead of answering.
    #[test]
    fn stops_reading_a_substitution_nested_past_the_bound() {
        let nest = MAX_SUBSTITUTION_DEPTH + 2;
        let deep = format!(
            "x={}echo a{} ; git commit --no-verify -m x",
            "$(".repeat(nest),
            ")".repeat(nest)
        );
        let commands = split(&deep);
        assert!(
            commands.contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
            "the line lost its verdict: {commands:?}"
        );

        let buried = format!(
            "x={}git commit --no-verify{} ; echo done",
            "$(".repeat(nest),
            ")".repeat(nest)
        );
        let commands = split(&buried);
        assert!(
            !commands.contains(&owned(&[&["git", "commit", "--no-verify"]])[0]),
            "a body past the bound was read: {commands:?}"
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

    /// Every line of up to five symbols drawn from what the scan branches on
    /// returns a split. The scan walks byte offsets by hand, and what breaks
    /// such a walk is an edge no example names — a step past the end, an offset
    /// inside a multi-byte character — while the hook runs it on every command
    /// a session writes. Five symbols reach a heredoc closed by its delimiter
    /// (`<<x⏎x`) and every escape standing last in a substitution or a quote.
    #[test]
    fn split_commands_answers_every_short_line() {
        const SYMBOLS: [&str; 19] = [
            "\\", "'", "\"", "$", "(", ")", "`", "<", ">", "-", "&", "\n", "#", "{", " ", "x", "u",
            "7", "é",
        ];
        let mut digits = Vec::new();
        loop {
            let line: String = digits.iter().map(|&d| SYMBOLS[d]).collect();
            let answered = std::panic::catch_unwind(|| split_commands(&line));
            assert!(answered.is_ok(), "split_commands panicked on {line:?}");
            let Some(position) = digits.iter().rposition(|&d| d + 1 < SYMBOLS.len()) else {
                if digits.len() == 5 {
                    break;
                }
                digits = vec![0; digits.len() + 1];
                continue;
            };
            digits[position] += 1;
            digits[position + 1..].fill(0);
        }
    }
}
