//! # markdown — the one reader for what a rendered document shows
//!
//! Several gates in this crate ask the same question of a markdown file:
//! which lines does a reader actually see. A fenced block is a sample, an
//! HTML comment is an instruction, and neither carries an assertion about
//! the project — so a checker that reads them anyway reports findings the
//! author never made, and one that mis-tracks them reports a clean pass over
//! a document it never opened.
//!
//! [`Visibility`] is that reader, driven a line at a time; [`doc_lines`]
//! splits the document the way a renderer does, and [`atx_heading`] and
//! [`strip_code_spans`] spell the two inline shapes those gates match on.
//!
//! ## What this module refuses to do
//!
//! - Never renders. It answers what is visible and what a line spells, not
//!   what the output looks like.
//! - Never guesses at a malformed document. A fence or comment left open is
//!   reported as [`Unclosed`] for the caller to judge, because the lines
//!   after it are hidden for a reason no reader can see.
//! - Never normalizes a heading. What an author cites is matched against
//!   what the file spells, so a check cannot resolve a pointer its reader
//!   would not.

/// The renderer's line splitter.
///
/// CommonMark ends a line at a newline, a carriage return not followed by
/// one, or the pair — `str::lines` reads only the first and last, so a
/// classic-Mac document reaches every check as one line the renderer shows
/// as many. A leading U+FEFF is the encoding's byte-order mark, consumed at
/// decode by every reference reader; kept, it would un-head a heading on the
/// first line.
pub(crate) fn doc_lines(text: &str) -> Vec<&str> {
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = Vec::new();
    while !rest.is_empty() {
        match rest.find(['\n', '\r']) {
            Some(at) => {
                lines.push(&rest[..at]);
                let after = &rest[at..];
                rest = after.strip_prefix("\r\n").unwrap_or(&after[1..]);
            }
            None => {
                lines.push(rest);
                break;
            }
        }
    }
    lines
}

/// The ATX heading a line spells, as (level, title) — up to three leading
/// spaces, one to six `#`s, whitespace (or end of line), and an optional
/// closing run of `#`s, per CommonMark. A reader that recognizes fewer
/// heading spellings than a renderer turns a cosmetic trailing `#` into a
/// missing section, and a missing section disarms every check that reads it.
pub(crate) fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let unindented = line.trim_start_matches(' ');
    if line.len() - unindented.len() > 3 {
        return None;
    }
    let level = unindented.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &unindented[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let title = rest.trim_matches([' ', '\t']);
    let stripped = title.trim_end_matches('#');
    if stripped.len() == title.len() {
        return Some((level, title));
    }
    // A closing sequence counts only when whitespace separates it from the
    // title (or it is the whole remainder) — `## a#b` keeps its `#`.
    if stripped.is_empty() {
        return Some((level, stripped));
    }
    match stripped.ends_with([' ', '\t']) {
        true => Some((level, stripped.trim_end_matches([' ', '\t']))),
        false => Some((level, title)),
    }
}

/// Remove CommonMark code spans: a run of N backticks up to the next run of
/// exactly N. An unmatched run stays literal, as the spec reads it.
pub(crate) fn strip_code_spans(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let run = i - start;
            if let Some(close) = find_backtick_run(&text[i..], run) {
                i += close + run;
                continue;
            }
            out.push_str(&text[start..i]);
            continue;
        }
        let ch = text[i..].chars().next().expect("in-bounds char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Byte offset just past a run of exactly `len` backticks in `text`.
fn find_backtick_run(text: &str, len: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - start == len {
                return Some(start);
            }
            continue;
        }
        i += 1;
    }
    None
}

struct Fence {
    marker: u8,
    len: usize,
    line: u32,
}

/// A delimiter still open when the document ended, and the line that opened
/// it. Everything after it is hidden, so a reader that answers anyway is
/// answering about a document the author does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unclosed {
    Fence { line: u32 },
    Comment { line: u32 },
}

/// What a reader sees, line by line.
///
/// Driven over [`doc_lines`] in order: each call advances the fence and
/// comment state and answers with the visible text of that line, or nothing
/// where a reader sees nothing. Fences are settled before comments, because
/// inside a fence a comment marker is content — and inside a comment, a
/// fence marker is invisible.
#[derive(Default)]
pub(crate) struct Visibility {
    fence: Option<Fence>,
    comment: Option<u32>,
}

