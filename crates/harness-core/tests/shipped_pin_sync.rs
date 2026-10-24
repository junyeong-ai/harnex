//! Drift guard over the `[meta] harnex_version` pins this repository ships —
//! the plugin's `harness.toml` template, the examples, and the README's copy.
//!
//! A shipped pin starts at the release that ships it and stops at the next
//! breaking one, as Cargo reads `0.y`. Additions ship in patches, so what a
//! harness written from this release uses may be a patch's addition: a lower
//! floor admits a binary that refuses it as an unknown key rather than as the
//! version it is. The floor is written in full because it is also a tag: the
//! skill installs `v<floor>`, and no release is tagged `v0.36`.
//! Constitution IX.

mod common;

use std::path::PathBuf;

use pulldown_cmark::{Event, Parser, TagEnd};
use semver::{BuildMetadata, Comparator, Op, Version, VersionReq};

const TEMPLATE: &str = "plugins/harnex/templates/common/harness.toml";
const README: &str = "README.md";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn repo_path(rel: &str) -> PathBuf {
    repo_root().join(rel)
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn this_release() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("own version parses")
}

/// The first release a break may ship in.
fn next_break() -> Version {
    let v = this_release();
    if v.major == 0 {
        Version::new(0, v.minor + 1, 0)
    } else {
        Version::new(v.major + 1, 0, 0)
    }
}

/// The version a comparator names, pre-release included: `>=0.36.0-rc.1`
/// admits the release candidate below `0.36.0`.
fn bound(c: &Comparator) -> Version {
    Version {
        major: c.major,
        minor: c.minor.unwrap_or(0),
        patch: c.patch.unwrap_or(0),
        pre: c.pre.clone(),
        build: BuildMetadata::EMPTY,
    }
}

/// Read as `Config::load_from` reads it, surrounding whitespace trimmed.
fn assert_starts_at_this_release(pin: &str, whose: &str) {
    let req = VersionReq::parse(pin.trim()).unwrap_or_else(|e| panic!("{whose}: '{pin}': {e}"));
    let floor = req.comparators.iter().find(|c| c.op == Op::GreaterEq);
    let ceiling = req.comparators.iter().find(|c| c.op == Op::Less);
    let (Some(floor), Some(ceiling), 2) = (floor, ceiling, req.comparators.len()) else {
        panic!("{whose}: '{pin}' is not the `>=floor, <ceiling` form harnex ships");
    };
    assert!(
        floor.minor.is_some() && floor.patch.is_some(),
        "{whose} pins '{pin}': the floor names a release tag, so it carries all three parts"
    );
    assert_eq!(
        (bound(floor), bound(ceiling)),
        (this_release(), next_break()),
        "{whose} pins '{pin}': a shipped pin starts at this release and stops at the next \
         breaking one"
    );
}

fn toml_pin(path: &PathBuf) -> String {
    let table: toml::Table =
        toml::from_str(&read(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    table["meta"]["harnex_version"]
        .as_str()
        .unwrap_or_else(|| panic!("{} declares no [meta] harnex_version", path.display()))
        .to_string()
}

#[test]
fn every_shipped_config_pins_from_this_release() {
    let mut shipped = common::tracked(&repo_root(), "examples");
    shipped.push(repo_path(TEMPLATE));
    for path in shipped {
        assert_starts_at_this_release(&toml_pin(&path), &path.display().to_string());
    }
}

/// The text a reader of the README sees, one string per block: a paragraph,
/// heading or list item as CommonMark renders it — a line break one space, a
/// quote marker gone, a code span's text in place — and a fenced block as
/// written. Raw HTML is passed through unrendered, so it is not read.
fn readme_text(readme: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut text = String::new();
    for event in Parser::new(readme) {
        match event {
            Event::Text(part) | Event::Code(part) => text.push_str(&part),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) => {
                blocks.push(std::mem::take(&mut text));
            }
            _ => {}
        }
    }
    blocks.push(text);
    blocks
}

/// Every value written as `<key> = "…"` in that text. Each occurrence of the
/// key is read where it stands, so a value naming the key is not split there,
/// and a quote left open is reported as the typo it is.
fn assigned(blocks: &[String], key: &str) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|text| {
            text.match_indices(key).filter_map(move |(at, _)| {
                let value = text[at + key.len()..].trim_start().strip_prefix('=')?;
                let value = value.trim_start().strip_prefix('"')?;
                let (value, _) = value.split_once('"').unwrap_or_else(|| {
                    panic!("{README} leaves the quote after {key} open: `{text}`")
                });
                Some(value.to_string())
            })
        })
        .collect()
}

#[test]
fn the_readme_pins_from_this_release() {
    let text = readme_text(&read(&repo_path(README)));
    let pins = assigned(&text, "harnex_version");
    assert!(!pins.is_empty(), "{README} shows no harnex_version pin");
    for pin in &pins {
        assert_starts_at_this_release(pin, README);
    }
}
