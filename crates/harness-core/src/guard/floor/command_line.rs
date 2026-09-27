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
//! - A backtick body is *not* read on the command line, and the asymmetry
//!   is measured rather than stylistic: a backtick is also the code-span
//!   mark, so every document this parser meets is full of pairs, and
//!   pairing runs across the whole text — whether a mention lands inside
//!   one depends on how many backticks precede it. Reading them refused 2
//!   of this repository's 200 most recent commit bodies where `$(…)`
//!   refused none, and the operator cannot read that refusal back to a
//!   cause. Read those counts against their population: 43 of the same 200
//!   reach no verdict on either reading, because an apostrophe in prose
//!   leaves the line unparseable and an unparseable line is a skip. What
//!   stays open is the legacy spelling of a form `$(…)` now covers; the
//!   module is a tripwire for the bypass a session writes by hand, and the
//!   server-side re-run is the backstop for the one it does not.
//!   Inside a heredoc body the shell expands, the pairing is the shell's
//!   own and it runs what the pair holds, so there the body is read: what
//!   a refusal costs is a line the shell already runs, and the accident
//!   the command-line count measured cannot arise. Where the span itself
//!   is written, an escaped backtick is the span's text and not its end —
//!   ending it at the first backtick found is the same defect as ending a
//!   `$(…)` by counting parens.
//! - Redirections are read as the shell reads them (maximal munch, optional
//!   fd prefix): the operator terminates the current word and its target is
//!   dropped, so `2>&1` binds as one redirection rather than splitting at its
//!   `&`, and `--no-verify>log` reads as a flag plus a redirection rather
//!   than one opaque word.
//! - A heredoc's delimiter is read as the operator's target, and its body is
//!   read twice because two programs read it. The shell expands a body under
//!   a bare delimiter before the receiving program sees a byte, and only
//!   `\`, `$` and a backtick act there — an apostrophe quotes nothing and a
//!   `#` comments nothing — so a `$(…)` written in one stands however it is
//!   surrounded. The receiving program then reads what it was handed and may
//!   be a shell: `bash <<'EOF'` runs every line of it, quoted delimiter and
//!   all, so no rule reading the command line can tell a document from a
//!   script. That second reading is the command-line one, obeying the quoting
//!   the first does not, and each answers for one program: substitutions are
//!   the first reading's and everything else the second's, so neither repeats
//!   the other's commands. Neither reaches past the body either — a quote or
//!   a `$(` opened in body text is that body's, the command line after the
//!   delimiter line keeps its verdict, and a `<<` written in a body or in a
//!   delimiter line opens nothing on the line that carried the operator.
//!   Scanning the body false-blocks a document; skipping it passes a script.
//!   This takes the block, which surfaces, over the pass, which does not, so
//!   a prose line beginning `git commit --no-verify` inside `cat <<EOF` still
//!   false-blocks, while a mention inside the line rather than at its head —
//!   the shape a document that quotes the flag actually takes — is not a
//!   command and passes. What the delimiter settles is the first reading:
//!   quote any character of it and nothing expands, so a `$(…)` there is a
//!   mention. Reading it under a quoted delimiter refused the documents that
//!   explain this module, and what that costs is a bypass wrapped in an
//!   assignment inside `bash <<'EOF'` — a second shell running a script,
//!   which is out of scope however it is spelled.

use std::collections::HashMap;
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

/// The byte just past the backtick closing the one before `start`.
///
/// A backslash escapes the next character inside a backtick body, which is how
/// the legacy spelling nests: `` \` `` does not close the span. Searching for
/// the next backtick alone ends the span early and leaves the rest of the text
/// to be read as something else — the same defect as ending a `$(…)` by
/// counting parens.
fn backtick_end(input: &str, start: usize) -> Result<usize, SplitError> {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err(SplitError::UnterminatedBacktick)
}

