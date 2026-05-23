//! # codegen — cross-file sentinel-block sync
//!
//! Synchronizes TOML-string-array values from a source-of-truth into N
//! target files between BEGIN/END sentinel comments. Used to keep enum
//! vocabularies coherent across `.claude/rules/`, `nodex.toml`, shell
//! hook scripts, etc., without hand-syncing each mirror site.
//!
//! ## What this module refuses to do
//!
//! - Never sync any value that is not a TOML array of strings (closed shape).
//! - Never modify content outside the sentinel block.
//! - Never write if rendered content equals current content (no churn).
//! - Never follow cyclic references — `Config::validate` rejects target
//!   files that are also sources of any group.

pub mod renderer;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{CodegenConfig, CodegenGroupDecl, SentinelTargetDecl};
use crate::error::{Error, Result};
use crate::path_guard;

pub use renderer::{Renderer, RendererStrategy, renderer_for};

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SyncOutcome {
    pub group: String,
    pub target: PathBuf,
    pub changed: bool,
    pub rendered_bytes: usize,
}

pub struct SentinelSyncer<'a> {
    config: &'a CodegenConfig,
    working_dir: &'a Path,
}

impl<'a> SentinelSyncer<'a> {
    pub fn new(config: &'a CodegenConfig, working_dir: &'a Path) -> Self {
        Self { config, working_dir }
    }

    pub fn sync(&self) -> Result<Vec<SyncOutcome>> {
        self.run(true)
    }

    pub fn check(&self) -> Result<Vec<SyncOutcome>> {
        self.run(false)
    }

    fn run(&self, apply: bool) -> Result<Vec<SyncOutcome>> {
        let mut outcomes = Vec::new();
        let mut source_cache: HashMap<PathBuf, toml::Value> = HashMap::new();
        for group in &self.config.groups {
            let source_path = self.working_dir.join(&group.source);
            let source = match source_cache.get(&source_path) {
                Some(v) => v.clone(),
                None => {
                    let v = load_source(&source_path)?;
                    source_cache.insert(source_path.clone(), v.clone());
                    v
                }
            };
            let values = extract_string_array(&source, &group.source_key, &source_path)?;
            for target in &group.targets {
                outcomes.push(self.apply_one(group, target, &values, apply)?);
            }
        }
        Ok(outcomes)
    }

    fn apply_one(
        &self,
        group: &CodegenGroupDecl,
        target: &SentinelTargetDecl,
        values: &[String],
        apply: bool,
    ) -> Result<SyncOutcome> {
        let renderer = renderer_for(&target.format).ok_or_else(|| Error::CodegenRendererUnknown {
            name: target.format.clone(),
        })?;
        let rendered = renderer.render(target.name.as_deref(), values);

        let target_path = self.working_dir.join(&target.path);
        let original = std::fs::read_to_string(&target_path).map_err(|e| Error::IoFailure {
            path: target_path.clone(),
            source: e,
        })?;
        let new_content = replace_block(&original, &target.begin, &target.end, &rendered)
            .ok_or_else(|| Error::CodegenSentinelMissing {
                begin: target.begin.clone(),
                end: target.end.clone(),
                path: target_path.clone(),
            })?;
        let changed = new_content != original;
        if apply && changed {
            path_guard::write_atomic(&target_path, new_content.as_bytes())?;
        }
        Ok(SyncOutcome {
            group: group.name.clone(),
            target: target_path,
            changed,
            rendered_bytes: rendered.len(),
        })
    }
}

fn load_source(path: &Path) -> Result<toml::Value> {
    let raw = std::fs::read_to_string(path).map_err(|_| Error::CodegenSourceMissing {
        path: path.to_path_buf(),
    })?;
    toml::from_str(&raw).map_err(|e| Error::ConfigInvalid {
        message: format!("codegen source TOML parse: {e}"),
        location: None,
    })
}

fn extract_string_array(value: &toml::Value, dot_path: &str, source: &Path) -> Result<Vec<String>> {
    let mut current = value;
    for segment in dot_path.split('.') {
        current = current
            .get(segment)
            .ok_or_else(|| Error::CodegenSourceKeyMissing {
                key: dot_path.to_string(),
                path: source.to_path_buf(),
            })?;
    }
    let arr = current
        .as_array()
        .ok_or_else(|| Error::CodegenSourceShapeInvalid {
            key: dot_path.to_string(),
            path: source.to_path_buf(),
        })?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item
            .as_str()
            .ok_or_else(|| Error::CodegenSourceShapeInvalid {
                key: dot_path.to_string(),
                path: source.to_path_buf(),
            })?;
        out.push(s.to_string());
    }
    Ok(out)
}

fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") { "\r\n" } else { "\n" }
}

fn replace_block(content: &str, begin: &str, end: &str, new_inner: &str) -> Option<String> {
    let eol = detect_line_ending(content);
    let lines: Vec<&str> = content.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == begin.trim())?;
    let end_idx = (begin_idx + 1..lines.len()).find(|i| lines[*i].trim() == end.trim())?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 4);
    out.extend(lines[..=begin_idx].iter().copied());
    for line in new_inner.lines() {
        out.push(line);
    }
    out.extend(lines[end_idx..].iter().copied());
    let mut joined = out.join(eol);
    if content.ends_with('\n') || content.ends_with("\r\n") {
        joined.push_str(eol);
    }
    Some(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_block_preserves_lf() {
        let content = "before\n// BEGIN:x\nold\n// END:x\nafter\n";
        let result = replace_block(content, "// BEGIN:x", "// END:x", "new").unwrap();
        assert!(result.contains("new"));
        assert!(!result.contains("\r\n"), "should not introduce CRLF");
    }

    #[test]
    fn replace_block_preserves_crlf() {
        let content = "before\r\n// BEGIN:x\r\nold\r\n// END:x\r\nafter\r\n";
        let result = replace_block(content, "// BEGIN:x", "// END:x", "new").unwrap();
        assert!(result.contains("\r\n"), "should preserve CRLF");
        assert!(!result.contains("\r\n\n"), "should not double-newline");
        assert!(result.contains("new"));
    }
}
