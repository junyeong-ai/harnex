//! What the reader shows, against what a renderer shows.
//!
//! The gate's whole premise is that a claim is live where a reader sees it, so
//! the reader's answer has to be a renderer's answer. This table is that
//! comparison held in place: each row is one markdown construct carrying one
//! marker, and `visible` is what an independent CommonMark implementation
//! renders — derived by running every row through `pandoc --from=commonmark`
//! and asking whether the marker survives outside a code block or a comment.
//!
//! The reader this replaced disagreed with pandoc in every group below.
//! Keeping the table means the next change to the reader is checked against
//! CommonMark rather than against whoever writes it.
//!
//! Every row runs in all three line endings, because a document written with
//! carriage returns is the same document to a renderer and was one line to
//! the reader that preceded this one.

use harness_core::evidence::{ClaimKind, parse_claims};

const MARKER: &str = "[file: no/such/target.rs:1]";
const FRONTMATTER: &str = "---\npaths: [\"src/**\"]\n---\n\n# Rule\n\n";

/// `true` where a renderer shows the marker as prose.
const CASES: &[(&str, &str, bool)] = &[
    // Prose, wherever a container puts it.
    ("bare prose", "Owner: {M}.", true),
    ("nested bullet", "- outer:\n    - inner: {M}", true),
    ("list continuation", "- item\n\n  {M}", true),
    ("deep list continuation", "- a\n  - b\n\n    {M}", true),
    ("table cell", "| a | b |\n|---|---|\n| {M} | y |", true),
    ("block quote", "> Quoted: {M}", true),
    ("emphasis", "*Owner: {M}*", true),
    ("after a thematic break", "---\n\n{M}", true),
    ("after a setext heading", "Note\n---\n\n{M}", true),
    (
        "after a link reference",
        "[ref]: https://example.com\n\n{M}",
        true,
    ),
    ("after an entity", "&amp; then {M}", true),
    ("after a hard break", "line  \n{M}", true),
    // A code block is a sample, however it was opened and wherever it sits.
    ("top-level fence", "```\n{M}\n```", false),
    ("fence with info string", "```markdown\n{M}\n```", false),
    ("tilde fence", "~~~\n{M}\n~~~", false),
    ("four-backtick fence", "````\n```\n{M}\n```\n````", false),
    (
        "fence in a list item",
        "- like this:\n\n    ```\n    {M}\n    ```",
        false,
    ),
    (
        "fence in a nested list",
        "- a\n  - b:\n\n        ```\n        {M}\n        ```",
        false,
    ),
    ("fence in a block quote", "> ```\n> {M}\n> ```", false),
    ("fence closed by the document", "```\n{M}", false),
    (
        "fence closed by its container",
        "- a:\n\n  ```\n  {M}\n\nBack out.",
        false,
    ),
    ("indented code", "Prose.\n\n    {M}", false),
    ("tab-indented code", "Prose.\n\n\t{M}", false),
    // A comment is an instruction; other raw HTML is content a reader opens.
    ("html comment, one line", "<!-- {M} -->", false),
    ("html comment, multi line", "<!--\n{M}\n-->", false),
    ("html comment, unterminated", "<!--\n{M}", false),
    (
        "inline comment mid-line",
        "Before <!-- {M} --> after.",
        false,
    ),
    (
        "incomplete inline comment",
        "Before <!-- unterminated\n\n{M}",
        true,
    ),
    ("quoted comment marker", "Write `<!--` then {M}.", true),
    ("after a comment closes on its line", "<!-- c --> {M}", true),
    (
        "after a multi-line comment closes",
        "<!--\nc\n--> {M}",
        true,
    ),
    (
        "between two comments on one line",
        "<!-- a --> {M} <!-- b -->",
        true,
    ),
    // Tight, so the marker is inside the HTML block rather than a paragraph
    // after it — a renderer hands the block to the browser, which shows it.
    (
        "details block",
        "<details>\n<summary>S</summary>\n{M}\n</details>",
        true,
    ),
    ("raw html div", "<div>\n{M}\n</div>", true),
    (
        "details block, loose",
        "<details>\n<summary>S</summary>\n\n{M}\n\n</details>",
        true,
    ),
];

/// The one row where this reader answers differently from a renderer, on
/// purpose. `.claude/rules/audit.md` owns the reason: the marker is a reserved
/// token, so writing one literally where a scan reaches it is itself the
/// finding, and an example goes in a fence, in a comment, or paraphrased.
/// Exempting a code span would mean an author who habitually backticks their
/// markers loses the check and is told nothing.
const CODE_SPAN: (&str, &str, bool) = ("inline code span", "Quoted `{M}` inline.", true);

fn marker_is_live(body: &str, ending: &str) -> bool {
    let document =
        (FRONTMATTER.to_string() + &body.replace("{M}", MARKER) + "\n").replace('\n', ending);
    parse_claims(&document)
        .iter()
        .any(|claim| match &claim.kind {
            ClaimKind::File { path, .. } => path == "no/such/target.rs",
            _ => false,
        })
}

#[test]
fn the_reader_shows_what_a_renderer_shows() {
    let mut checked = 0;
    for (label, body, visible) in CASES {
        for ending in ["\n", "\r\n", "\r"] {
            checked += 1;
            assert_eq!(
                marker_is_live(body, ending),
                *visible,
                "`{label}` with {ending:?} line endings: a renderer shows the marker \
                 as prose = {visible}"
            );
        }
    }
    assert_eq!(checked, CASES.len() * 3);
}

#[test]
fn the_one_divergence_from_a_renderer_is_the_one_that_is_written_down() {
    let (label, body, _) = CODE_SPAN;
    for ending in ["\n", "\r\n", "\r"] {
        assert!(
            marker_is_live(body, ending),
            "`{label}` with {ending:?}: a marker inside a code span stays a claim, \
             because the marker is a reserved token and an example belongs in a \
             fence, a comment, or a paraphrase"
        );
    }
}
