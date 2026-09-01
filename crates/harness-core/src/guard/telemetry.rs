//! PostToolUse / PostToolUseFailure telemetry emit — the auto-emit seam that
//! feeds the `harness_invocation` ledger.
//!
//! One event per harness-element invocation: the element's own slug and
//! whether the call succeeded. The slug is resolved through
//! [`crate::session::asset_of`] — the one owner of the tool → element mapping,
//! shared with the transcript reader so the recorded set cannot drift from the
//! measured one. Outcome is the hook event's identity, never a payload field,
//! so a failure cannot be recorded as a success.
//!
//! ## What this module refuses to do
//!
//! - Never emit anything but the element's slug and the outcome — not a tool's
//!   arguments, not content. The slug is an element's own name, which the
//!   session contract reports plainly; nothing a person typed crosses.
//! - Never block a tool call. Every reason it cannot record — no `[telemetry]`
//!   section, the Kind undeclared, a write failure — is a silent no-op, not an
//!   error. Telemetry that failed loud on absence would trade a measurement for
//!   an interruption.

use std::path::Path;

use crate::config::Config;
use crate::session::asset_of;
use crate::telemetry::{JsonlStorage, TelemetryAppender};

/// The Kind every auto-emit event lands in. Declared in the scaffold's
/// `harness.toml`; a drift guard holds the two equal.
pub const HARNESS_INVOCATION_KIND: &str = "harness_invocation";

/// The payload field carrying the invoked element's slug.
pub const SURFACE_FIELD: &str = "surface";
/// The payload field carrying the outcome (`ok` / `failed`).
pub const OUTCOME_FIELD: &str = "outcome";

/// What a hook event resolved to, for the caller to act on. The command maps
/// every arm to exit 0; the distinction exists for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitOutcome {
    /// One event was appended for `surface` with `outcome`.
    Recorded { surface: String, outcome: String },
    /// Nothing to record — not an outcome-bearing event, not an element
    /// invocation, or telemetry is not configured for this Kind. Never an error.
    Skipped,
}

/// Whether a resolved name is an element slug: non-empty and drawn from the
/// character class the skill and agent name validators enforce — lowercase
/// ASCII, digits, and `-` (`validate::skills` / `validate::agents`
/// `NAME_PATTERN`). That is exactly a project element name, so every genuine
/// invocation records: the emit imposes no length of its own, matching the
/// agent validator, which sets no maximum (the skill validator caps at 64), so
/// a long agent name is no false Silent. A value carrying content — whitespace,
/// a `=`, a `;`, an uppercase letter — is not a name and is declined.
///
/// This is a syntactic bound, not project-element membership: a value that is
/// nonetheless well-formed — a failed invocation of a name no element has —
/// records. It is a stated residual, not a hole. The record is inert to the
/// one consumer: the retirement scan matches a slug against a real project
/// element, which a name no element has never is, so it changes no verdict;
/// only the element's own name-shaped field ever crosses, never a tool's
/// arguments; and it stays a strict subset of what `session::asset_of` records
/// with no validation at all. Proving membership would cost a project-tree read
/// on every tool call, on the hot path, to reject a value already inert — a
/// price the port source does not pay either (it checks shape, not membership).
///
/// A plugin element's invocation carries the namespace separator `:`
/// (`plugin:name`) and so declines — correctly: the retirement sweep globs the
/// project tree only, so a plugin element is not one of its subjects, and its
/// slug not reaching the ledger changes no verdict.
fn is_slug_shaped(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Resolve the outcome an event name carries, or `None` when the event is not
/// one the ledger records an outcome for.
fn outcome_of(event: &str) -> Option<&'static str> {
    match event {
        "PostToolUse" => Some("ok"),
        "PostToolUseFailure" => Some("failed"),
        _ => None,
    }
}

/// Emit one `harness_invocation` event for a hook payload, from a config
/// discovered by walking up from `working_dir`. Any reason it cannot record is
/// [`EmitOutcome::Skipped`], never an error — the caller exits 0 regardless.
pub fn emit(working_dir: &Path, hook_json: &str) -> EmitOutcome {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(hook_json) else {
        return EmitOutcome::Skipped;
    };
    let Some(outcome) = event
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .and_then(outcome_of)
    else {
        return EmitOutcome::Skipped;
    };
    let (Some(tool), Some(input)) = (
        event.get("tool_name").and_then(|v| v.as_str()),
        event.get("tool_input"),
    ) else {
        return EmitOutcome::Skipped;
    };
    // The element's slug, from the one owner of the tool → element mapping. A
    // tool that invokes no element, or a call missing the element's key, is
    // nothing to record.
    let Some(asset) = asset_of(tool, input) else {
        return EmitOutcome::Skipped;
    };
    // The slug is an element's own name — but the field it is read from is
    // agent-supplied, so bound it to a slug's shape before it crosses: a real
    // name is one line and short. A control character or an implausible length
    // is not a name; declining to record it keeps the field from becoming a
    // content channel, without diverging from the session contract for the
    // ordinary case.
    if !is_slug_shaped(&asset.name) {
        return EmitOutcome::Skipped;
    }

    let mut payload = serde_json::Map::new();
    payload.insert(SURFACE_FIELD.into(), asset.name.clone().into());
    payload.insert(OUTCOME_FIELD.into(), outcome.into());
    let payload = serde_json::Value::Object(payload);
    if append(working_dir, &payload).is_none() {
        return EmitOutcome::Skipped;
    }
    EmitOutcome::Recorded {
        surface: asset.name,
        outcome: outcome.to_string(),
    }
}

