//! Shared declared `paths:` grammar for validation and context resolution.

pub(crate) const SYNTAX_HINT: &str =
    "use globset syntax; escape literal metacharacters with a backslash";

/// Whether a `paths:` value is a shape Claude Code reads as globs — a
/// comma-separated string or a list of strings.
pub(crate) fn is_glob_shaped(value: &yaml_serde::Value) -> bool {
    match value.as_sequence() {
        Some(seq) => seq.iter().all(yaml_serde::Value::is_string),
        None => value.is_string(),
    }
}

/// Every glob a `paths:` value carries, in declaration order. The string form
/// is comma-separated per the memory spec.
pub(crate) fn globs(value: &yaml_serde::Value) -> Vec<String> {
    match value.as_sequence() {
        Some(seq) => seq
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect(),
        None => value
            .as_str()
            .into_iter()
            .flat_map(split_patterns)
            .map(|s| s.trim().to_string())
            .collect(),
    }
}

fn split_patterns(value: &str) -> Vec<&str> {
    #[derive(Clone, Copy)]
    enum ClassPosition {
        Start,
        Negated,
        Body,
    }
    let mut depth = 0usize;
    let mut class = None;
    let mut escaped = false;
    let mut start = 0;
    let mut patterns = Vec::new();
    for (index, ch) in value.char_indices() {
        if let Some(position) = class {
            class = match (position, ch) {
                (ClassPosition::Start, '!' | '^') => Some(ClassPosition::Negated),
                (ClassPosition::Body, ']') => None,
                _ => Some(ClassPosition::Body),
            };
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => class = Some(ClassPosition::Start),
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                patterns.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    patterns.push(&value[start..]);
    patterns
}

pub(crate) fn compile_glob(pattern: &str) -> std::result::Result<globset::Glob, globset::Error> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
}

/// Whether a `paths:` value actually scopes the rule.
///
/// Presence of the key is not the question: `paths:` with no value, an empty
/// list, and a list of empty strings all carry zero globs, so Claude Code has
/// nothing to match the rule against and it is not path-scoped. Reading the
/// key alone would exempt such a rule from both the always-loaded budget and
/// the declaration requirement while it loads on every turn.
pub(crate) fn declares_scope(value: Option<&yaml_serde::Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value.as_sequence() {
        Some(seq) => seq
            .iter()
            .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())),
        None => value.as_str().is_some_and(|s| !s.trim().is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_validators_accept_compound_scalar_patterns() {
        use crate::config::{RulesPolicy, SkillsPolicy};
        use crate::validate::{RuleValidator, SkillValidator};
        use std::path::Path;
        let rules: RulesPolicy = toml::from_str("").unwrap();
        let skills: SkillsPolicy = toml::from_str("").unwrap();
        for pattern in [
            "a[b,c].md",
            "docs/[],a].md",
            "src/*.{ts,tsx}",
            "{src,test}/**/*.rs",
            "photos \\[2024/**",
        ] {
            let content = format!(
                "---\nname: probe\ndescription: Read-only probe.\npaths: '{pattern}'\n---\n"
            );
            assert!(
                RuleValidator::new(&rules)
                    .validate_text(&content, Path::new(".claude/rules/probe.md"))
                    .is_empty(),
                "rule: {pattern}"
            );
            assert!(
                SkillValidator::new(&skills)
                    .validate_text(&content, Path::new(".claude/skills/probe/SKILL.md"))
                    .is_empty(),
                "skill: {pattern}"
            );
        }
    }

    #[test]
    fn scope_patterns_keep_brace_and_bracket_commas() {
        let value =
            yaml_serde::Value::String("src/**/*.{ts,tsx}, docs/[a,b]*.md, data/\\,*.csv".into());
        assert_eq!(
            globs(&value),
            ["src/**/*.{ts,tsx}", "docs/[a,b]*.md", "data/\\,*.csv"]
        );
        for (input, expected) in [
            ("docs/[{,]*.md,*.txt", vec!["docs/[{,]*.md", "*.txt"]),
            ("{a[},],b}.md,x.txt", vec!["{a[},],b}.md", "x.txt"]),
        ] {
            assert_eq!(globs(&yaml_serde::Value::String(input.into())), expected);
        }
    }

    #[test]
    fn scope_matcher_obeys_separator_braces_escape_and_case() {
        for (pattern, path, expected) in [
            ("*.md", "a.md", true),
            ("*.md", "sub/a.md", false),
            ("**/*.md", ".hidden/a.md", true),
            ("**/*.md", "a.md", true),
            ("src/*.{ts,tsx}", "src/a.tsx", true),
            ("src/*.{ts,tsx}", "src/sub/a.ts", false),
            ("{src,test}/**/*.{ts,tsx}", "test/a.ts", true),
            ("photos \\[2024/**", "photos [2024/a.jpg", true),
            ("*.md", "a.MD", false),
        ] {
            assert_eq!(
                compile_glob(pattern)
                    .unwrap()
                    .compile_matcher()
                    .is_match(path),
                expected,
                "{pattern}: {path}"
            );
        }
        assert!(compile_glob("photos [2024/**").is_err());
    }
}
