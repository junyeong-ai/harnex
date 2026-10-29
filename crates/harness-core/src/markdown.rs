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
//! - Never hides raw HTML that is not a comment. A `<details>` block is
//!   content a reader opens; only `<!-- -->` is an instruction.
//! - Never enables a syntax extension beyond YAML frontmatter, which every
//!   rule file opens with and no core parser reads. The rest change what a
//!   heading says — under GFM strikethrough `## ~~Old~~ New` is cited as
//!   `Old New`, and without it as written — and a citation resolves against
//!   CommonMark core, the one renderer every consumer of a harness agrees on.
//!   An extension that changes no answer here is not enabled either: it would
//!   be configuration nothing can test.
//! - Never reads a link reference definition as anything but the line it
//!   is. `[file: x.rs:1]: https://…` renders nothing and is a claim here —
//!   the marker is the token, and a token spelled as a link label is not a
//!   shape any rule writes.
//! - Never reads a leading `---` as a thematic break. With frontmatter
//!   enabled it opens a metadata block, and a document that opens with a
//!   rule instead of frontmatter is hidden to its next `---` — loudly, as a
//!   section the gate then reports missing.
//! - Never decodes a line. `prose()` is the source line with hidden spans
//!   removed, not rendered inline text, so an entity or a backslash escape
//!   reaches a gate as written. The gates match reserved tokens, and a token
//!   is reserved in source — `[file&#58; x]` renders as a marker and is not
//!   one, exactly as a marker inside a code span is one and does not render.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// One line a reader sees, with the spans a reader does not removed.
pub(crate) struct Line {
    pub(crate) no: u32,
    pub(crate) text: String,
}

/// One heading, as a reader reads it: the inline text with its markup
/// resolved, whatever spelling the source used.
pub(crate) struct Heading {
    pub(crate) text: String,
    pub(crate) line: u32,
}

/// A markdown document, read once.
pub(crate) struct Document {
    lines: Vec<Line>,
    headings: Vec<Heading>,
}

