//! Rendering strategies for sentinel-block targets.
//!
//! The closed set of strategies is the [`RendererStrategy`] enum — its
//! variants are the single source of truth. Both config validation
//! (`Config::validate_codegen`) and the factory ([`renderer_for`]) consume
//! the enum, so a new variant forces both sites to update at compile time.

pub trait Renderer: Send + Sync {
    /// Render values for embedding between sentinels. `name` is used by
    /// assignment renderers (TOML/Bash); list renderers ignore it.
    fn render(&self, name: Option<&str>, values: &[String]) -> String;
}

/// Closed set of supported renderer strategies. Adding a variant requires
/// updating [`from_str`], [`as_str`], [`ALL`], and the match in
/// [`renderer_for`] — all of which the compiler enforces via exhaustive
/// `match` on `Self`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererStrategy {
    TomlArrayAssignment,
    BashArrayAssignment,
    MarkdownBulletList,
}

impl RendererStrategy {
    pub const ALL: &'static [Self] = &[
        Self::TomlArrayAssignment,
        Self::BashArrayAssignment,
        Self::MarkdownBulletList,
    ];

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "toml-array-assignment" => Self::TomlArrayAssignment,
            "bash-array-assignment" => Self::BashArrayAssignment,
            "markdown-bullet-list" => Self::MarkdownBulletList,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TomlArrayAssignment => "toml-array-assignment",
            Self::BashArrayAssignment => "bash-array-assignment",
            Self::MarkdownBulletList => "markdown-bullet-list",
        }
    }
}

/// Construct a renderer for the named strategy. Returns `None` for any
/// string that does not correspond to a [`RendererStrategy`] variant.
pub fn renderer_for(name: &str) -> Option<Box<dyn Renderer>> {
    Some(match RendererStrategy::from_str(name)? {
        RendererStrategy::TomlArrayAssignment => Box::new(TomlArrayAssignmentRenderer),
        RendererStrategy::BashArrayAssignment => Box::new(BashArrayAssignmentRenderer),
        RendererStrategy::MarkdownBulletList => Box::new(MarkdownBulletListRenderer),
    })
}

fn escape_double_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) struct TomlArrayAssignmentRenderer;

impl Renderer for TomlArrayAssignmentRenderer {
    fn render(&self, name: Option<&str>, values: &[String]) -> String {
        let body = values
            .iter()
            .map(|v| format!("\"{}\"", escape_double_quotes(v)))
            .collect::<Vec<_>>()
            .join(", ");
        match name {
            Some(n) => format!("{n} = [{body}]"),
            None => format!("[{body}]"),
        }
    }
}

pub(crate) struct BashArrayAssignmentRenderer;

impl Renderer for BashArrayAssignmentRenderer {
    fn render(&self, name: Option<&str>, values: &[String]) -> String {
        let body = values
            .iter()
            .map(|v| format!("\"{}\"", escape_double_quotes(v)))
            .collect::<Vec<_>>()
            .join(" ");
        match name {
            Some(n) => format!("{n}=({body})"),
            None => format!("({body})"),
        }
    }
}

pub(crate) struct MarkdownBulletListRenderer;

impl Renderer for MarkdownBulletListRenderer {
    fn render(&self, _name: Option<&str>, values: &[String]) -> String {
        values
            .iter()
            .map(|v| format!("- {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_assignment_with_name() {
        let out = TomlArrayAssignmentRenderer.render(Some("allowed"), &["a".into(), "b".into()]);
        assert_eq!(out, "allowed = [\"a\", \"b\"]");
    }

    #[test]
    fn bash_assignment_with_name() {
        let out = BashArrayAssignmentRenderer.render(Some("KINDS"), &["a".into(), "b".into()]);
        assert_eq!(out, "KINDS=(\"a\" \"b\")");
    }

    #[test]
    fn markdown_list_ignores_name() {
        let out = MarkdownBulletListRenderer.render(Some("ignored"), &["a".into(), "b".into()]);
        assert_eq!(out, "- a\n- b");
    }

    #[test]
    fn toml_quote_escaping() {
        let out = TomlArrayAssignmentRenderer.render(None, &["he said \"hi\"".into()]);
        assert_eq!(out, "[\"he said \\\"hi\\\"\"]");
    }

    #[test]
    fn unknown_renderer_returns_none() {
        assert!(renderer_for("xyz").is_none());
    }

    #[test]
    fn strategy_from_str_round_trips_every_variant() {
        for s in RendererStrategy::ALL {
            assert_eq!(RendererStrategy::from_str(s.as_str()), Some(*s));
        }
    }

    #[test]
    fn strategy_from_str_rejects_unknown() {
        assert_eq!(RendererStrategy::from_str("xyz"), None);
    }
}
