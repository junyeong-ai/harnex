//! YAML frontmatter extraction. Frontmatter is the optional block between
//! the first two `---` lines at the top of a markdown file.

use std::path::Path;

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
}
