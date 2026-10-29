//! YAML frontmatter extraction, and how the runtime reads a value from it.
//! Frontmatter is the optional block a markdown file opens with. A file Claude
//! Code loads is read the way it reads it ([`parse`]); a document whose
//! grammar harnex owns is read by YAML's own fences ([`parse_owned`]).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use yaml_serde::Value;

use crate::error::{Error, Result};

/// What a JavaScript `\s` matches. Rust's own `\s` differs at U+0085 and
/// U+FEFF.
const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

/// The runtime's frontmatter: `---` and whitespace through a line break open
/// it, and the next `---` closes it wherever that falls — inside a value too,
/// which the runtime then reads as cut there — with the whitespace after it.
static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"\A---{JS_SPACE}*\n((?s:.*?))---{JS_SPACE}*\n?"))
        .expect("frontmatter block regex")
});

/// The opening of a block, closed or not.
static OPENING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"\A---{JS_SPACE}*\n")).expect("frontmatter opening regex")
});

/// A file's text split as the runtime splits it, past a leading byte-order
/// mark: the frontmatter's YAML and the body after it, or no frontmatter and
/// the whole text as body.
pub fn split(content: &str) -> (Option<&str>, &str) {
    let text = content.strip_prefix('\u{feff}').unwrap_or(content);
    match BLOCK.captures(text) {
        None => (None, text),
        Some(block) => {
            let yaml = block.get(1).expect("group 1 is the block").as_str();
            let end = block.get(0).expect("group 0 is the match").end();
            (Some(yaml), &text[end..])
        }
    }
}

pub struct Frontmatter {
    pub begin_line: u32,
    pub yaml_text: String,
}

/// The frontmatter a file opens with, as [`split`] finds it. `Ok(None)` when
/// the file opens none. `Err` when it opens one and never closes it, which the
/// runtime reads as a file with no frontmatter at all.
pub fn parse(content: &str, source: &Path) -> Result<Option<Frontmatter>> {
    match split(content) {
        (Some(yaml), _) => Ok(Some(Frontmatter {
            begin_line: 1,
            yaml_text: yaml.to_string(),
        })),
        (None, text) if OPENING.is_match(text) => Err(Error::ValidateFrontmatterMalformed {
            path: source.to_path_buf(),
            message: "frontmatter `---` is not terminated".into(),
        }),
        (None, _) => Ok(None),
    }
}

/// The frontmatter of a document whose grammar harnex owns: the lines between
/// a first line `---` and the next line `---`, so a `---` inside a block
/// scalar stays the scalar's text. `Ok(None)` and `Err` as for [`parse`].
pub fn parse_owned(content: &str, source: &Path) -> Result<Option<Frontmatter>> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Ok(None);
    }
    let mut yaml = Vec::new();
    for line in lines {
        if line == "---" {
            return Ok(Some(Frontmatter {
                begin_line: 1,
                yaml_text: yaml.join("\n"),
            }));
        }
        yaml.push(line);
    }
    Err(Error::ValidateFrontmatterMalformed {
        path: source.to_path_buf(),
        message: "frontmatter `---` is not terminated".into(),
    })
}

/// `String(n)` for a JavaScript number (ECMA-262 `Number::toString`). The
/// runtime's YAML reader yields every number as one, so `1.0` reads as `"1"`
/// and an integer past 2^53 as the double nearest it.
fn js_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n < 0.0 {
        return format!("-{}", js_number(-n));
    }
    // `{:e}` spells the shortest digits that round-trip, as the standard
    // requires, with the exponent of the first: `d[.ddd]e<exp>`.
    let scientific = format!("{n:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` writes an exponent");
    let mut digits = mantissa.replace('.', "");
    if let Some(even) = even_of_a_tie(n, &digits, exponent) {
        digits = even;
    }
    let k = digits.len() as i64;
    let point = exponent.parse::<i64>().expect("`{:e}` writes an integer") + 1;
    if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        let (whole, fraction) = digits.split_at(point as usize);
        format!("{whole}.{fraction}")
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let (first, rest) = digits.split_at(1);
        let sign = if point > 0 { '+' } else { '-' };
        let mantissa = match rest {
            "" => first.to_string(),
            _ => format!("{first}.{rest}"),
        };
        format!("{mantissa}e{sign}{}", (point - 1).abs())
    }
}

