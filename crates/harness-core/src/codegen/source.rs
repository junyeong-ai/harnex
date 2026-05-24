//! Source-of-truth loading for sentinel-block sync.
//!
//! Codegen sources hold a string array at a dot-path. The closed set of
//! accepted serialization formats is the [`SourceFormat`] enum — its
//! variants are the single source of truth. Both config validation
//! (`Config::validate_codegen`) and the loader ([`load_source`]) consume
//! the enum, so a new variant forces both sites to update at compile time.
//! Every format parses into one common `serde_json::Value` shape so the
//! extractor ([`extract_string_array`]) is format-agnostic.
//!
//! ## What this module refuses to do
//!
//! - Never infer a format from the file extension — the group declares it.
//! - Never accept a value that is not a string array at the dot-path
//!   (closed shape, enforced by [`extract_string_array`]).
//! - Never silently default an unknown format string — `from_str` returns
//!   `None` and the caller turns that into a typed error.

use std::path::Path;

use crate::error::{Error, Result};

/// Closed set of supported codegen source serialization formats. Adding a
/// variant requires updating [`from_str`], [`as_str`], [`ALL`], and the
/// match in [`load_source`] — all of which the compiler enforces via
/// exhaustive `match` on `Self`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Toml,
    Json,
    Yaml,
}

impl SourceFormat {
    pub const ALL: &'static [Self] = &[Self::Toml, Self::Json, Self::Yaml];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "toml" => Self::Toml,
            "json" => Self::Json,
            "yaml" => Self::Yaml,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }
}

/// Read and parse a codegen source file into a common `serde_json::Value`
/// shape. The `format` selects the parser; all formats converge on the same
/// value model so downstream extraction is format-agnostic.
pub fn load_source(path: &Path, format: SourceFormat) -> Result<serde_json::Value> {
    let raw = std::fs::read_to_string(path).map_err(|_| Error::CodegenSourceMissing {
        path: path.to_path_buf(),
    })?;
    match format {
        SourceFormat::Toml => {
            let value: toml::Value = toml::from_str(&raw).map_err(|e| Error::ConfigInvalid {
                message: format!("codegen source parse (toml): {e}"),
                location: None,
            })?;
            serde_json::to_value(value).map_err(|e| Error::ConfigInvalid {
                message: format!("codegen source parse (toml): {e}"),
                location: None,
            })
        }
        SourceFormat::Json => {
            serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| Error::ConfigInvalid {
                message: format!("codegen source parse (json): {e}"),
                location: None,
            })
        }
        SourceFormat::Yaml => {
            serde_yml::from_str::<serde_json::Value>(&raw).map_err(|e| Error::ConfigInvalid {
                message: format!("codegen source parse (yaml): {e}"),
                location: None,
            })
        }
    }
}

/// Walk a dot-path into `value` and return the string array found there.
/// Rejects a missing path segment ([`Error::CodegenSourceKeyMissing`]) and
/// any target that is not an array of strings
/// ([`Error::CodegenSourceShapeInvalid`]).
pub fn extract_string_array(
    value: &serde_json::Value,
    dot_path: &str,
    source: &Path,
) -> Result<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn format_from_str_round_trips_every_variant() {
        for f in SourceFormat::ALL {
            assert_eq!(SourceFormat::from_str(f.as_str()), Some(*f));
        }
    }

    #[test]
    fn format_from_str_rejects_unknown() {
        assert_eq!(SourceFormat::from_str("xml"), None);
    }

    fn write(tmp: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = tmp.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_source_toml_extracts_string_array() {
        let tmp = TempDir::new().unwrap();
        let path = write(
            &tmp,
            "src.toml",
            r#"
[x]
items = ["a", "b"]
"#,
        );
        let value = load_source(&path, SourceFormat::Toml).unwrap();
        let out = extract_string_array(&value, "x.items", &path).unwrap();
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn load_source_json_extracts_string_array() {
        let tmp = TempDir::new().unwrap();
        let path = write(&tmp, "src.json", r#"{ "x": { "items": ["a", "b"] } }"#);
        let value = load_source(&path, SourceFormat::Json).unwrap();
        let out = extract_string_array(&value, "x.items", &path).unwrap();
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn load_source_yaml_extracts_string_array() {
        let tmp = TempDir::new().unwrap();
        let path = write(&tmp, "src.yaml", "x:\n  items:\n    - a\n    - b\n");
        let value = load_source(&path, SourceFormat::Yaml).unwrap();
        let out = extract_string_array(&value, "x.items", &path).unwrap();
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn extract_rejects_non_array() {
        let value = serde_json::json!({ "x": { "items": "not-an-array" } });
        let err = extract_string_array(&value, "x.items", Path::new("src.json")).unwrap_err();
        assert_eq!(
            err.code(),
            crate::error::ErrorCode::CodegenSourceShapeInvalid
        );
    }

    #[test]
    fn extract_rejects_non_string_element() {
        let value = serde_json::json!({ "x": { "items": ["a", 2] } });
        let err = extract_string_array(&value, "x.items", Path::new("src.json")).unwrap_err();
        assert_eq!(
            err.code(),
            crate::error::ErrorCode::CodegenSourceShapeInvalid
        );
    }

    #[test]
    fn extract_rejects_missing_segment() {
        let value = serde_json::json!({ "x": { "items": ["a"] } });
        let err = extract_string_array(&value, "x.absent", Path::new("src.json")).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::CodegenSourceKeyMissing);
    }

    #[test]
    fn extract_rejects_traversal_into_non_object() {
        // The dot-path is object-key-only: descending into an array (or any
        // non-object) yields a key-missing error rather than panicking or
        // interpreting a numeric segment as an array index.
        let value = serde_json::json!({ "x": ["a", "b"] });
        let err = extract_string_array(&value, "x.0", Path::new("src.json")).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::CodegenSourceKeyMissing);
    }
}
