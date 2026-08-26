//! # rule — the Claude Code permission rule grammar
//!
//! A permission rule is `Tool` or `Tool(specifier)`, and Claude Code does not
//! consult every rule it accepts. Two documented shapes parse, merge across
//! scopes, and are then never read: a path rule for a tool the file
//! permission checks skip, and a specifier naming a tool's primary content
//! field. Both read as a guardrail and enforce nothing, which is the worst
//! failure a deny rule has — the operator believes a path is closed.
//!
//! Every surface that writes, generates, or inspects a rule asks
//! [`PermissionRule::effect`] rather than matching on the string, so the
//! grammar has one owner and a rule that cannot be honored is refused where
//! it is declared instead of shipping into a settings file.
//!
//! ## What this module refuses to do
//!
//! - Never guess at an undocumented shape. The closed sets below are exactly
//!   what /en/permissions enumerates; a tool absent from them is consulted as
//!   far as this crate is concerned, because a false "your rule does nothing"
//!   costs more than a missed one.
//! - Never infer a rewrite that is not mechanical. A file-tool path rule maps
//!   onto its consulted tool unchanged; a content-field rule does not, so it
//!   is reported without one.

/// File-editing tools whose path rules the permission checks never consult,
/// because `Edit(path)` already covers every built-in tool that edits files.
pub const COVERED_BY_EDIT_RULES: &[&str] = &["MultiEdit", "NotebookEdit", "Write"];

/// File-reading tools whose path rules the permission checks never consult,
/// because `Read(path)` already covers them. A `Glob` rule passed in
/// `--allowedTools` is the documented exception and never reaches a settings
/// file, which is the only surface this crate reads.
pub const COVERED_BY_READ_RULES: &[&str] = &["Glob"];

/// Each tool's primary content field, `Tool:field`. `Tool(param:value)`
/// matching refuses exactly these — a content match is bypassable by a
/// compound command — so a rule naming one is ignored.
pub const PRIMARY_CONTENT_FIELDS: &[&str] = &[
    "Bash:command",
    "Edit:file_path",
    "Glob:path",
    "Grep:path",
    "NotebookEdit:notebook_path",
    "PowerShell:command",
    "Read:file_path",
    "WebFetch:url",
    "Write:file_path",
];

/// Every closed set this module reads from the permissions page, labelled.
/// The measurement stamp digests exactly this list.
pub const SPEC_SETS: &[(&str, &[&str])] = &[
    ("covered-by-edit-rules", COVERED_BY_EDIT_RULES),
    ("covered-by-read-rules", COVERED_BY_READ_RULES),
    ("primary-content-fields", PRIMARY_CONTENT_FIELDS),
];

/// Whether Claude Code reads a rule once it has accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleEffect {
    /// A permission check consults the rule.
    Consulted,
    /// The rule is stored and no check ever reads it.
    Inert(InertRule),
}

