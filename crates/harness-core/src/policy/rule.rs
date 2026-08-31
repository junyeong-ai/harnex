//! # rule — the Claude Code permission rule grammar
//!
//! A permission rule is `Tool` or `Tool(specifier)`, and Claude Code does not
//! consult every rule it accepts. Two documented shapes parse, merge across
//! scopes, and are then never read: a path rule for a tool the file
//! permission checks skip, and a specifier naming a tool's primary content
//! field. Both read as a guardrail and enforce nothing, which is the worst
//! failure a deny rule has — the operator believes a path is closed.
//!
//! A second failure reads: a `Bash` body whose reach differs from what its
//! spelling reads as to a person. The legacy `:*` suffix reads as a
//! namespace glob and matches as a word-boundary prefix, and a wildcard
//! matches across whitespace so literal text after one anchors only the
//! command's end (spec-facts § Bash matching, measured). Both function —
//! the operator just holds a different rule than the one they wrote.
//!
//! Every surface that writes, generates, or inspects a rule asks
//! [`PermissionRule::effect`] and [`PermissionRule::misleading`] rather
//! than matching on the string, so the grammar has one owner and a rule
//! that cannot be honored — or reads as one it is not — is refused where
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

/// Which permissions array a rule was authored in. A spelling can be a
/// defect in one direction and the fail-safe idiom in another: an allow
/// that reaches further than it reads grants silently, while a deny that
/// reaches further still refuses — which is the direction the baseline
/// profile writes deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDirection {
    Allow,
    Ask,
    Deny,
}

/// A rule a permission check reads, whose reach differs from what the
/// spelling reads as to a person. Both shapes are measured behaviour of the
/// matcher (spec-facts § Bash matching): the rule functions, so
/// [`RuleEffect`] calls it consulted — this is the second question, asked
/// beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MisleadingRule {
    pub reason: MisleadingReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MisleadingReason {
    /// The body ends in `:*`, which reads as a namespace glob and matches as
    /// a word-boundary prefix: `Bash(pnpm gate:*)` grants `pnpm gate` and
    /// `pnpm gate <args>`, never `pnpm gate:fast`.
    LegacyColonWildcard { prefix: String },
    /// Literal text follows a wildcard. The wildcard matches across
    /// whitespace, so that text anchors the end of the command and nothing
    /// else — the middle takes any arguments, options included.
    TailAfterWildcard { tail: String },
}

impl MisleadingRule {
    /// How the rule's reach differs from its reading.
    pub fn reason_text(&self) -> String {
        match &self.reason {
            MisleadingReason::LegacyColonWildcard { prefix } => format!(
                "the legacy `:*` suffix is a word-boundary prefix, not a namespace glob — it \
                 grants `{prefix}` and `{prefix} <args>`, never `{prefix}:<suffix>`"
            ),
            MisleadingReason::TailAfterWildcard { tail } => format!(
                "the wildcard matches across whitespace, so `{tail}` anchors the end of the \
                 command and nothing else — the middle takes any arguments, options included"
            ),
        }
    }

    /// What to write in its place.
    pub fn hint(&self) -> String {
        match &self.reason {
            MisleadingReason::LegacyColonWildcard { prefix } => format!(
                "write `{prefix} *` for the word-boundary prefix it matches as, or `{prefix}*` \
                 for the namespace glob it reads as"
            ),
            MisleadingReason::TailAfterWildcard { .. } => {
                "state the command exactly, or drop the rule so it prompts".to_string()
            }
        }
    }
}

/// A run of literal text, or a wildcard operator between runs, in a Bash
/// rule body. `**/` runs collapse into the `/` that opens them and a lone
/// `*` stands alone, mirroring the matcher's compiler — a character-wise
/// reader would count the second star of `**` as text placed after a
/// wildcard, which is exactly the misreading this tokenizer exists to avoid.
enum BodyToken {
    Literal(String),
    Wildcard,
}

