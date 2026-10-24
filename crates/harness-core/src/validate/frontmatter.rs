//! YAML frontmatter extraction, and how the runtime reads a value from it.
//! Frontmatter is the optional block between the first two `---` lines at the
//! top of a markdown file.

use std::path::Path;

use yaml_serde::Value;

use crate::error::{Error, Result};

pub struct Frontmatter {
    pub begin_line: u32,
    pub end_line: u32,
    pub yaml_text: String,
    pub body_start_line: u32,
}

/// Parse frontmatter if present. Returns Ok(None) when the file has no
/// frontmatter (no leading `---`). Returns Err on unterminated frontmatter.
pub fn parse(content: &str, source: &Path) -> Result<Option<Frontmatter>> {
    let mut lines = content.lines();
    let first = lines.next();
    if first != Some("---") {
        return Ok(None);
    }
    let mut yaml_lines: Vec<&str> = Vec::new();
    let mut end_line: Option<u32> = None;
    for (idx, line) in lines.enumerate() {
        if line == "---" {
            end_line = Some(idx as u32 + 2);
            break;
        }
        yaml_lines.push(line);
    }
    let end_line = end_line.ok_or_else(|| Error::ValidateFrontmatterMalformed {
        path: source.to_path_buf(),
        message: "frontmatter `---` is not terminated".into(),
    })?;
    Ok(Some(Frontmatter {
        begin_line: 1,
        end_line,
        yaml_text: yaml_lines.join("\n"),
        body_start_line: end_line + 1,
    }))
}

/// `String(value)` as the runtime spells a frontmatter value, `None` for null.
pub fn js_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
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

    #[test]
    fn parses_well_formed() {
        let content = "---\nkey: value\n---\n# Body\n";
        let fm = parse(content, &PathBuf::from("x.md")).unwrap().unwrap();
        assert_eq!(fm.begin_line, 1);
        assert_eq!(fm.end_line, 3);
        assert_eq!(fm.yaml_text, "key: value");
        assert_eq!(fm.body_start_line, 4);
    }

    #[test]
    fn rejects_unterminated() {
        let content = "---\nkey: value\nno-end";
        assert!(parse(content, &PathBuf::from("x.md")).is_err());
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