/// A rule Claude Code accepts and never consults, and what to write instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InertRule {
    pub reason: InertReason,
    /// The same intent spelled where a check reads it, when the rewrite is
    /// mechanical. `None` when only the tool's own specifier syntax can
    /// express it and this module would have to invent the translation.
    pub rewrite_as: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InertReason {
    /// File permission checks consult `Read(path)` and `Edit(path)` only.
    UnconsultedFileTool { consulted: &'static str },
    /// The specifier names the tool's primary content field.
    PrimaryContentField { field: &'static str },
}

impl InertRule {
    /// Why no check reads the rule.
    pub fn reason_text(&self) -> String {
        match self.reason {
            InertReason::UnconsultedFileTool { consulted } => format!(
                "file permission checks consult `Read(path)` and `Edit(path)` only, and \
                 `{consulted}` already covers this tool"
            ),
            InertReason::PrimaryContentField { field } => format!(
                "`{field}` is the tool's primary content field, which parameter matching refuses"
            ),
        }
    }

    /// What to write in its place.
    pub fn hint(&self) -> String {
        match (&self.reason, &self.rewrite_as) {
            (_, Some(rewrite)) => format!("state it as `{rewrite}`"),
            (InertReason::PrimaryContentField { field }, None) => {
                format!("drop the `{field}:` parameter and use the tool's own specifier syntax")
            }
            (InertReason::UnconsultedFileTool { consulted }, None) => {
                format!("state it as a `{consulted}` rule")
            }
        }
    }
}

/// A parsed permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionRule<'a> {
    tool: &'a str,
    specifier: Option<&'a str>,
}

impl<'a> PermissionRule<'a> {
    /// Split a rule into its tool and specifier.
    ///
    /// Total by construction: anything without a balanced trailing
    /// `(specifier)` is the documented bare tool-name form, which matches at
    /// the tool level and is always consulted. `Tool(*)` is equivalent to
    /// `Tool`, so it carries no specifier either.
    pub fn parse(rule: &'a str) -> Self {
        let Some((tool, specifier)) = rule.strip_suffix(')').and_then(|r| r.split_once('(')) else {
            return Self {
                tool: rule,
                specifier: None,
            };
        };
        Self {
            tool,
            specifier: (specifier != "*").then_some(specifier),
        }
    }

    pub fn tool(&self) -> &'a str {
        self.tool
    }

    pub fn specifier(&self) -> Option<&'a str> {
        self.specifier
    }

    /// Whether a permission check reads this rule.
    pub fn effect(&self) -> RuleEffect {
        let Some(specifier) = self.specifier else {
            return RuleEffect::Consulted;
        };
        if let Some(consulted) = self.consulted_file_tool() {
            return RuleEffect::Inert(InertRule {
                reason: InertReason::UnconsultedFileTool { consulted },
                rewrite_as: Some(format!("{consulted}({specifier})")),
            });
        }
        if let Some(field) = self.primary_content_field(specifier) {
            return RuleEffect::Inert(InertRule {
                reason: InertReason::PrimaryContentField { field },
                rewrite_as: None,
            });
        }
        RuleEffect::Consulted
    }

    /// The `Bash` command this rule governs, stripped of the equivalent
    /// trailing wildcard spellings so `Bash(rm *)`, `Bash(rm:*)` and
    /// `Bash(rm)` reduce alike. `None` for every other tool.
    pub fn bash_base(&self) -> Option<String> {
        (self.tool == "Bash").then_some(())?;
        Some(
            self.specifier?
                .trim_end_matches('*')
                .trim_end_matches(':')
                .trim()
                .to_string(),
        )
    }

    fn consulted_file_tool(&self) -> Option<&'static str> {
        if COVERED_BY_EDIT_RULES.contains(&self.tool) {
            return Some("Edit");
        }
        if COVERED_BY_READ_RULES.contains(&self.tool) {
            return Some("Read");
        }
        None
    }

    fn primary_content_field(&self, specifier: &str) -> Option<&'static str> {
        let named = specifier.split_once(':')?.0.trim();
        PRIMARY_CONTENT_FIELDS
            .iter()
            .copied()
            .filter_map(|entry| entry.split_once(':'))
            .find(|(tool, field)| *tool == self.tool && *field == named)
            .map(|(_, field)| field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(rule: &str) -> RuleEffect {
        PermissionRule::parse(rule).effect()
    }

    fn inert(rule: &str) -> InertRule {
        match effect(rule) {
            RuleEffect::Inert(i) => i,
            RuleEffect::Consulted => panic!("'{rule}' should be inert"),
        }
    }

    #[test]
    fn a_bare_tool_name_parses_without_a_specifier() {
        let r = PermissionRule::parse("Bash");
        assert_eq!(r.tool(), "Bash");
        assert_eq!(r.specifier(), None);
    }

    #[test]
    fn a_specifier_is_split_from_its_tool() {
        let r = PermissionRule::parse("Bash(npm run *)");
        assert_eq!(r.tool(), "Bash");
        assert_eq!(r.specifier(), Some("npm run *"));
    }

    #[test]
    fn an_mcp_rule_carries_no_specifier() {
        for rule in ["mcp__server", "mcp__server__tool", "mcp__server__*"] {
            let r = PermissionRule::parse(rule);
            assert_eq!(r.tool(), rule);
            assert_eq!(r.specifier(), None);
        }
    }

    #[test]
    fn a_star_specifier_is_the_bare_tool_form() {
        // `Bash(*)` is documented as equivalent to `Bash`. Reading it as a
        // path would make `Write(*)` — a tool-level rule someone reasonably
        // writes — report as a rule that does nothing.
        assert_eq!(PermissionRule::parse("Write(*)").specifier(), None);
        assert_eq!(effect("Write(*)"), RuleEffect::Consulted);
        assert_eq!(effect("Bash(*)"), RuleEffect::Consulted);
    }

    #[test]
    fn a_path_rule_for_an_unconsulted_file_tool_is_inert() {
        for tool in COVERED_BY_EDIT_RULES {
            let rule = format!("{tool}(/secrets/**)");
            assert_eq!(
                inert(&rule).rewrite_as.as_deref(),
                Some("Edit(/secrets/**)"),
                "{rule}"
            );
        }
        for tool in COVERED_BY_READ_RULES {
            let rule = format!("{tool}(/secrets/**)");
            assert_eq!(
                inert(&rule).rewrite_as.as_deref(),
                Some("Read(/secrets/**)"),
                "{rule}"
            );
        }
    }

    #[test]
    fn the_consulted_file_tools_are_consulted() {
        for rule in ["Read(.env)", "Edit(*.pem)", "Edit(~/.ssh/*)"] {
            assert_eq!(effect(rule), RuleEffect::Consulted, "{rule}");
        }
    }

    #[test]
    fn a_primary_content_field_rule_is_inert_without_a_rewrite() {
        let i = inert("Bash(command:rm *)");
        assert_eq!(
            i.reason,
            InertReason::PrimaryContentField { field: "command" }
        );
        assert_eq!(
            i.rewrite_as, None,
            "no mechanical rewrite exists — inventing one risks widening the rule"
        );
        for rule in [
            "Read(file_path:.env)",
            "Edit(file_path:.env)",
            "Grep(path:src)",
            "PowerShell(command:Remove-Item *)",
            "WebFetch(url:https://example.com)",
        ] {
            assert!(matches!(
                effect(rule),
                RuleEffect::Inert(InertRule {
                    reason: InertReason::PrimaryContentField { .. },
                    ..
                })
            ));
        }
    }

    #[test]
    fn a_parameter_rule_on_any_other_field_is_consulted() {
        // `Tool(param:value)` is the documented deny/ask matcher. Only the
        // primary content field is refused, so flagging the rest would call
        // a working guardrail dead.
        for rule in [
            "Agent(model:opus)",
            "Agent(isolation:worktree)",
            "Bash(run_in_background:true)",
            "WebFetch(domain:github.com)",
        ] {
            assert_eq!(effect(rule), RuleEffect::Consulted, "{rule}");
        }
    }

    #[test]
    fn a_trailing_colon_wildcard_is_not_a_parameter_rule() {
        // `Bash(ls:*)` ≡ `Bash(ls *)`; the field check reads the name before
        // the colon, and `ls` is not a Bash input field.
        assert_eq!(effect("Bash(ls:*)"), RuleEffect::Consulted);
        assert_eq!(effect("Bash(git push:*)"), RuleEffect::Consulted);
    }

    #[test]
    fn whitespace_around_the_parameter_colon_is_ignored() {
        assert!(matches!(
            effect("Bash(command :rm *)"),
            RuleEffect::Inert(_)
        ));
    }

    #[test]
    fn bash_base_collapses_the_equivalent_wildcard_spellings() {
        for rule in ["Bash(rm *)", "Bash(rm:*)", "Bash(rm)"] {
            assert_eq!(
                PermissionRule::parse(rule).bash_base().as_deref(),
                Some("rm"),
                "{rule}"
            );
        }
        assert_eq!(PermissionRule::parse("Read(.env)").bash_base(), None);
        assert_eq!(PermissionRule::parse("Bash").bash_base(), None);
    }

    #[test]
    fn a_scoped_bash_rule_keeps_its_longer_base() {
        assert_eq!(
            PermissionRule::parse("Bash(curl https://api *)")
                .bash_base()
                .as_deref(),
            Some("curl https://api")
        );
    }

    #[test]
    fn every_content_field_entry_names_a_tool_and_a_field() {
        for entry in PRIMARY_CONTENT_FIELDS {
            let (tool, field) = entry
                .split_once(':')
                .unwrap_or_else(|| panic!("'{entry}' must be spelled `Tool:field`"));
            assert!(!tool.is_empty() && !field.is_empty(), "{entry}");
        }
    }

    #[test]
    fn no_tool_is_covered_by_both_read_and_edit_rules() {
        for tool in COVERED_BY_EDIT_RULES {
            assert!(
                !COVERED_BY_READ_RULES.contains(tool),
                "'{tool}' cannot defer to two consulted tools"
            );
        }
    }
}