impl Document {
    pub(crate) fn of(source: &str) -> Self {
        let text = normalize(source);
        let mut quoted: Vec<Range<usize>> = Vec::new();
        let mut indented: Vec<Range<usize>> = Vec::new();
        let mut headings: Vec<Heading> = Vec::new();
        let mut open_heading: Option<Heading> = None;
        let mut in_comment = false;

        let options = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
        for (event, range) in Parser::new_ext(&text, options).into_offset_iter() {
            if !matches!(event, Event::Html(_)) {
                in_comment = false;
            }
            match event {
                Event::Start(Tag::CodeBlock(kind)) => match kind {
                    CodeBlockKind::Fenced(_) => quoted.push(range),
                    CodeBlockKind::Indented => indented.push(range),
                },
                Event::Html(_) => {
                    // Raw HTML is content a browser shows, except the comments
                    // in it, which it hides — each `<!-- -->` span is masked
                    // and the rest of the line stays, so a row or a claim
                    // written after a comment's close is still read. An open
                    // `<!--` runs to the end of the document.
                    let slice = &text[range.clone()];
                    let mut at = 0usize;
                    loop {
                        if in_comment {
                            match slice[at..].find("-->") {
                                Some(close) => {
                                    let end = at + close + 3;
                                    quoted.push(range.start + at..range.start + end);
                                    in_comment = false;
                                    at = end;
                                }
                                None => {
                                    quoted.push(range.start + at..range.end);
                                    break;
                                }
                            }
                        } else {
                            match slice[at..].find("<!--") {
                                Some(open) => {
                                    in_comment = true;
                                    at += open;
                                }
                                None => break,
                            }
                        }
                    }
                }
                Event::InlineHtml(_) => {
                    // An inline comment hides its span; `<br>` and every other
                    // inline tag is content a reader sees around it.
                    if text[range.clone()].starts_with("<!--") {
                        quoted.push(range);
                    }
                }
                Event::Start(Tag::MetadataBlock(_)) => quoted.push(range),
                Event::Start(Tag::Heading { .. }) => {
                    open_heading = Some(Heading {
                        text: String::new(),
                        line: line_of(&text, range.start),
                    });
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some(mut heading) = open_heading.take() {
                        // A renderer strips the whitespace a trailing tab or
                        // a masked comment leaves; a citation is written from
                        // what it shows.
                        heading.text = heading.text.trim().to_string();
                        headings.push(heading);
                    }
                }
                Event::Text(run) | Event::Code(run) => {
                    if let Some(heading) = open_heading.as_mut() {
                        heading.text.push_str(&run);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(heading) = open_heading.as_mut() {
                        heading.text.push(' ');
                    }
                }
                _ => {}
            }
        }

        Self {
            lines: read_lines(&text, &quoted, &indented),
            headings,
        }
    }

    /// The lines a reader reads as prose — what the document asserts.
    pub(crate) fn prose(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter()
    }

    pub(crate) fn headings(&self) -> &[Heading] {
        &self.headings
    }
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
    // `str::lines` is right once the terminators are, which is the whole
    // reason `normalize` runs first.
    u32::try_from(normalize(source).lines().count()).unwrap_or(u32::MAX)
}

/// The text of the 1-indexed `line`, terminators normalised as [`line_count`]
/// counts them, or `None` past the end.
pub(crate) fn line_at(source: &str, line: u32) -> Option<String> {
    let index = usize::try_from(line.checked_sub(1)?).ok()?;
    normalize(source).lines().nth(index).map(str::to_string)
}

/// The 1-indexed line an offset falls on.
fn line_of(text: &str, offset: usize) -> u32 {
    let count = text[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Each source line as a reader meets it as prose: quoted spans removed, and
/// a line an indent turned into code or a reader sees nothing of dropped.
fn read_lines(text: &str, quoted: &[Range<usize>], indented: &[Range<usize>]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, raw) in text.split('\n').enumerate() {
        let end = start + raw.len();
        let no = u32::try_from(idx + 1).unwrap_or(u32::MAX);

        // An indented block's range opens at its content, past the indent
        // that made it code, so the whole line is dropped rather than masked
        // down to that indent.
        if indented
            .iter()
            .any(|range| range.start < end && range.end > start)
        {
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
        out.push(Line { no, text: visible });
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(text: &str) -> Vec<(u32, String)> {
        Document::of(text)
            .prose()
            .map(|line| (line.no, line.text.clone()))
            .collect()
    }

    fn headings(text: &str) -> Vec<(String, u32)> {
        Document::of(text)
            .headings()
            .iter()
            .map(|h| (h.text.clone(), h.line))
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
    fn an_inline_tag_is_content_and_an_inline_comment_is_not() {
        assert_eq!(
            visible("Text with <br> and CLAIM").first(),
            Some(&(1, "Text with <br> and CLAIM".into()))
        );
        assert_eq!(
            visible("Text <!-- hidden --> and CLAIM").first(),
            Some(&(1, "Text  and CLAIM".into()))
        );
    }

    #[test]
    fn raw_html_that_is_not_a_comment_is_content_a_reader_opens() {
        // A `<details>` block renders; masking it swallowed a live claim and
        // a finding-shaped row with no finding from either gate.
        let seen: Vec<String> =
            visible("<details>\n<summary>Owners</summary>\nCLAIM\n</details>\n\nAfter.\n")
                .into_iter()
                .map(|(_, t)| t)
                .collect();
        assert!(seen.iter().any(|line| line.contains("CLAIM")), "{seen:?}");
        assert!(seen.iter().any(|line| line.contains("After")), "{seen:?}");
    }

    #[test]
    fn a_fence_its_container_closed_hides_nothing_past_it() {
        // The list item ends the block, and everything past it is read.
        let text = "- note:\n\n  ```\n  code\n\nBack at top level.\n";
        let seen: Vec<String> = visible(text).into_iter().map(|(_, t)| t).collect();
        assert!(
            seen.iter().any(|line| line.contains("Back at top level")),
            "{seen:?}"
        );
    }

    #[test]
    fn a_heading_spanning_source_lines_reads_with_the_break_as_a_space() {
        assert_eq!(
            headings("Two\nLines\n=====\n"),
            vec![("Two Lines".into(), 1)]
        );
    }

    #[test]
    fn a_setext_heading_keeps_a_leading_hash_as_text() {
        // `#123 was closed upstream` over `---` is an h2 whose range opens
        // with `#`, and the `#` is part of what a reader reads.
        assert_eq!(
            headings("#123 was closed upstream\n---\n"),
            vec![("#123 was closed upstream".into(), 1)]
        );
    }

    #[test]
    fn a_heading_reads_without_the_whitespace_a_renderer_strips() {
        for (source, want) in [
            ("## Storage\t\n", "Storage"),
            (
                "## Outstanding issues <!-- one row per finding -->\n",
                "Outstanding issues",
            ),
            ("## <!-- lead --> Storage\n", "Storage"),
            ("##   Storage   \n", "Storage"),
        ] {
            assert_eq!(
                Document::of(source).headings()[0].text,
                want,
                "from: {source:?}"
            );
        }
    }

    #[test]
    fn text_after_a_comment_closes_on_its_line_is_still_read() {
        // A block comment's closing line is one raw-HTML event; masking the
        // whole event hid the row or claim written after `-->`.
        let seen =
            |text: &str| -> Vec<String> { visible(text).into_iter().map(|(_, t)| t).collect() };
        assert_eq!(seen("<!-- c --> Owner: CLAIM"), vec![" Owner: CLAIM"]);
        assert_eq!(seen("<!-- a --> x <!-- b --> y"), vec![" x  y"]);
        assert_eq!(
            seen("<!--\nmulti\n--> Owner: CLAIM\nnext"),
            vec![" Owner: CLAIM".to_string(), "next".to_string()]
        );
        // A comment inside another raw-HTML block is hidden wherever it sits.
        let seen = seen("<div>\nx <!-- hidden --> y\n</div>");
        assert!(seen.iter().any(|l| l == "x  y"), "{seen:?}");
        assert!(!seen.iter().any(|l| l.contains("hidden")), "{seen:?}");
    }

    #[test]
    fn a_file_is_as_many_lines_as_an_editor_shows() {
        for (source, want) in [
            ("", 0),
            ("\n", 1),
            ("a", 1),
            ("a\n", 1),
            ("a\nb", 2),
            ("a\nb\n", 2),
            ("a\rb\r", 2),
            ("\u{feff}a\n", 1),
        ] {
            assert_eq!(line_count(source), want, "from: {source:?}");
        }
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
        assert_eq!(headings("## Storage\n"), vec![("Storage".into(), 1)]);
        assert_eq!(headings("Storage\n=======\n"), vec![("Storage".into(), 1)]);
        assert_eq!(headings("## **Storage**\n"), vec![("Storage".into(), 1)]);
        assert_eq!(headings("## Storage ##\n"), vec![("Storage".into(), 1)]);
        assert_eq!(
            headings("## The `write_atomic` contract\n"),
            vec![("The write_atomic contract".into(), 1)]
        );
        assert_eq!(headings("```\n## Decoy\n```\n"), vec![]);
        assert_eq!(headings("<!--\n## Decoy\n-->\n"), vec![]);
        assert_eq!(
            headings("# One\n\n## Two\n"),
            vec![("One".into(), 1), ("Two".into(), 3)]
        );
    }

    #[test]
    fn two_spellings_of_one_heading_are_two_headings() {
        // The count is what a section pointer resolves against, so a
        // duplicate hidden behind different markup must still be two.
        assert_eq!(
            headings("## Storage\n\n## **Storage**\n")
                .iter()
                .filter(|(text, _)| text == "Storage")
                .count(),
            2
        );
    }

    #[test]
    fn an_indent_makes_code_as_a_fence_does() {
        let seen: Vec<String> = visible("Prose.\n\n    indented\n\n```\nfenced\n```\n")
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert!(seen.iter().any(|line| line == "Prose."), "{seen:?}");
        assert!(
            !seen
                .iter()
                .any(|line| line.contains("indented") || line.contains("fenced")),
            "{seen:?}"
        );
    }
}