/// The even spelling where two shortest ones lie equally close to `n`: the
/// standard takes it, and Rust's formatter takes the upper. They part only at
/// an exact tie — `n`'s exact expansion is the lower spelling followed by a
/// single 5 — and only where the lower spelling also reads back as `n`.
fn even_of_a_tie(n: f64, digits: &str, exponent: &str) -> Option<String> {
    let (head, last) = digits.split_at(digits.len() - 1);
    let last = last.parse::<u8>().ok()?;
    if last % 2 == 0 {
        return None;
    }
    let lower = format!("{head}{}", last - 1);
    // 767 fractional digits hold the exact expansion of every finite double.
    let exact = format!("{n:.767e}");
    let (exact_mantissa, exact_exponent) = exact.split_once('e')?;
    let tail = exact_mantissa
        .replace('.', "")
        .strip_prefix(lower.as_str())?
        .to_string();
    let tie =
        exact_exponent == exponent && tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0');
    let (first, rest) = lower.split_at(1);
    let reads_back = format!("{first}.{rest}0e{exponent}").parse::<f64>().ok() == Some(n);
    (tie && reads_back).then_some(lower)
}

/// `String(value)` as the runtime spells a frontmatter value, `None` for null.
pub fn js_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => n.as_f64().map(js_number),
        Value::String(s) => Some(s.clone()),
        Value::Sequence(items) => Some(
            items
                .iter()
                .map(|item| js_string(item).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Mapping(_) => Some("[object Object]".to_string()),
        Value::Tagged(tagged) => js_string(&tagged.value),
    }
}

/// The spellings [`flag`] reads, as a finding names them.
pub const FLAG_SPELLINGS: &str = "true/false, yes/no, on/off or 1/0";

/// A flag as the runtime reads it: `true`, `1`, `yes` and `on` set it, and
/// `false`, `0`, `no` and `off` clear it, spelled as a boolean, a number or
/// text in any case. `None` is a value it reads as neither, which leaves the
/// flag unset.
pub fn flag(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(set) => Some(*set),
        spelled @ (Value::String(_) | Value::Number(_)) => {
            match js_string(spelled)?.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn no_frontmatter_returns_none() {
        let content = "# Just markdown\n\nSome content.";
        assert!(parse(content, &PathBuf::from("x.md")).unwrap().is_none());
    }

    fn block(content: &str) -> Option<String> {
        parse(content, &PathBuf::from("x.md"))
            .unwrap()
            .map(|fm| fm.yaml_text)
    }

    #[test]
    fn parses_well_formed() {
        assert_eq!(
            block("---\nkey: value\n---\n# Body\n").as_deref(),
            Some("key: value\n")
        );
    }

    #[test]
    fn reads_the_fences_the_runtime_reads() {
        for content in [
            "--- \nkey: value\n---  \n# Body\n",
            "\u{feff}---\r\nkey: value\r\n---\r\n# Body\r\n",
            "---\t\n\nkey: value\n---",
            "---\u{feff}\nkey: value\n---\n",
        ] {
            assert_eq!(
                block(content).as_deref().map(str::trim),
                Some("key: value"),
                "{content:?}"
            );
        }
        for content in [
            "----\nkey: value\n---\n",
            "--- x\nkey: value\n---\n",
            "---",
            "---\u{85}\nkey: value\n---\n",
        ] {
            assert_eq!(block(content), None, "{content:?}");
        }
    }

    #[test]
    fn closes_at_the_first_fence_even_inside_a_value() {
        assert_eq!(
            block("---\na: x --- y\nb: 1\n---\n").as_deref(),
            Some("a: x ")
        );
    }

    #[test]
    fn rejects_unterminated() {
        let content = "---\nkey: value\nno-end";
        assert!(parse(content, &PathBuf::from("x.md")).is_err());
    }

    #[test]
    fn spells_a_number_as_javascript_does() {
        for (n, spelled) in [
            (1.0, "1"),
            (-0.0, "0"),
            (0.5, "0.5"),
            (-1.5, "-1.5"),
            (123.0, "123"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (9007199254740993.0, "9007199254740992"),
            (1.000_007_629_394_531_2, "1.0000076293945312"),
            (f64::INFINITY, "Infinity"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(js_number(n), spelled, "{n:?}");
        }
    }

    #[test]
    fn reads_a_number_flag_by_its_value() {
        for (text, expected) in [("1.0", Some(true)), ("0.0", Some(false)), ("2", None)] {
            let value: Value = yaml_serde::from_str(text).unwrap();
            assert_eq!(flag(&value), expected, "{text}");
        }
    }

    #[test]
    fn every_spelling_a_finding_names_is_one_the_flag_reads() {
        for pair in FLAG_SPELLINGS.split([',', ' ']).filter(|w| w.contains('/')) {
            let (set, unset) = pair.split_once('/').expect("set/unset");
            for (word, expected) in [(set, true), (unset, false)] {
                for spelled in [word.to_string(), word.to_ascii_uppercase()] {
                    assert_eq!(
                        flag(&Value::String(spelled.clone())),
                        Some(expected),
                        "{spelled}"
                    );
                }
            }
        }
    }
}