/// A backtick body as the shell hands it on: a backslash before a backtick, a
/// `$` or another backslash is removed, and any other backslash is one of the
/// body's own characters.
fn unescaped_backtick_body(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && matches!(bytes.get(i + 1), Some(b'`' | b'$' | b'\\')) {
            out.push(char::from(bytes[i + 1]));
            i += 2;
        } else {
            let ch = body[i..].chars().next().expect("in-bounds char");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
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

/// Both readings of a heredoc `body`, into `acc`, and the error the line
/// keeps. `expanded` says whether the shell expanded this body before the
/// receiving program saw it, which is a bare delimiter under text that expands
/// at all.
///
/// Neither reading is given the input, only the body, so neither can reach
/// past it: a quote or a `$(` opened in body text is the body's, and the
/// command line after the delimiter line keeps the verdict it had. That is
/// also what makes a `<<` written in a body or in a delimiter line open
/// nothing on the line that carried the operator — the scan never walks
/// either.
///
/// A body either reading cannot read is a body whose commands are unknown, so
/// the error is the line's and the line is a visible skip. That a reading
/// stopped is no evidence that a shell stops there: where this scan's grammar
/// is the weaker of the two it stops early, and a span ended by counting
/// parens leaves a quote loose that pairs with one further down the body and
/// swallows the command between. Most heredocs carry prose, whose apostrophe
/// ends the reading on the first line, so the skips this costs are many —
/// and every one of them is a line whose body this scan did not read.
fn heredoc_commands(
    body: &str,
    expanded: bool,
    depth: usize,
    acc: &mut Accumulator,
) -> Option<SplitError> {
    if depth >= MAX_NESTING_DEPTH {
        return None;
    }
    // What the receiving program makes of the body if it is a shell. The other
    // reading holds the substitutions, so this one leaves them the text they
    // are and no command is found twice.
    let (commands, script) = split_nested(body, depth + 1, Expansion::Mention);
    acc.push_substitution(commands);
    // What the shell already did to the body, which no quoting in it answers
    // to.
    let expansion = expanded
        .then(|| expanded_substitutions(body, depth + 1, acc).err())
        .flatten();
    script.or(expansion)
}

/// The commands an expanding heredoc `body` runs by expansion alone.
///
/// Only `\`, `$` and a backtick act in a body the shell expands: an
/// apostrophe quotes nothing and a `#` comments nothing, so `it's $(…) isn't`
/// and `# built at $(…)` both run the substitution. Reading the body as the
/// command list it might also be obeys those characters, and so cannot answer
/// for this.
fn expanded_substitutions(
    body: &str,
    depth: usize,
    acc: &mut Accumulator,
) -> Result<(), SplitError> {
    let bytes = body.as_bytes();
    let mut settled = Settled::default();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // A backslash escapes only these here; before anything else it
            // is one of the body's own characters.
            b'\\' if matches!(bytes.get(i + 1), Some(b'$' | b'`' | b'\\' | b'\n')) => i += 2,
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                // A span this reading cannot delimit is a command list it has
                // not read, and the shell runs it either way. The paren count
                // the command line falls back to would end the span somewhere
                // and report nothing from it, which is that list passing in
                // silence.
                let (end, commands) =
                    read_substitution(body, i, depth, Expansion::Runs, &mut settled)
                        .ok_or(SplitError::UnterminatedSubstitution)?;
                acc.push_substitution(commands);
                i = end;
            }
            // Read, unlike on the command line. There a backtick is also the
            // code-span mark and pairing runs across prose, so a mention lands
            // inside a pair by accident; here the shell pairs the body's
            // backticks the same way this search does and runs what it finds,
            // so reading one refuses only what the line already runs.
            b'`' => {
                let end = backtick_end(body, i + 1)?;
                let (commands, _) = split_nested(
                    &unescaped_backtick_body(&body[i + 1..end - 1]),
                    depth + 1,
                    Expansion::Runs,
                );
                acc.push_substitution(commands);
                i = end;
            }
            _ => i += 1,
        }
    }
    Ok(())
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
/// which is what every body this scan cannot follow already is. A substitution
/// body and a heredoc body both count, being the two texts this module reads
/// as commands of their own.
///
/// It bounds what a line costs as well as the stack: each level reads its own
/// text, so a byte nested `d` deep is read `d` times. Measured on a release
/// build over 250 KB of `"$(#)"` repeated, 0.011 s at depth 0 and 0.679 s at
/// depth 32 — linear in the depth, and the cap is what keeps that a multiple
/// rather than a ladder.
const MAX_NESTING_DEPTH: usize = 32;

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
fn substitution_end(input: &str, start: usize, settled: &mut Settled) -> Result<usize, SplitError> {
    body_end(input, start, QuotedSubstitution::Read, settled)
        .or_else(|_| body_end(input, start, QuotedSubstitution::Text, settled))
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

/// The byte a walk is at, whether the frame on top there is a quote, and
/// whether a word is open. These three settle what the walk does next and so
/// everything after it: the frames beneath the top one cannot be reached
/// before it closes, so they never enter.
type Resume = (usize, bool, bool);

/// What one reading of this input has already worked out, so that no walk
/// works it out again.
///
/// Each entry answers a [`Resume`] with the byte just past where the frame on
/// top there closes, or with the error the walk meets before it does. A walk
/// stepping through a state has answered for every later walk that reaches the
/// same one, and the states number with the bytes, so a line is stepped once
/// per reading rather than once per `$(` written on it.
///
/// The difference is not only speed. A line is read inside a `PreToolUse`
/// hook, and a hook cancelled at its timeout is killed while the tool call
/// proceeds, with nothing said to the operator or the model
/// (`plugins/harnex/reference/spec-facts.md`). A read that grows with the
/// square of the line is a floor that stops holding once a line is long
/// enough, and reports that to no one.
#[derive(Default)]
struct Settled {
    read: HashMap<Resume, Result<usize, SplitError>>,
    text: HashMap<Resume, Result<usize, SplitError>>,
    /// Every newline in the input, ascending, built when a comment first needs
    /// one. Searching from each `#` instead re-reads the rest of the line once
    /// per comment, which is the same growth by another route.
    newlines: Option<Vec<usize>>,
    /// Bytes stepped across every walk of this input. A state is stepped once
    /// and answered from the table afterwards, so this stays within a small
    /// multiple of the input's length however many `$(` are written on it.
    /// That is the property the table exists for, and the one a test can hold
    /// at any size on any machine where a wall-clock bound could not.
    steps: usize,
}

impl Settled {
    fn of(
        &mut self,
        quoted: QuotedSubstitution,
    ) -> &mut HashMap<Resume, Result<usize, SplitError>> {
        match quoted {
            QuotedSubstitution::Read => &mut self.read,
            QuotedSubstitution::Text => &mut self.text,
        }
    }

    /// The first newline at or after `i`, or the end of the input.
    fn line_end(&mut self, input: &str, i: usize) -> usize {
        let newlines = self.newlines.get_or_insert_with(|| {
            input
                .bytes()
                .enumerate()
                .filter_map(|(at, b)| (b == b'\n').then_some(at))
                .collect()
        });
        let next = newlines.partition_point(|&at| at < i);
        newlines.get(next).copied().unwrap_or(input.len())
    }
}

/// A frame the walk holds open, and the resume states it stepped through while
/// this frame was the top one. The byte that closes the frame is the answer to
/// every one of them, so they are settled together the moment it arrives.
struct Frame {
    kind: Open,
    pending: Vec<Resume>,
}

impl Frame {
    fn open(kind: Open) -> Self {
        Self {
            kind,
            pending: Vec::new(),
        }
    }
}

fn body_end(
    input: &str,
    start: usize,
    quoted: QuotedSubstitution,
    settled: &mut Settled,
) -> Result<usize, SplitError> {
    let mut open = vec![Frame::open(Open::Substitution)];
    let outcome = walk_body(input, start, false, quoted, &mut open, settled);
    // A frame still held is one the walk never closed, and a walk beginning at
    // any state it stepped through under that frame ends the same way.
    if let Err(ref error) = outcome {
        let table = settled.of(quoted);
        for frame in open.iter() {
            for key in frame.pending.iter() {
                table.insert(*key, Err(error.clone()));
            }
        }
    }
    outcome
}

/// Answer every state `frame` was waiting on, now that it has closed at `end`.
fn close(frame: Frame, end: usize, quoted: QuotedSubstitution, settled: &mut Settled) {
    let table = settled.of(quoted);
    for key in frame.pending {
        table.insert(key, Ok(end));
    }
}

/// Steps from `start` until the frame on top of `open` closes, and answers with
/// the byte just past it. Any frame will do: a walk reads the one on top, so
/// what it answers is that frame's end whatever sits beneath it.
fn walk_body(
    input: &str,
    start: usize,
    word_open: bool,
    quoted: QuotedSubstitution,
    open: &mut Vec<Frame>,
    settled: &mut Settled,
) -> Result<usize, SplitError> {
    let bytes = input.as_bytes();
    let mut i = start;
    let mut in_word = word_open;
    // No byte index equals the sentinel, so the first turn passes it.
    let mut previous = usize::MAX;
    while i < bytes.len() {
        // Every turn advances, which is what bounds the states recorded below
        // by the length of the input rather than leaving them to grow without
        // end.
        debug_assert!(
            i != previous,
            "the walk stalled at byte {i} of {}",
            input.len()
        );
        previous = i;
        settled.steps += 1;
        let top = open.last().expect("the walk holds its own frame").kind;
        let here = (i, top == Open::DoubleQuote, in_word);
        match settled.of(quoted).get(&here).cloned() {
            Some(Err(error)) => return Err(error),
            Some(Ok(end)) => {
                let frame = open.pop().expect("the frame just read");
                close(frame, end, quoted, settled);
                i = end;
                if open.is_empty() {
                    return Ok(end);
                }
                // A closed quote and a closed substitution both end part of a
                // word; a closed subshell ends a command.
                in_word = top != Open::Subshell;
                continue;
            }
            None => open
                .last_mut()
                .expect("the walk holds its own frame")
                .pending
                .push(here),
        }
        if top == Open::DoubleQuote {
            match bytes[i] {
                b'"' => {
                    let frame = open.pop().expect("the quote frame");
                    i += 1;
                    close(frame, i, quoted, settled);
                    if open.is_empty() {
                        return Ok(i);
                    }
                    in_word = true;
                }
                b'\\' => i += 2,
                b'$' if quoted == QuotedSubstitution::Read && bytes.get(i + 1) == Some(&b'(') => {
                    open.push(Frame::open(Open::Substitution));
                    in_word = false;
                    i += 2;
                }
                _ => i += 1,
            }
            continue;
        }
        match bytes[i] {
            // A comment runs to the newline, and the shell drops it before it
            // is anything: a quote or a paren written there is neither. Sought
            // from past the `#`, which finds the same newline — this byte is
            // not one — and leaves the turn advancing by arithmetic rather than
            // by what the index holds.
            b'#' if !in_word => {
                i = settled.line_end(input, i + 1);
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
                open.push(Frame::open(Open::Substitution));
                i += 2;
            }
            b'\'' => {
                let end = input[i + 1..]
                    .find('\'')
                    .ok_or(SplitError::UnterminatedSingleQuote)?;
                i += end + 2;
            }
            b'"' => {
                open.push(Frame::open(Open::DoubleQuote));
                i += 1;
            }
            b'(' => {
                open.push(Frame::open(Open::Subshell));
                i += 1;
            }
            b')' => {
                let frame = open.pop().expect("the walk holds its own frame");
                i += 1;
                let closes_substitution = frame.kind == Open::Substitution;
                close(frame, i, quoted, settled);
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
/// `None` where the scan cannot follow the shell's grammar far enough to
/// delimit the span at all — a `case` pattern's `)`, a heredoc written inside
/// the body.
///
/// Where nothing expands the span is still the substitution's, because a span
/// is grammar: the receiving shell reading a literal body delimits it the same
/// way, and an expanded body had the whole of it replaced before that shell
/// saw a byte. Only what is reported differs, and there the text is a mention
/// and runs nothing.
///
/// Reading a body is an addition to what this parser used to do, so `None`
/// returns each caller to what it did with a `$(` before there was one:
/// unquoted, the span the paren count gives; inside double quotes, two
/// ordinary characters. Neither costs the line a verdict it already had.
fn read_substitution(
    input: &str,
    dollar: usize,
    depth: usize,
    expansion: Expansion,
    settled: &mut Settled,
) -> Option<(usize, Vec<Vec<String>>)> {
    let start = dollar + 2;
    let end = substitution_end(input, start, settled).ok()?;
    if expansion == Expansion::Mention || depth >= MAX_NESTING_DEPTH {
        return Some((end, Vec::new()));
    }
    let (commands, _) = split_nested(&input[start..end - 1], depth + 1, Expansion::Runs);
    Some((end, commands))
}

/// Whether the shell expands the text being scanned before anything reads it.
/// A command line and a substitution body expand; a heredoc body does not,
/// having reached the receiving program byte for byte, so what expands in it
/// is that program's to do and a `$(…)` there is a mention.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expansion {
    Runs,
    Mention,
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
    let (commands, error) = split_nested(input, 0, Expansion::Runs);
    Split { commands, error }
}

fn split_nested(
    input: &str,
    depth: usize,
    expansion: Expansion,
) -> (Vec<Vec<String>>, Option<SplitError>) {
    let mut acc = Accumulator::default();
    let error = scan(input, depth, expansion, &mut acc).err();
    acc.push_command();
    (acc.commands, error)
}

fn scan(
    input: &str,
    depth: usize,
    expansion: Expansion,
    acc: &mut Accumulator,
) -> Result<(), SplitError> {
    let bytes = input.as_bytes();
    let mut settled = Settled::default();
    let mut i = 0;
    let mut queued: Vec<Heredoc> = Vec::new();
    // A heredoc body is read where it ends rather than where it stops being
    // readable, so an error inside one is the line's only if nothing else
    // stopped the scan, and a bypass standing after the body is still judged.
    let mut deferred: Option<SplitError> = None;
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
                    match read_substitution(input, i, depth, expansion, &mut settled) {
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
            let (end, commands) = match read_substitution(input, i, depth, expansion, &mut settled)
            {
                Some(read) => read,
                None => (opaque_substitution_end(input, i + 2)?, Vec::new()),
            };
            acc.push_str(&input[i..end]);
            acc.push_substitution(commands);
            i = end;
            continue;
        }
        if b == b'`' {
            let end = backtick_end(input, i + 1)?;
            acc.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if let Some(redirection) = redirection_at(input, i) {
            if matches!(redirection, "<<" | "<<-")
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
                for heredoc in queued.drain(..) {
                    let (body_end, resume) = heredoc_body(input, i, &heredoc);
                    let expanded = expansion == Expansion::Runs && !heredoc.literal;
                    deferred =
                        deferred.or(heredoc_commands(&input[i..body_end], expanded, depth, acc));
                    i = resume;
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
    deferred.map_or(Ok(()), Err)
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
            "echo #\ngit commit --no-verify",
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
        let dense = split("o=$(echo \"$(#)\"\"$(#)\"\n\"$(#)\"; git commit --no-verify)");
        assert!(
            dense.contains(&bypass),
            "a body holding several unclosed substitutions went unread: {dense:?}"
        );
        let escaped = split("o=$(echo \"\\$(git commit --no-verify)\")");
        assert!(
            !escaped.contains(&bypass),
            "an escaped `$(` read as a command: {escaped:?}"
        );
    }

    /// Every entry [`Settled`] takes is the answer a walk of that state
    /// alone gives. One walk answering for the next rests on nothing else, and
    /// an entry that disagreed would hand a later `$(` a body it does not have
    /// — the reading itself is untouched, so this equality is the whole of what
    /// the table has to be right about.
    #[test]
    fn the_table_answers_what_walking_that_state_alone_answers() {
        for input in [
            "$(echo $(echo a) $(echo b))",
            "$(echo \"$(echo a)\" $(echo b))",
            "$(echo $(echo a) $(#)\n)",
            "$(echo \"$(#)\"\"$(#)\"\n\"$(#)\")",
            "$(echo '$(a)' $(b))",
            "$(a\\)b $(c))",
            "$(echo $'\\x28' $(b))",
            "$(echo (a) $(b)",
        ] {
            let mut settled = Settled::default();
            let _ = substitution_end(input, 2, &mut settled);
            let mut checked = 0;
            // The second reading runs only where the first fails, so a body the
            // first reads settles nothing under it.
            for (quoted, name) in [
                (QuotedSubstitution::Read, "Read"),
                (QuotedSubstitution::Text, "Text"),
            ] {
                let recorded: Vec<(Resume, Result<usize, SplitError>)> = settled
                    .of(quoted)
                    .iter()
                    .map(|(key, value)| (*key, value.clone()))
                    .collect();
                checked += recorded.len();
                for ((at, quote_on_top, in_word), answer) in recorded {
                    let frame = if quote_on_top {
                        Open::DoubleQuote
                    } else {
                        Open::Substitution
                    };
                    let mut alone = Settled::default();
                    let walked = walk_body(
                        input,
                        at,
                        in_word,
                        quoted,
                        &mut vec![Frame::open(frame)],
                        &mut alone,
                    );
                    assert_eq!(
                        walked, answer,
                        "{input:?}: the table answers byte {at} (quote on top {quote_on_top}, \
                         in_word {in_word}) under {name} differently from a walk that starts \
                         there"
                    );
                }
            }
            assert!(
                checked > 0,
                "{input:?} settled nothing at all, so this case proves nothing"
            );
        }
    }

    /// Settling every `$(` on a line through one table answers what settling
    /// each on a table of its own answers. The table is a shortcut, so sharing
    /// it may not change a verdict, and a key that leaves out state breaks
    /// exactly here: entries an earlier `$(` wrote are read by a later one that
    /// is in a different state, and only the shared run is wrong.
    ///
    /// Checking each entry by re-walking it from its own key cannot see that —
    /// the entry and the re-walk read the same key, so a key missing state
    /// agrees with itself. This drives the table the way `scan` drives it
    /// instead, over lines built from the constructs the scan branches on, to
    /// the depth where two walks first reach one byte in different states.
    #[test]
    fn sharing_the_table_answers_what_settling_each_alone_answers() {
        const FRAGMENTS: [&str; 12] = [
            "$(", ")", "\"", "#", "\n", "\\\n", "'", "x", " ", "(", "$(x)", "`",
        ];
        let mut lines = vec![String::from("\"$(#$(x\\\n#)\"")];
        let mut digits = [0usize; 4];
        loop {
            lines.push(digits.iter().map(|&d| FRAGMENTS[d]).collect());
            let Some(place) = digits.iter().rposition(|&d| d + 1 < FRAGMENTS.len()) else {
                break;
            };
            digits[place] += 1;
            digits[place + 1..].fill(0);
        }

        for line in lines {
            let mut shared = Settled::default();
            let mut at = 0;
            while let Some(found) = line[at..].find("$(") {
                at += found + 2;
                let together = substitution_end(&line, at, &mut shared);
                let alone = substitution_end(&line, at, &mut Settled::default());
                assert_eq!(
                    together, alone,
                    "sharing the table changed the body at byte {at} of {line:?}"
                );
            }
        }
    }

    /// Lines that used to cost a pass over the rest of the line for every `$(`
    /// written on it. They differ in what stood between one pass and the next
    /// — a quote and a subshell paren, a comment reaching for a newline that is
    /// not there, a heredoc drained before every substitution, a body neither
    /// reading closes — and each was found by measuring rather than by reading
    /// the code, so each stays as its own case.
    ///
    /// The count holds the table and nothing else. A pass per `$(` is that
    /// count rising with the square of the line, which shows at any size, on
    /// any machine and under either build profile, where a wall-clock bound
    /// would show it only on a long enough line and a fast enough build — and
    /// tests here run unoptimised. What it cannot see is work done inside one
    /// step: reaching for a comment's end without the newline index stays one
    /// step however far it scans. That one is held by its own shape being here
    /// at all, and the commit carries what it measured.
    #[test]
    fn a_line_is_stepped_once_however_many_substitutions_it_carries() {
        let dense = vec!["\"$(#)\"".repeat(13); 400].join("\n");
        let bodies = [
            (
                "a quote and a paren between the passes",
                vec!["\"$(#\"("; 4_000].join("\n"),
            ),
            (
                "no newline for the comment to end at",
                "\"$( #\"".repeat(4_000),
            ),
            (
                "a heredoc drained before every substitution",
                format!("{}{}", "cat <<'E'\nE\n".repeat(2_000), "$(x)".repeat(2_000)),
            ),
            ("a body neither reading closes", format!("echo \"{dense}")),
        ];
        let bypass = owned(&[&["git", "commit", "--no-verify"]]).remove(0);
        for (name, body) in bodies {
            let line = format!("git commit --no-verify; {body}");
            assert!(
                split(&line).contains(&bypass),
                "the line ahead of {name} went unjudged"
            );

            // Driven the way `scan` drives it: one settling per `$(` on the
            // line, all of them reading the same table.
            let mut settled = Settled::default();
            let mut settlings = 0;
            let mut at = 0;
            while let Some(found) = line[at..].find("$(") {
                at += found + 2;
                let _ = substitution_end(&line, at, &mut settled);
                settlings += 1;
            }
            // Four states to a byte under two readings bounds the steps that
            // record one at 8n, and every step that instead reads the table
            // closes a frame some earlier step opened, so 16n is the ceiling.
            // These four measure under 4n.
            // A walk steps at least once before it can read the table, so the
            // count is never under the settlings that drove it. Without a
            // floor a count that never rose would pass the bound.
            assert!(
                settled.steps >= settlings,
                "{name}: {settlings} settlings stepped {} bytes",
                settled.steps
            );
            let bound = 16 * line.len();
            assert!(
                settled.steps <= bound,
                "{name}: stepped {} bytes of a {}-byte line, past {bound} — it is being read \
                     once per `$(` again",
                settled.steps,
                line.len()
            );
        }
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

    /// A `<<` is an operator only on a line the shell reads as a command line.
    /// Body text and a delimiter line are neither, and a heredoc queued from
    /// one ends the outer body at the inner delimiter — leaving the lines
    /// between read as the shell never reads them, or, under a quoted inner
    /// delimiter, not read at all. The second is the silent pass.
    ///
    /// Measured with `$(echo RAN >&2)` in place of the bypass: bash 5.3, bash
    /// 3.2 and zsh 5.9 all print RAN.
    #[test]
    fn a_heredoc_operator_off_the_command_line_opens_nothing() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "cat <<A <<'B'\n<<'C'\no=$(git commit --no-verify -m x)\nC\nA\nbody\nB",
            "cat <<A\n<<'C'\no=$(git commit --no-verify -m x)\nC\nA",
            "cat <<A\n<<C\no=$(git commit --no-verify -m x)\nC\nA",
            // Spelled in the delimiter word, so the line that ends the body
            // carries it and the command after that line is the next command.
            "cat <<\"<<'C'\"\nhello\n<<'C'\necho $(git commit --no-verify -m x)\nC",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a `<<` off the command line opened one: {line} gave {:?}",
                split(line)
            );
        }
        for after in [
            // The outer body still ends where its own delimiter says, so what
            // follows the heredoc keeps its verdict.
            "cat <<A\n<<'C'\ntext\nC\nA\ngit commit --no-verify -m x",
            // Two heredocs whose first delimiter spells an operator: the
            // second body starts after the first delimiter line, not before it.
            "cat <<'<<X' <<'B'\nbody1\n<<X\nbody2\nB\ngit commit --no-verify -m x",
        ] {
            assert!(
                split(after).contains(&bypass),
                "the line after the body lost its verdict: {after} gave {:?}",
                split(after)
            );
        }
    }

    /// An escaped backtick inside a span is the span's own text, not its end.
    /// Ending the span at the first backtick found leaves the rest to be read
    /// as something else, and the command standing after the escape goes with
    /// it — in a body the shell expands and on the command line alike. The
    /// third line is where the span ends somewhere else entirely rather than
    /// one character out, which is what a step that is not two bytes does.
    ///
    /// Measured: bash 5.3 and bash 3.2 run git on all three, zsh on the first
    /// two.
    #[test]
    fn ends_a_backtick_span_where_an_escape_does_not() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "cat <<EOF\ncost `echo \\`z\\` ; git commit --no-verify -m x`\nEOF",
            "o=`x \\`y\\`` ; git commit --no-verify -m x",
            "o=`abc\\`x` ; git commit --no-verify -m x",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a span ended at an escaped backtick: {line} gave {:?}",
                split(line)
            );
        }
    }

    /// A backtick body reaches its own parser with the three escapes the shell
    /// removes removed and every other backslash still standing. Removing one
    /// the shell keeps opens a quote the text never opened; keeping one the
    /// shell removes leaves a `$` that never expands.
    ///
    /// Measured: bash 5.3, bash 3.2 and zsh 5.9 run git on the first two and
    /// on none of the third.
    #[test]
    fn hands_a_backtick_body_on_as_the_shell_does() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            // The escape stands first in the body, and `\$` is one the shell
            // removes.
            "cat <<EOF\ncost `\\$(git commit --no-verify -m x)`\nEOF",
            // `\'` is one it keeps, so the quote is a character and the
            // substitution beside it still expands.
            "cat <<EOF\ncost `echo \\'$(git commit --no-verify -m x)\\'`\nEOF",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a body was handed on wrong: {line} gave {:?}",
                split(line)
            );
        }
        // An unescaped quote does open one, and what it holds is text.
        let quoted = "cat <<EOF\ncost `echo '$(git commit --no-verify -m x)'`\nEOF";
        assert!(
            !split(quoted).contains(&bypass),
            "a quoted mention was read as a command: {:?}",
            split(quoted)
        );
    }

    /// What acts inside a body the shell expands. A backslash escapes only
    /// `$`, a backtick, itself and a newline there, so `\$(…)` is text while
    /// `\\$(…)` is an escaped backslash in front of a substitution. The
    /// spellings are quoted because only then does the escape decide the
    /// verdict: unquoted, reading the body as a script finds the same command
    /// either way.
    ///
    /// A backtick pair is read here, unlike on the command line. There the
    /// mark is also the code span's and pairing runs across prose, so a
    /// mention lands inside a pair by accident; a body the shell expands has
    /// its backticks paired by the shell the same way and run, so what is
    /// refused is what the line runs. What that pair holds is then a command
    /// line, and a backtick written inside it keeps the command line's answer.
    ///
    /// Measured on bash 5.3, bash 3.2 and zsh 5.9: none runs git on the
    /// escaped `$`, and all run it on every other line here, the last one
    /// included.
    #[test]
    fn reads_a_body_the_way_the_shell_expands_one() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        let mention = "cat <<EOF\n'\\$(git commit --no-verify -m x)'\nEOF";
        assert!(
            !split(mention).contains(&bypass),
            "a mention was read as a command: {:?}",
            split(mention)
        );
        for line in [
            "cat <<EOF\n'\\\\$(git commit --no-verify -m x)'\nEOF",
            "cat <<EOF\ncost `git commit --no-verify -m x`\nEOF",
            // An escaped backtick is one of the body's own characters and
            // opens no span, so the pair after it is where one begins.
            "cat <<EOF\ncost \\`x\\` and `git commit --no-verify -m x`\nEOF",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a body the shell expands went unread: {line} gave {:?}",
                split(line)
            );
        }
        // Inside the pair the command line's reading takes over, and there a
        // backtick body is not read. The shells run this one.
        let nested = "cat <<EOF\ncost `echo \\`git commit --no-verify -m x\\``\nEOF";
        assert!(
            !split(nested).contains(&bypass),
            "the inner backtick was read after all: {:?}",
            split(nested)
        );
    }

    /// A body the scan cannot read leaves the line a skip, and leaves the
    /// command line alone: what stands after the delimiter line is read and
    /// judged all the same, because the body's quote is bounded to the body.
    #[test]
    fn keeps_the_command_line_past_a_heredoc_body_it_cannot_read() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "cat <<'EOF'\nit's done\nEOF\ngit commit --no-verify -m x",
            "cat <<EOF\ncost $(x\nEOF\ngit commit --no-verify -m x",
            "cat <<'EOF'\ncost $(x\nEOF\ngit commit --no-verify -m x",
        ] {
            let read = split_commands(line);
            assert!(
                read.error.is_some(),
                "an unreadable body reported nothing: {line}"
            );
            assert!(
                read.commands.contains(&bypass),
                "the line after the body lost its verdict: {line} gave {:?}",
                read.commands
            );
        }
    }

    /// The two readings of a body stop in different places, so the line keeps
    /// whichever error it meets. Quoting and a `#` end the script reading
    /// before a `$(` the shell expands regardless, and a substitution nothing
    /// can close is a command list nothing has read.
    ///
    /// Measured: all three shells report a syntax error on the first two, so
    /// what the floor cannot read is not something they run either.
    #[test]
    fn keeps_the_error_of_whichever_reading_meets_one() {
        for line in ["cat <<EOF\n'cost $(x'\nEOF", "cat <<EOF\n# cost $(x\nEOF"] {
            assert!(
                split_commands(line).error.is_some(),
                "the reading that meets an error was not the one asked: {line}"
            );
        }
        // Under a quoted delimiter nothing expands, so the reading that would
        // meet it never runs and the same text is the body's own.
        assert!(
            split_commands("cat <<'EOF'\n'cost $(x'\nEOF")
                .error
                .is_none()
        );
    }

    /// A span is grammar, so the reading that reports no command from a `$(…)`
    /// still ends it where the shell ends it. Reading the span by paren count
    /// instead leaves the quoting inside it loose, and the quote left over
    /// pairs with one further down the body and swallows the command between.
    ///
    /// Measured: bash 5.3, bash 3.2 and zsh 5.9 run git on every line here.
    #[test]
    fn ends_a_mentioned_substitution_where_its_own_grammar_ends_it() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "bash <<EOF\nv=\"$(echo \"it's\")\"\ngit commit --no-verify -m x\nEOF",
            "bash <<EOF\nn=\"$(grep -c \"'\" /dev/null)\"\ngit commit --no-verify -m x\necho 'done'\nEOF",
            "sh <<'EOF'\nx=$(echo \")\")\ngit commit --no-verify -m x\nEOF",
            "sh <<'EOF'\nx=$(echo \"(\")\ngit commit --no-verify -m x\nEOF",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a mentioned substitution ran past its own end: {line} gave {:?}",
                split(line)
            );
        }
    }

    /// Two programs read a body, and neither reading may reach past it. The
    /// shell expands a bare-delimiter body with only `\\`, `$` and a backtick
    /// special, so quoting and `#` in the text hide nothing from it; the
    /// receiving program reads what it was handed, and a quote opened in that
    /// text is the body's rather than the command line's.
    ///
    /// Measured: bash 5.3, bash 3.2 and zsh 5.9 all run git on each of these.
    #[test]
    fn neither_reading_of_a_body_reaches_past_it() {
        let bypass = owned(&[&["git", "commit", "--no-verify", "-m", "x"]]).remove(0);
        for line in [
            "cat <<EOF\nit's $(git commit --no-verify -m x) isn't\nEOF",
            "cat <<EOF\n# built at $(git commit --no-verify -m x)\nEOF",
            "cat <<'EOF'\nit's done\nEOF\ngit commit --no-verify -m x",
            "cat <<EOF\nit's done\nEOF\ngit commit --no-verify -m x",
            "cat <<'EOF'\nopen (\nEOF\ngit commit --no-verify -m x",
            "cat <<EOF\n$(echo one\nEOF\ngit commit --no-verify -m x",
        ] {
            assert!(
                split(line).contains(&bypass),
                "a body reached past itself: {line} gave {:?}",
                split(line)
            );
        }
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
            // Nothing expands inside a literal body, the heredoc written in
            // one included. All three shells run git on this one; it is a
            // second shell running a script, which is out of scope however
            // deep the spelling goes, and reading it is what refused the
            // documents that explain this module.
            "bash <<'OUTER'\ncat <<INNER\no=$(git commit --no-verify -m x)\nINNER\nOUTER",
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

    /// A heredoc body is read as commands of its own, so a heredoc written in
    /// one recurses the parser exactly as a substitution does and answers to
    /// the same bound. Without it a line short enough for any shell faults
    /// instead of answering.
    #[test]
    fn stops_reading_a_heredoc_nested_past_the_bound() {
        let nest = MAX_NESTING_DEPTH + 2;
        let open: String = (0..nest).map(|d| format!("cat <<'E{d}'\n")).collect();
        let deep = format!("{open}git commit --no-verify -m x\n");
        assert!(
            !split(&deep).contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
            "a body past the bound was read: {:?}",
            split(&deep)
        );

        // Well inside the bound the same nesting is read, so the bound is what
        // stops the deep one rather than the nesting being unread throughout.
        let shallow = "cat <<'A'\ncat <<'B'\ngit commit --no-verify -m x\nB\nA";
        assert!(
            split(shallow).contains(&owned(&[&["git", "commit", "--no-verify", "-m", "x"]])[0]),
            "a body inside the bound went unread: {:?}",
            split(shallow)
        );
    }

    /// Past the bound the body stops being read, which is what an unreadable
    /// body already is. The line keeps its own commands either way, and the
    /// recursion stays off the stack. Both halves are read from the outside,
    /// because a bound that stopped counting would look the same from here
    /// until the nesting is deep enough to fault instead of answering.
    #[test]
    fn stops_reading_a_substitution_nested_past_the_bound() {
        let nest = MAX_NESTING_DEPTH + 2;
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
        // no evidence that the body is a document. The delimiter line itself
        // is the operator's target and no command.
        for delimiter in ["EOF", "'EOF'", "\"EOF\"", "\\EOF"] {
            assert_eq!(
                split(&format!(
                    "bash <<{delimiter}\ngit commit --no-verify -m x\nEOF"
                )),
                owned(&[&["bash"], &["git", "commit", "--no-verify", "-m", "x"]]),
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

    /// Every spelling of the allocation form, because each character class in
    /// it decides whether the word is dropped or becomes the command word —
    /// and the command word is what the bypass check reads. Measured on bash
    /// 5.3, which is where the form runs at all: `{_fd}` and `{a1}` allocate
    /// and git runs, `{9x}` is no varname so the word stands and nothing runs.
    #[test]
    fn binds_an_fd_prefix_to_its_redirection() {
        assert_eq!(
            split("git commit 2>&1 --no-verify"),
            owned(&[&["git", "commit", "--no-verify"]])
        );
        for prefix in ["{fd}", "{_fd}", "{a1}"] {
            assert_eq!(
                split(&format!("{prefix}>/dev/null git commit --no-verify -m x")),
                owned(&[&["git", "commit", "--no-verify", "-m", "x"]]),
                "{prefix}"
            );
        }
        assert_eq!(
            split("{9x}>/dev/null git commit --no-verify -m x"),
            owned(&[&["{9x}", "git", "commit", "--no-verify", "-m", "x"]])
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