impl Visibility {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The visible text of `raw`, or `None` where a reader sees nothing —
    /// a fence delimiter, a line inside one, or a line inside a comment.
    pub(crate) fn read(&mut self, raw: &str, line_no: u32) -> Option<String> {
        if self.fence.is_none() && self.comment.is_none() {
            let unindented = raw.trim_start_matches(' ');
            if raw.len() - unindented.len() <= 3 {
                let ticks = unindented.bytes().take_while(|&b| b == b'`').count();
                let tildes = unindented.bytes().take_while(|&b| b == b'~').count();
                // An info string may not contain a backtick, so a line of
                // prose opening with one does not open a block.
                if ticks >= 3 && !unindented[ticks..].contains('`') {
                    self.fence = Some(Fence {
                        marker: b'`',
                        len: ticks,
                        line: line_no,
                    });
                    return None;
                }
                if tildes >= 3 {
                    self.fence = Some(Fence {
                        marker: b'~',
                        len: tildes,
                        line: line_no,
                    });
                    return None;
                }
            }
        }
        if let Some(Fence { marker, len, .. }) = self.fence {
            let unindented = raw.trim_start_matches(' ');
            if raw.len() - unindented.len() <= 3
                && unindented.bytes().take_while(|&b| b == marker).count() >= len
                && unindented.trim_end().bytes().all(|b| b == marker)
            {
                self.fence = None;
            }
            return None;
        }
        strip_html_comments(raw, line_no, &mut self.comment)
    }

    /// The delimiter still open, if the document ended inside one.
    pub(crate) fn unclosed(&self) -> Option<Unclosed> {
        if let Some(Fence { line, .. }) = self.fence {
            return Some(Unclosed::Fence { line });
        }
        self.comment.map(|line| Unclosed::Comment { line })
    }
}