/// Append to the ledger, or `None` on any reason it cannot — config not found,
/// no `[telemetry]`, the Kind undeclared, a write failure. Every one is a
/// silent no-op: telemetry never blocks the tool call that triggered it.
fn append(working_dir: &Path, payload: &serde_json::Value) -> Option<()> {
    let (config, config_path) = Config::load(working_dir).ok()?;
    let tcfg = config.telemetry.as_ref()?;
    let config_dir = config_path.parent().unwrap_or(working_dir);
    let storage_dir = if tcfg.storage_dir.is_absolute() {
        tcfg.storage_dir.clone()
    } else {
        config_dir.join(&tcfg.storage_dir)
    };
    let storage = JsonlStorage::new(storage_dir, tcfg.rotate_at_mb);
    let mut appender = TelemetryAppender::new(tcfg, storage).ok()?;
    appender
        .append(HARNESS_INVOCATION_KIND, payload.clone())
        .ok()?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCAFFOLD_TELEMETRY: &str = r#"
        [meta]
        harnex_version = ">=0.2, <0.3"
        [telemetry]
        storage_dir = ".harness/telemetry"
        [[telemetry.kinds]]
        name = "harness_invocation"
        payload_schema = { type = "object", required = ["surface", "outcome"], properties = { surface = { type = "string" }, outcome = { type = "string", enum = ["ok", "failed"] } } }
    "#;

    fn scaffold_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("harness.toml"), SCAFFOLD_TELEMETRY).unwrap();
        dir
    }

    #[test]
    fn records_a_skill_invocation_by_its_slug() {
        let dir = scaffold_dir();
        let hook = r#"{"hook_event_name":"PostToolUse","tool_name":"Skill","tool_input":{"skill":"review-lenses","args":"go"}}"#;
        assert_eq!(
            emit(dir.path(), hook),
            EmitOutcome::Recorded {
                surface: "review-lenses".into(),
                outcome: "ok".into()
            }
        );
    }

    #[test]
    fn records_an_agent_invocation_from_the_subagent_type_key() {
        let dir = scaffold_dir();
        for tool in ["Task", "Agent"] {
            let hook = format!(
                r#"{{"hook_event_name":"PostToolUseFailure","tool_name":"{tool}","tool_input":{{"subagent_type":"session-judge"}}}}"#
            );
            assert_eq!(
                emit(dir.path(), &hook),
                EmitOutcome::Recorded {
                    surface: "session-judge".into(),
                    outcome: "failed".into()
                }
            );
        }
    }

    #[test]
    fn outcome_is_the_event_never_a_payload_field_that_claims_otherwise() {
        let dir = scaffold_dir();
        // A hostile payload field cannot flip a failure to a success.
        let hook = r#"{"hook_event_name":"PostToolUseFailure","tool_name":"Skill","tool_input":{"skill":"x","outcome":"ok"}}"#;
        assert_eq!(
            emit(dir.path(), hook),
            EmitOutcome::Recorded {
                surface: "x".into(),
                outcome: "failed".into()
            }
        );
    }

    #[test]
    fn a_surface_carrying_content_is_declined_but_a_long_valid_slug_records() {
        let dir = scaffold_dir();
        // Content — whitespace, punctuation, a newline — is not a name.
        for content in ["review secret=leak", "review\nleak", "Review", "a;b", "a@b"] {
            let hook = format!(
                r#"{{"hook_event_name":"PostToolUse","tool_name":"Skill","tool_input":{{"skill":{}}}}}"#,
                serde_json::to_string(content).unwrap()
            );
            assert_eq!(emit(dir.path(), &hook), EmitOutcome::Skipped, "{content}");
        }
        // A long but valid lowercase-hyphen agent name is a real element and
        // must record — no length cap, so no false Silent (the exact failure
        // the original wrong-key bug caused).
        let long = format!("agent-{}", "x".repeat(200));
        let hook = format!(
            r#"{{"hook_event_name":"PostToolUse","tool_name":"Agent","tool_input":{{"subagent_type":"{long}"}}}}"#
        );
        assert_eq!(
            emit(dir.path(), &hook),
            EmitOutcome::Recorded {
                surface: long,
                outcome: "ok".into()
            }
        );
    }

    #[test]
    fn a_non_element_tool_is_nothing_to_record() {
        let dir = scaffold_dir();
        for hook in [
            r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"mcp__srv__tool","tool_input":{}}"#,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Skill","tool_input":{"skill":"x"}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"Skill","tool_input":{"name":"wrong-key"}}"#,
        ] {
            assert_eq!(emit(dir.path(), hook), EmitOutcome::Skipped, "{hook}");
        }
    }

    #[test]
    fn malformed_input_and_absent_telemetry_are_silent_skips() {
        let dir = scaffold_dir();
        assert_eq!(emit(dir.path(), "{not json"), EmitOutcome::Skipped);
        // No config at all: still a skip, never an error.
        let empty = tempfile::tempdir().unwrap();
        let hook =
            r#"{"hook_event_name":"PostToolUse","tool_name":"Skill","tool_input":{"skill":"x"}}"#;
        assert_eq!(emit(empty.path(), hook), EmitOutcome::Skipped);
    }
}