fn tokenize_bash_body(body: &str) -> Vec<BodyToken> {
    let collapsed: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && matches!(chars.get(i + 1), Some('*') | Some('\\')) {
            literal.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '/' {
            let mut end = i + 1;
            while chars[end..].starts_with(&['*', '*', '/']) {
                end += 3;
            }
            if end > i + 1 {
                if !literal.is_empty() {
                    tokens.push(BodyToken::Literal(std::mem::take(&mut literal)));
                }
                tokens.push(BodyToken::Wildcard);
                i = end;
                continue;
            }
        }
        if chars[i] == '*' {
            if !literal.is_empty() {
                tokens.push(BodyToken::Literal(std::mem::take(&mut literal)));
            }
            tokens.push(BodyToken::Wildcard);
            i += 1;
            continue;
        }
        literal.push(chars[i]);
        i += 1;
    }
    if !literal.is_empty() {
        tokens.push(BodyToken::Literal(literal));
    }
    tokens
}

/// The visible text a body places after its first wildcard, if any does.
/// A literal that trims to nothing anchors nothing a reader can see, so it
/// does not count as a tail.
fn tail_after_wildcard(body: &str) -> Option<String> {
    let tokens = tokenize_bash_body(body);
    let first = tokens
        .iter()
        .position(|t| matches!(t, BodyToken::Wildcard))?;
    tokens[first + 1..].iter().find_map(|t| match t {
        BodyToken::Literal(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        BodyToken::Wildcard => None,
    })
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

    /// Whether the rule's reach differs from what its spelling reads as.
    ///
    /// Asked beside [`Self::effect`], never instead of it: a rule no check
    /// reads has nothing to mislead about, so an inert rule answers `None`
    /// here and the caller surfaces the inertness. Only `Bash` bodies carry
    /// the two measured shapes. The tail check runs on `Allow` only — a
    /// deny or an ask that reaches further than it reads still refuses or
    /// still asks, and the baseline deny writes that shape deliberately.
    pub fn misleading(&self, direction: RuleDirection) -> Option<MisleadingRule> {
        if self.tool != "Bash" || self.effect() != RuleEffect::Consulted {
            return None;
        }
        let body = self.specifier?;
        if let Some(prefix) = body.strip_suffix(":*") {
            // A bare `:*` has no prefix to be legacy for, and a prefix
            // spanning a line terminator is one the matcher's own legacy
            // pattern cannot reach — both read as an exact command,
            // misleading nobody. The suffix still shadows the wildcard
            // reading either way.
            if prefix.is_empty() || prefix.contains(['\n', '\r']) {
                return None;
            }
            return Some(MisleadingRule {
                reason: MisleadingReason::LegacyColonWildcard {
                    prefix: prefix.to_string(),
                },
            });
        }
        if direction == RuleDirection::Allow
            && let Some(tail) = tail_after_wildcard(body)
        {
            return Some(MisleadingRule {
                reason: MisleadingReason::TailAfterWildcard { tail },
            });
        }
        None
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

    fn misleads(rule: &str, direction: RuleDirection) -> Option<MisleadingReason> {
        PermissionRule::parse(rule)
            .misleading(direction)
            .map(|m| m.reason)
    }

    #[test]
    fn the_legacy_colon_wildcard_misleads_in_every_direction() {
        // Measured: `X:*` is a word-boundary prefix — `Bash(pnpm gate:*)`
        // grants `pnpm gate <args>` and never reaches `pnpm gate:fast`. The
        // spelling misreads whichever array carries it: an allow grants other
        // than it says, a deny leaves open what it reads as closing.
        for direction in [
            RuleDirection::Allow,
            RuleDirection::Ask,
            RuleDirection::Deny,
        ] {
            assert_eq!(
                misleads("Bash(pnpm gate:*)", direction),
                Some(MisleadingReason::LegacyColonWildcard {
                    prefix: "pnpm gate".into()
                }),
                "{direction:?}"
            );
        }
        let m = PermissionRule::parse("Bash(pnpm gate:*)")
            .misleading(RuleDirection::Allow)
            .unwrap();
        assert!(m.reason_text().contains("never `pnpm gate:<suffix>`"));
        assert!(m.hint().contains("`pnpm gate *`") && m.hint().contains("`pnpm gate*`"));
    }

    #[test]
    fn a_tail_after_a_wildcard_misleads_only_where_it_grants() {
        // Measured: a wildcard matches across whitespace, so the literal
        // after one anchors only the end — `Bash(pnpm --filter * dev)` reads
        // as one package name and grants `pnpm --filter x exec … dev`. A deny
        // spelled that way reaches further and still refuses, which is the
        // fail-safe direction the baseline writes deliberately.
        assert_eq!(
            misleads("Bash(pnpm --filter * dev)", RuleDirection::Allow),
            Some(MisleadingReason::TailAfterWildcard { tail: "dev".into() })
        );
        assert_eq!(
            misleads("Bash(pnpm --filter * dev)", RuleDirection::Deny),
            None
        );
        assert_eq!(
            misleads("Bash(pnpm --filter * dev)", RuleDirection::Ask),
            None
        );
        assert_eq!(
            misleads("Bash(gcloud * projects delete *)", RuleDirection::Deny),
            None
        );
    }

    #[test]
    fn the_sanctioned_spellings_mislead_nowhere() {
        // Trailing star (the prefix-plus-arguments idiom), an exact command,
        // a leading wildcard whose tail is its whole point of reference on
        // the deny side, and an escaped star that is literal text.
        for rule in [
            "Bash(git commit -m *)",
            "Bash(npm run build)",
            "Bash(curl https://api *)",
            "Bash(echo \\* rest)",
        ] {
            assert_eq!(misleads(rule, RuleDirection::Allow), None, "{rule}");
        }
        assert_eq!(misleads("Read(.env)", RuleDirection::Allow), None);
        assert_eq!(misleads("Bash", RuleDirection::Allow), None);
    }

    #[test]
    fn an_inert_rule_is_inert_before_it_is_misleading() {
        // `Bash(command:*)` ends in `:*` AND names the primary content
        // field; the check no reader consults is the finding, and reporting
        // both would prescribe two rewrites for one rule.
        assert!(matches!(effect("Bash(command:*)"), RuleEffect::Inert(_)));
        assert_eq!(misleads("Bash(command:*)", RuleDirection::Allow), None);
    }

    #[test]
    fn a_bare_colon_star_is_exact_and_an_invisible_tail_is_no_tail() {
        // `:*` strips to an empty prefix — the matcher reads it as an exact
        // command, and a finding about it would prescribe rewrites of
        // nothing. A tail that trims to whitespace anchors nothing a reader
        // can see; the next visible literal, if any, is the tail.
        assert_eq!(misleads("Bash(:*)", RuleDirection::Allow), None);
        assert_eq!(misleads("Bash(foo\nbar:*)", RuleDirection::Allow), None);
        assert_eq!(misleads("Bash(a * *)", RuleDirection::Allow), None);
        assert_eq!(
            misleads("Bash(a * * b)", RuleDirection::Allow),
            Some(MisleadingReason::TailAfterWildcard { tail: "b".into() })
        );
    }

    #[test]
    fn a_globstar_run_reads_as_one_wildcard() {
        // The second star of `**` and the slashes of a `/**/` run are the
        // operator's own spelling, not text placed after a wildcard — while
        // a literal beyond the run still anchors only the end.
        assert_eq!(misleads("Bash(cat /logs/**)", RuleDirection::Allow), None);
        assert_eq!(
            misleads("Bash(cat /logs/**/latest)", RuleDirection::Allow),
            Some(MisleadingReason::TailAfterWildcard {
                tail: "latest".into()
            })
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