/// The part of `line` a renderer shows, carrying comment state across lines.
///
/// Stripped span-wise so prose around an inline comment still reads, and
/// code spans are settled first so a `<!--` an author quoted stays literal.
fn strip_html_comments(line: &str, line_no: u32, comment: &mut Option<u32>) -> Option<String> {
    let mut out = String::new();
    let mut rest = line;
    if comment.is_some() {
        let end = rest.find("-->")?;
        *comment = None;
        rest = &rest[end + 3..];
    }
    while let Some(open) = rest.find("<!--") {
        if let Some(tick) = rest.find('`')
            && tick < open
        {
            let run = rest[tick..].bytes().take_while(|&b| b == b'`').count();
            if let Some(close) = find_backtick_run(&rest[tick + run..], run) {
                let span_end = tick + run + close + run;
                out.push_str(&rest[..span_end]);
                rest = &rest[span_end..];
                continue;
            }
        }
        out.push_str(&rest[..open]);
        let after = &rest[open + 4..];
        // `<!-->` and `<!--->` are complete, empty comments to a renderer —
        // read as unterminated openers, they swallowed the visible rows
        // after them.
        if let Some(tail) = after.strip_prefix('>') {
            rest = tail;
            continue;
        }
        if let Some(tail) = after.strip_prefix("->") {
            rest = tail;
            continue;
        }
        match after.find("-->") {
            Some(end) => rest = &after[end + 3..],
            None => {
                *comment = Some(line_no);
                return Some(out);
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible lines of `text`, as (line number, text).
    fn visible(text: &str) -> Vec<(u32, String)> {
        let mut state = Visibility::new();
        doc_lines(text)
            .into_iter()
            .enumerate()
            .filter_map(|(idx, raw)| {
                let line_no = (idx as u32) + 1;
                state.read(raw, line_no).map(|line| (line_no, line))
            })
            .collect()
    }

    #[test]
    fn a_line_ends_where_the_renderer_ends_it() {
        for (label, text) in [
            ("newlines", "a\nb\nc"),
            ("carriage returns", "a\rb\rc"),
            ("the pair", "a\r\nb\r\nc"),
            ("a byte-order mark", "\u{feff}a\nb\nc"),
        ] {
            assert_eq!(doc_lines(text), vec!["a", "b", "c"], "with {label}");
        }
    }

    #[test]
    fn a_fence_hides_its_delimiters_and_everything_between() {
        assert_eq!(
            visible("before\n```rust\nhidden\n```\nafter"),
            vec![(1, "before".into()), (5, "after".into())]
        );
    }

    #[test]
    fn a_closing_fence_is_at_least_as_long_and_carries_nothing_else() {
        // A four-backtick block quoting three-backtick examples — how a rule
        // about writing rules is spelled — closed itself at the first inner
        // fence, and read the rest of the block as prose.
        assert_eq!(
            visible("````\n```\ninner\n```\n````\nafter"),
            vec![(6, "after".into())]
        );
        assert_eq!(
            visible("```\nhidden\n``` trailing prose\nstill hidden\n```\nafter"),
            vec![(6, "after".into())]
        );
    }

    #[test]
    fn an_info_string_holding_a_backtick_does_not_open_a_block() {
        // A backtick fence's info string may hold no backtick, so this line is
        // prose a renderer shows — and the lines under it are not code.
        assert_eq!(
            visible("```rust `x`\nafter"),
            vec![(1, "```rust `x`".into()), (2, "after".into())]
        );
        // The rule is the info string's, not the run's: a clean one fences.
        assert_eq!(
            visible("```rust\nhidden\n```\nafter"),
            vec![(4, "after".into())]
        );
        // And a tilde fence has no such restriction.
        assert_eq!(
            visible("~~~ `x`\nhidden\n~~~\nafter"),
            vec![(4, "after".into())]
        );
    }

    #[test]
    fn a_comment_hides_its_span_and_a_fence_inside_one_is_invisible() {
        assert_eq!(
            visible("keep <!-- drop --> keep\nafter"),
            vec![(1, "keep  keep".into()), (2, "after".into())]
        );
        assert_eq!(
            visible("<!--\n```\n-->\nafter"),
            vec![(1, "".into()), (3, "".into()), (4, "after".into())]
        );
    }

    #[test]
    fn a_quoted_comment_marker_stays_literal() {
        assert_eq!(
            visible("Write `<!--` to open one.\nafter"),
            vec![(1, "Write `<!--` to open one.".into()), (2, "after".into())]
        );
    }

    #[test]
    fn an_empty_comment_is_complete_rather_than_an_opener() {
        for text in ["<!-->\nafter", "<!--->\nafter"] {
            assert_eq!(
                visible(text).last().map(|(_, l)| l.as_str()),
                Some("after"),
                "from: {text}"
            );
        }
    }

    #[test]
    fn a_delimiter_left_open_is_reported_with_the_line_that_opened_it() {
        let unclosed = |text: &str| {
            let mut state = Visibility::new();
            for (idx, raw) in doc_lines(text).into_iter().enumerate() {
                state.read(raw, (idx as u32) + 1);
            }
            state.unclosed()
        };
        assert_eq!(unclosed("a\n```\nb"), Some(Unclosed::Fence { line: 2 }));
        assert_eq!(unclosed("a\n<!--\nb"), Some(Unclosed::Comment { line: 2 }));
        assert_eq!(unclosed("a\n```\nb\n```"), None);
    }

    #[test]
    fn a_heading_is_read_as_a_renderer_reads_it() {
        for (line, want) in [
            ("# One", Some((1, "One"))),
            ("###### Six", Some((6, "Six"))),
            ("   ## Indented", Some((2, "Indented"))),
            ("## Closed ##", Some((2, "Closed"))),
            ("## a#b", Some((2, "a#b"))),
            ("## ", Some((2, ""))),
            ("    # Four spaces is code", None),
            ("#No space", None),
            ("####### Seven", None),
            ("Not a heading", None),
        ] {
            assert_eq!(atx_heading(line), want, "from: {line}");
        }
    }

    #[test]
    fn a_code_span_is_removed_and_an_unmatched_run_stays() {
        assert_eq!(strip_code_spans("a `b` c"), "a  c");
        assert_eq!(strip_code_spans("a ``b`` c"), "a  c");
        assert_eq!(strip_code_spans("a ` b c"), "a ` b c");
    }
}
