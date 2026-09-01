//! # markdown — the one reader for what a rendered document shows
//!
//! Several gates in this crate ask the same question of a markdown file:
//! which lines does a reader actually see, and what heading does a line
//! spell. A fenced block is a sample and an HTML comment is an instruction,
//! so neither carries an assertion about the project — and a checker that
//! reads them anyway reports findings the author never made.
//!
//! [`Document`] answers both from one CommonMark parse. The subset was
//! hand-rolled until it produced four false Blockers in a gate that is on by
//! default: a fenced sample indented inside a list item, a fence CommonMark
//! terminates at the end of the document, and a heading spelled setext or
//! carrying emphasis. Each needs container or inline context that a
//! line-at-a-time reader does not have, which is why the parser is a
//! dependency rather than a subset.
//!
//! ## What this module refuses to do
//!
//! - Never renders. It answers what is visible and what a heading says, not
//!   what the output looks like.
//! - Never normalizes a heading beyond what the renderer does. `## **A**`
//!   and `## A` are one heading text because a reader sees one, and a
//!   citation is matched against that — but nothing folds case, collapses
//!   whitespace, or strips punctuation, so a check cannot resolve a pointer
//!   its reader would not.
//! - Never hides an inline code span. Quoting a reserved marker in backticks
//!   is how a document mentions one, and `.claude/rules/audit.md` settles
//!   where an example goes instead: a fenced block, an HTML comment, or
//!   paraphrase.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// One line a reader sees, with the spans a reader does not removed.
pub(crate) struct Line {
    pub(crate) no: u32,
    pub(crate) text: String,
    /// An indent made this line code, where a fence would have been a
    /// deliberate quotation. The two are one thing to a renderer and two to
    /// a gate: a gate reading what a document asserts skips both, and a gate
    /// looking for what its author wrote reads this one, because a stray tab
    /// is how a line meant as prose stops being read.
    pub(crate) indented_code: bool,
}

/// One heading, as a reader reads it: the inline text with its markup
/// resolved, whatever spelling the source used.
pub(crate) struct Heading {
    pub(crate) level: u32,
    pub(crate) text: String,
    pub(crate) line: u32,
}

/// A delimiter the document ends inside.
///
/// CommonMark closes both at the end of the document, so neither is an error
/// — but everything the author wrote after the opener is inside it, which is
/// the shape of a line believed recorded and never read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unterminated {
    Fence { line: u32 },
    Comment { line: u32 },
}

/// A markdown document, read once.
pub(crate) struct Document {
    lines: Vec<Line>,
    headings: Vec<Heading>,
    unterminated: Option<Unterminated>,
}

impl Document {
    pub(crate) fn of(source: &str) -> Self {
        let text = normalize(source);
        let mut quoted: Vec<Range<usize>> = Vec::new();
        let mut indented: Vec<Range<usize>> = Vec::new();
        let mut headings: Vec<Heading> = Vec::new();
        let mut open_heading: Option<Heading> = None;
        let mut unterminated = None;
        let mut open_fence: Option<Fence> = None;
        let mut html_run: Option<Range<usize>> = None;

        let options = Options::ENABLE_TABLES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
        for (event, range) in Parser::new_ext(&text, options).into_offset_iter() {
            if !matches!(event, Event::Html(_)) {
                html_run = None;
            }
            match event {
                Event::Start(Tag::CodeBlock(kind)) => match kind {
                    CodeBlockKind::Fenced(_) => {
                        open_fence = Some(Fence {
                            line: line_of(&text, range.start),
                            end: range.end,
                            content_end: line_end(&text, range.start),
                        });
                        quoted.push(range);
                    }
                    CodeBlockKind::Indented => indented.push(range),
                },
                Event::End(TagEnd::CodeBlock) => {
                    if let Some(fence) = open_fence.take()
                        && fence.end <= fence.content_end
                    {
                        unterminated = Some(Unterminated::Fence { line: fence.line });
                    }
                }
                Event::Html(_) => {
                    html_run = Some(match html_run {
                        Some(open) => open.start..range.end,
                        None => range.clone(),
                    });
                    if let Some(run) = html_run.as_ref()
                        && run.end == text.len()
                    {
                        let block = &text[run.clone()];
                        if block.starts_with("<!--") && !block.contains("-->") {
                            unterminated = Some(Unterminated::Comment {
                                line: line_of(&text, run.start),
                            });
                        }
                    }
                    quoted.push(range);
                }
                Event::Start(Tag::MetadataBlock(_)) | Event::InlineHtml(_) => quoted.push(range),
                Event::Start(Tag::Heading { level, .. }) => {
                    open_heading = Some(Heading {
                        level: level as u32,
                        text: String::new(),
                        line: line_of(&text, range.start),
                    });
                }
                Event::End(TagEnd::Heading(_)) => headings.extend(open_heading.take()),
                Event::Text(run) | Event::Code(run) => {
                    if let Some(fence) = open_fence.as_mut() {
                        fence.content_end = range.end;
                    }
                    if let Some(heading) = open_heading.as_mut() {
                        heading.text.push_str(&run);
                    }
                }
                _ => {}
            }
        }

        Self {
            lines: read_lines(&text, &quoted, &indented),
            headings,
            unterminated,
        }
    }

    /// The lines a reader sees, in order.
    pub(crate) fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub(crate) fn headings(&self) -> &[Heading] {
        &self.headings
    }

    /// The heading a line opens, if it opens one.
    pub(crate) fn heading_at(&self, line: u32) -> Option<&Heading> {
        self.headings.iter().find(|heading| heading.line == line)
    }

    pub(crate) fn unterminated(&self) -> Option<Unterminated> {
        self.unterminated
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

/// The document as a renderer decodes it.
///
/// CommonMark ends a line at a newline, a carriage return not followed by
/// one, or the pair, and a leading U+FEFF is the encoding's byte-order mark
/// that every reference reader consumes. The parser handles neither: a
/// classic-Mac document reaches it as one line, and a byte-order mark
/// un-heads the first. Rewriting the terminators keeps the line count, so a
/// line number here is the line number in the file.
fn normalize(source: &str) -> String {
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// How many lines a reader sees the document as, terminators and all.
///
/// A carriage return ends a line, so a document written with them is as long
/// as it looks — counted with `str::lines` it is one, and every claim into
/// it fails as out of range.
pub(crate) fn line_count(source: &str) -> u32 {
    let text = normalize(source);
    let lines = text.strip_suffix('\n').unwrap_or(&text);
    match lines.is_empty() {
        true => 0,
        false => u32::try_from(lines.split('\n').count()).unwrap_or(u32::MAX),
    }
}

/// The 1-indexed line an offset falls on.
fn line_of(text: &str, offset: usize) -> u32 {
    let count = text[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// A fenced block being read, and how far its content reaches.
///
/// A closing fence is the one line of a fenced block that is neither the
/// opener nor content, so a block whose extent runs past its content closed
/// itself and one that stops there was closed by the document. Both numbers
/// come from the parser, which is what keeps this from re-deriving the fence
/// rule it would then get wrong — a closing fence carries its container's
/// indent inside a list and its `>` inside a block quote, and a run shorter
/// than the opener closes nothing.
struct Fence {
    line: u32,
    end: usize,
    content_end: usize,
}

/// The offset just past the line `at` falls on.
fn line_end(text: &str, at: usize) -> usize {
    text[at..]
        .find('\n')
        .map_or(text.len(), |offset| at + offset + 1)
}

/// Each source line as a reader meets it: quoted spans removed, a line an
/// indent turned into code kept whole and marked, and a line a reader sees
/// nothing of dropped.
fn read_lines(text: &str, quoted: &[Range<usize>], indented: &[Range<usize>]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, raw) in text.split('\n').enumerate() {
        let end = start + raw.len();
        let no = u32::try_from(idx + 1).unwrap_or(u32::MAX);

        // An indented block's range opens at its content, past the indent
        // that made it code, so the line is kept as written rather than
        // masked — its indentation is the evidence.
        if indented
            .iter()
            .any(|range| range.start < end && range.end > start)
        {
            out.push(Line {
                no,
                text: raw.to_string(),
                indented_code: true,
            });
            start = end + 1;
            continue;
        }

        let mut visible = String::with_capacity(raw.len());
        let mut at = start;
        while at < end {
            match quoted.iter().find(|range| range.contains(&at)) {
                Some(range) => at = range.end.min(end),
                None => {
                    let next = quoted
                        .iter()
                        .filter(|range| range.start > at && range.start < end)
                        .map(|range| range.start)
                        .min()
                        .unwrap_or(end);
                    visible.push_str(&text[at..next]);
                    at = next;
                }
            }
        }
        if !raw.is_empty() && visible.is_empty() {
            start = end + 1;
            continue;
        }
        out.push(Line {
            no,
            text: visible,
            indented_code: false,
        });
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(text: &str) -> Vec<(u32, String)> {
        Document::of(text)
            .lines()
            .iter()
            .map(|line| (line.no, line.text.clone()))
            .collect()
    }

    fn headings(text: &str) -> Vec<(u32, String, u32)> {
        Document::of(text)
            .headings()
            .iter()
            .map(|h| (h.level, h.text.clone(), h.line))
            .collect()
    }

    #[test]
    fn a_line_ends_where_the_renderer_ends_it() {
        // A classic-Mac document reached the parser as one line, which read
        // the whole file as a single fence-opening line: every claim in it
        // went unparsed and the gate reported clean.
        for (label, text) in [
            ("newlines", "a\n\n```\nhidden\n```\n\nb\n"),
            ("carriage returns", "a\r\r```\rhidden\r```\r\rb\r"),
            ("the pair", "a\r\n\r\n```\r\nhidden\r\n```\r\n\r\nb\r\n"),
            ("a byte-order mark", "\u{feff}a\n\n```\nhidden\n```\n\nb\n"),
        ] {
            let seen: Vec<String> = visible(text).into_iter().map(|(_, t)| t).collect();
            assert!(
                seen.contains(&"a".to_string()) && seen.contains(&"b".to_string()),
                "with {label}: {seen:?}"
            );
            assert!(!seen.contains(&"hidden".to_string()), "with {label}");
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
    fn a_sample_stays_hidden_wherever_its_container_indents_it() {
        // The measured false Blocker: a fenced example inside a list item is
        // indented four spaces, which a line-at-a-time reader rejects as a
        // fence opener — so the sample became a live claim against a path
        // the author wrote as an example.
        for (label, text) in [
            (
                "inside a list item",
                "- Cite it like this:\n\n    ```markdown\n    SAMPLE\n    ```\n\n- Real.\n",
            ),
            ("inside a block quote", "> ```\n> SAMPLE\n> ```\n\nReal.\n"),
            ("terminated by the document", "Real.\n\n```\nSAMPLE\n"),
        ] {
            let seen: Vec<String> = visible(text).into_iter().map(|(_, t)| t).collect();
            assert!(
                !seen.iter().any(|line| line.contains("SAMPLE")),
                "with {label}: {seen:?}"
            );
            assert!(
                seen.iter().any(|line| line.contains("Real")),
                "with {label}: {seen:?}"
            );
        }
    }

    #[test]
    fn a_nested_bullet_is_not_code() {
        // The other direction, and the reason a four-space indent cannot
        // simply be treated as code: inside a list it is a nested item, and
        // a bullet list naming an owner per item is what a rule is made of.
        let text = "- A nested bullet:\n    - carries live text.\n";
        let seen: Vec<String> = visible(text).into_iter().map(|(_, t)| t).collect();
        assert!(
            seen.iter().any(|line| line.contains("carries live text")),
            "{seen:?}"
        );
    }

    #[test]
    fn a_comment_hides_its_span_and_the_prose_around_it_still_reads() {
        assert_eq!(
            visible("keep <!-- drop --> keep\nafter"),
            vec![(1, "keep  keep".into()), (2, "after".into())]
        );
        let seen: Vec<String> = visible("<!--\nhidden\n-->\nafter\n")
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert!(!seen.contains(&"hidden".to_string()), "{seen:?}");
        assert!(seen.contains(&"after".to_string()), "{seen:?}");
    }

    #[test]
    fn an_incomplete_comment_marker_is_literal_text() {
        // A renderer shows `<!--` with no `-->` as text, so the lines after
        // it are prose — reading them as a comment hid live claims.
        let seen: Vec<String> = visible("Visible <!-- literal\nnext line\n")
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert!(
            seen.iter().any(|line| line.contains("next line")),
            "{seen:?}"
        );
    }

    #[test]
    fn frontmatter_is_data_rather_than_prose() {
        let seen: Vec<String> = visible("---\npaths: SAMPLE\n---\n\nReal.\n")
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert!(!seen.iter().any(|line| line.contains("SAMPLE")), "{seen:?}");
        assert!(seen.iter().any(|line| line.contains("Real")), "{seen:?}");
    }

    #[test]
    fn a_heading_is_what_a_reader_reads_whatever_the_source_spells() {
        assert_eq!(headings("## Storage\n"), vec![(2, "Storage".into(), 1)]);
        assert_eq!(
            headings("Storage\n=======\n"),
            vec![(1, "Storage".into(), 1)]
        );
        assert_eq!(headings("## **Storage**\n"), vec![(2, "Storage".into(), 1)]);
        assert_eq!(headings("## Storage ##\n"), vec![(2, "Storage".into(), 1)]);
        assert_eq!(
            headings("## The `write_atomic` contract\n"),
            vec![(2, "The write_atomic contract".into(), 1)]
        );
        assert_eq!(headings("```\n## Decoy\n```\n"), vec![]);
        assert_eq!(headings("<!--\n## Decoy\n-->\n"), vec![]);
        assert_eq!(
            headings("# One\n\n## Two\n"),
            vec![(1, "One".into(), 1), (2, "Two".into(), 3)]
        );
    }

    #[test]
    fn two_spellings_of_one_heading_are_two_headings() {
        // The count is what a section pointer resolves against, so a
        // duplicate hidden behind different markup must still be two.
        assert_eq!(
            headings("## Storage\n\n## **Storage**\n")
                .iter()
                .filter(|(_, text, _)| text == "Storage")
                .count(),
            2
        );
    }

    #[test]
    fn a_delimiter_the_document_ends_inside_is_named() {
        let fence = |line| Some(Unterminated::Fence { line });
        assert_eq!(Document::of("a\n\n```\nx\n").unterminated(), fence(3));
        assert_eq!(Document::of("a\n\n```\n").unterminated(), fence(3));
        // A shorter run inside a longer fence does not close it.
        assert_eq!(Document::of("a\n\n````\nx\n```\n").unterminated(), fence(3));
        assert_eq!(Document::of("a\n\n~~~\nx\n").unterminated(), fence(3));
        assert_eq!(
            Document::of("a\n\n<!-- opened\nswallowed\n").unterminated(),
            Some(Unterminated::Comment { line: 3 })
        );

        // A closing fence carries whatever its container puts before it —
        // spaces inside a list, `>` inside a block quote — so this reads the
        // parser's extents rather than the fence's own spelling.
        assert_eq!(Document::of("> ```\n> x\n").unterminated(), fence(1));
        assert_eq!(
            Document::of("- a\n\n    ```\n    x\n").unterminated(),
            fence(3)
        );

        for closed in [
            "a\n\n```\nx\n```\n",
            "a\n\n```\nx\n```",
            "a\n\n~~~\nx\n~~~\n",
            "a\n\n```\n```\n",
            "a\n\n    indented\n",
            "a\n\n<!-- opened\nand closed\n-->\n",
            "a\n\n```\nx\n```\n\nb\n",
            "> ```\n> x\n> ```\n\nafter\n",
            "- a\n\n    ```\n    x\n    ```\n\n- b\n",
        ] {
            assert_eq!(
                Document::of(closed).unterminated(),
                None,
                "from: {closed:?}"
            );
        }
    }

    #[test]
    fn an_indent_makes_code_a_fence_would_have_quoted() {
        // Two gates read this differently on purpose, so the reader reports
        // which it is rather than choosing for them.
        let doc = Document::of("Prose.\n\n    - [Critical] indented\n\n```\n- fenced\n```\n");
        let marked: Vec<(u32, bool, String)> = doc
            .lines()
            .iter()
            .map(|line| (line.no, line.indented_code, line.text.clone()))
            .collect();
        assert!(
            marked.contains(&(3, true, "    - [Critical] indented".into())),
            "an indented line is kept whole and marked: {marked:?}"
        );
        assert!(
            !marked.iter().any(|(_, _, text)| text.contains("fenced")),
            "a fenced line is a quotation and is gone: {marked:?}"
        );
    }
}
