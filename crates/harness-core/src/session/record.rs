//! # record — the transcript shapes this module consumes, and what it could not read
//!
//! A Claude Code transcript is JSONL whose vocabulary is undocumented and
//! moves: the local corpus spans 27 runtime versions and 22 record `type`
//! values, several of which arrived recently. Modelling that vocabulary as a
//! closed enum would make a future record type a parse failure, and a parse
//! failure that is skipped reads as a shorter session — a *better* number,
//! arrived at silently. So only the two record types this module consumes are
//! closed; every other value is counted into [`Coverage`] under its own name.
//!
//! [`Authorship`] is the one classification made here, and it is made from the
//! runtime's own attribution rather than from the text. `origin.kind` is the
//! runtime's answer to "whose turn is this"; where it is absent the runtime
//! made no claim, and neither does this module. Reconstructing authorship from
//! phrasing would put a word list in source that tomorrow's phrasing is not in.
//!
//! ## What this module refuses to do
//!
//! - Never infer authorship from message text. `origin.kind` and
//!   `promptSource` are the only inputs; an unrecognised prompt source
//!   classifies as [`Authorship::SourceUnrecognised`], never as authored, so
//!   an upstream addition lowers coverage instead of entering the statistics.
//! - Never model the full record vocabulary. Unconsumed types are counted,
//!   not rejected and not dropped.
//! - Never carry prompt text or tool input into a serialised shape. Text is
//!   held for analysis and released; [`ToolAction`] compares inputs in memory
//!   and serialises only the tool name.
//! - Never treat "read nothing" as "found nothing". A discovered file that
//!   cannot be opened is counted, and the caller is told.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Prompt sources whose text the operator typed.
///
/// Inverted deliberately: naming the authored sources means a source this
/// binary has not seen — a new one, or an absent field — falls to
/// [`Authorship::SourceUnrecognised`] and shows up as reduced coverage. Naming
/// the *un*authored ones instead would let an upstream addition enter the
/// prompt statistics as though the operator had written it.
const AUTHORED_PROMPT_SOURCES: &[&str] = &["typed", "queued"];

/// The prompt source the runtime records when a turn was submitted while the
/// agent was still working.
///
/// This is what makes a submission boundary observable rather than guessed. A
/// person sending three messages before the agent replies has sent one
/// instruction, and asking a clock how close together they arrived is a proxy
/// for a fact the runtime already wrote down. Measured across 430 consecutive
/// pairs under ten seconds, 79% end in this source; the gap threshold that
/// would have stood in for it is deleted rather than tuned.
const CONTINUATION_PROMPT_SOURCE: &str = "queued";

/// Record types this module reads. Everything else is counted by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsumedType {
    User,
    Assistant,
}

impl ConsumedType {
    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            _ => return None,
        })
    }
}

/// Where a fact came from, precise enough to reopen.
///
/// Every fact this module emits carries one. It is what lets a reader verify a
/// number by hand and what lets the skill read the surrounding turns to reason
/// about a finding the oracle only counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Citation {
    pub session: String,
    pub file: PathBuf,
    pub uuid: String,
    pub timestamp: Timestamp,
}

/// The runtime's attribution of a user turn, as recorded.
///
/// Only [`Authorship::Authored`] enters prompt statistics. The rest are
/// reported separately so a consumer sees what was set aside and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Authorship {
    /// `origin.kind == "human"` with a prompt source the operator types.
    Authored,
    /// `origin.kind == "human"` with a prompt source outside
    /// [`AUTHORED_PROMPT_SOURCES`], including an absent one. The runtime says a
    /// person is behind the turn but not that they wrote its text.
    SourceUnrecognised,
    /// `origin.kind` present and not `human` — the runtime attributed the turn
    /// to something other than the operator.
    Attributed,
    /// A subagent transcript turn.
    Sidechain,
    /// No `origin.kind`. The runtime made no authorship claim, so neither does
    /// this module. Measured at roughly 45% of text-bearing user records across
    /// every runtime version in the local corpus — it is the ordinary shape of
    /// interrupts, resumptions and injected context, not a defect.
    Unclaimed,
}

impl Authorship {
    pub const ALL: &'static [Self] = &[
        Self::Authored,
        Self::SourceUnrecognised,
        Self::Attributed,
        Self::Sidechain,
        Self::Unclaimed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::SourceUnrecognised => "source-unrecognised",
            Self::Attributed => "attributed",
            Self::Sidechain => "sidechain",
            Self::Unclaimed => "unclaimed",
        }
    }

    /// Whether the runtime claimed a person was behind this turn. The coverage
    /// ratio is taken over this population: turns the runtime never claimed are
    /// outside the question, not failures to answer it.
    fn claims_a_person(self) -> bool {
        matches!(self, Self::Authored | Self::SourceUnrecognised)
    }
}

/// One tool invocation, reduced to what an equality test needs.
///
/// `input` stays private and unserialised: a Bash input carries the operator's
/// command text, and the envelope is not a place for it. Comparing whole
/// inputs rather than a per-tool "target" field keeps every tool's argument
/// vocabulary out of this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolAction {
    pub tool: String,
    input: serde_json::Value,
}

impl ToolAction {
    pub fn same_action(&self, other: &Self) -> bool {
        self.tool == other.tool && self.input == other.input
    }
}

/// A user turn, reduced to what this module reads.
#[derive(Debug, Clone)]
pub struct UserTurn {
    pub citation: Citation,
    pub authorship: Authorship,
    /// Joined text of a text-only message. `None` when the message carries
    /// anything else — a tool result is a tool result, not a turn.
    pub text: Option<String>,
    /// Whether the runtime marked this turn as submitted while the agent was
    /// working. Only meaningful on an [`Authorship::Authored`] turn.
    pub continues_submission: bool,
    /// The commit a tool result on this record reported.
    pub commit: Option<String>,
    /// The file a tool result on this record reported editing.
    pub edited_file: Option<PathBuf>,
}

/// An assistant turn, reduced to the actions it took.
#[derive(Debug, Clone)]
pub struct AssistantTurn {
    pub citation: Citation,
    pub actions: Vec<ToolAction>,
}

#[derive(Debug, Clone)]
pub enum Record {
    User(UserTurn),
    Assistant(AssistantTurn),
}

impl Record {
    pub fn citation(&self) -> &Citation {
        match self {
            Self::User(u) => &u.citation,
            Self::Assistant(a) => &a.citation,
        }
    }
}

/// What the reader understood, and what it did not.
///
/// Published with every result. A report that cannot say what fraction of its
/// input it read is not evidence, and the failure this guards against is the
/// flattering one: a pass that opened nothing reports zero of everything and
/// looks calm doing it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Coverage {
    pub files_discovered: usize,
    pub files_read: usize,
    pub files_unreadable: usize,
    pub records_total: usize,
    pub records_malformed: usize,
    /// Record types present in the input that this module does not consume,
    /// by name. Growth here is the signal that the vocabulary moved.
    pub record_types_unconsumed: BTreeMap<String, usize>,
    /// User turns by [`Authorship`], keyed by its stable string.
    pub user_turns_by_authorship: BTreeMap<String, usize>,
    /// Runtime versions observed across the input.
    pub runtime_versions: BTreeSet<String>,
}

impl Coverage {
    /// Share of turns the runtime attributed to a person that this module could
    /// also attribute to a prompt source.
    ///
    /// `None` when the runtime claimed no person anywhere in the input — a
    /// ratio over an empty population is not 100%, and reporting it as such is
    /// the flattering failure this type exists to prevent.
    pub fn authorship_ratio(&self) -> Option<f64> {
        let claimed: usize = Authorship::ALL
            .iter()
            .filter(|a| a.claims_a_person())
            .filter_map(|a| self.user_turns_by_authorship.get(a.as_str()))
            .sum();
        if claimed == 0 {
            return None;
        }
        let authored = self
            .user_turns_by_authorship
            .get(Authorship::Authored.as_str())
            .copied()
            .unwrap_or(0);
        Some(authored as f64 / claimed as f64)
    }

    fn count_authorship(&mut self, a: Authorship) {
        *self
            .user_turns_by_authorship
            .entry(a.as_str().to_string())
            .or_default() += 1;
    }
}

#[derive(Deserialize)]
struct RawOrigin {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    content: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawRecord {
    #[serde(rename = "type")]
    kind: Option<String>,
    uuid: Option<String>,
    timestamp: Option<Timestamp>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    version: Option<String>,
    origin: Option<RawOrigin>,
    #[serde(rename = "promptSource")]
    prompt_source: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<RawMessage>,
    /// Polymorphic upstream: an object for most tools, a bare string for some.
    /// Modelled as an untyped value so a string never fails the whole record.
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
}

/// The commit a tool result reported, if it reported one.
fn commit_of(result: &serde_json::Value) -> Option<String> {
    Some(
        result
            .get("gitOperation")?
            .get("commit")?
            .get("sha")?
            .as_str()?
            .to_string(),
    )
}

/// The file a tool result reported editing.
///
/// `structuredPatch` is required alongside the path: a result naming a file it
/// only read carries no patch, and counting a read as an edit would put every
/// inspection into the rework figures.
fn edited_file_of(result: &serde_json::Value) -> Option<PathBuf> {
    result.get("structuredPatch")?;
    Some(PathBuf::from(result.get("filePath")?.as_str()?))
}

/// Text of a message whose content is text and nothing else.
///
/// A message carrying a tool result, an image or any other block is not a turn
/// the operator wrote, so it yields `None` rather than a partial join — a
/// partial join would put tool output into prompt statistics.
fn text_only(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let mut out = Vec::with_capacity(blocks.len());
            for b in blocks {
                match b.get("type").and_then(serde_json::Value::as_str) {
                    Some("text") => out.push(b.get("text")?.as_str()?.to_string()),
                    _ => return None,
                }
            }
            Some(out.join("\n"))
        }
        _ => None,
    }
}

fn tool_actions(content: &serde_json::Value) -> Vec<ToolAction> {
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("tool_use"))
        .filter_map(|b| {
            Some(ToolAction {
                tool: b.get("name")?.as_str()?.to_string(),
                input: b.get("input").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect()
}

fn classify(raw: &RawRecord) -> Authorship {
    if raw.is_sidechain.unwrap_or(false) {
        return Authorship::Sidechain;
    }
    let Some(kind) = raw.origin.as_ref().and_then(|o| o.kind.as_deref()) else {
        return Authorship::Unclaimed;
    };
    if kind != "human" {
        return Authorship::Attributed;
    }
    match raw.prompt_source.as_deref() {
        Some(s) if AUTHORED_PROMPT_SOURCES.contains(&s) => Authorship::Authored,
        _ => Authorship::SourceUnrecognised,
    }
}

/// Read one transcript, appending what it understood into `coverage`.
///
/// A malformed line, a record without the identity a citation needs, and a
/// record type this module does not consume are each counted rather than
/// raised: the file is still readable and the rest of it still counts. Only an
/// unreadable file is the caller's problem, and that is signalled by the `Err`.
pub fn read_transcript(path: &Path, coverage: &mut Coverage) -> std::io::Result<Vec<Record>> {
    use std::io::BufRead;

    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        coverage.records_total += 1;

        let Ok(raw) = serde_json::from_str::<RawRecord>(&line) else {
            coverage.records_malformed += 1;
            continue;
        };
        if let Some(v) = &raw.version {
            coverage.runtime_versions.insert(v.clone());
        }
        let Some(kind) = raw.kind.as_deref() else {
            coverage.records_malformed += 1;
            continue;
        };
        let Some(consumed) = ConsumedType::from_str(kind) else {
            *coverage
                .record_types_unconsumed
                .entry(kind.to_string())
                .or_default() += 1;
            continue;
        };
        let (Some(uuid), Some(timestamp), Some(session)) =
            (&raw.uuid, raw.timestamp, &raw.session_id)
        else {
            coverage.records_malformed += 1;
            continue;
        };
        let citation = Citation {
            session: session.clone(),
            file: path.to_path_buf(),
            uuid: uuid.clone(),
            timestamp,
        };
        let content = raw.message.as_ref().and_then(|m| m.content.as_ref());

        match consumed {
            ConsumedType::User => {
                let authorship = classify(&raw);
                coverage.count_authorship(authorship);
                let result = raw.tool_use_result.as_ref();
                out.push(Record::User(UserTurn {
                    citation,
                    authorship,
                    text: content.and_then(text_only),
                    continues_submission: raw.prompt_source.as_deref()
                        == Some(CONTINUATION_PROMPT_SOURCE),
                    commit: result.and_then(commit_of),
                    edited_file: result.and_then(edited_file_of),
                }));
            }
            ConsumedType::Assistant => {
                out.push(Record::Assistant(AssistantTurn {
                    citation,
                    actions: content.map(tool_actions).unwrap_or_default(),
                }));
            }
        }
    }
    coverage.files_read += 1;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(json: &str) -> (Vec<Record>, Coverage) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, json).unwrap();
        let mut cov = Coverage::default();
        let recs = read_transcript(&path, &mut cov).unwrap();
        (recs, cov)
    }

    const BASE: &str =
        r#""uuid":"u1","timestamp":"2026-08-26T00:00:00Z","sessionId":"s1","version":"2.1.246""#;

    #[test]
    fn typed_human_turn_is_authored() {
        let (recs, cov) = rec(&format!(
            r#"{{"type":"user",{BASE},"origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":"hi"}}}}"#
        ));
        assert_eq!(recs.len(), 1);
        match &recs[0] {
            Record::User(u) => {
                assert_eq!(u.authorship, Authorship::Authored);
                assert_eq!(u.text.as_deref(), Some("hi"));
            }
            _ => panic!("expected a user turn"),
        }
        assert_eq!(cov.authorship_ratio(), Some(1.0));
    }

    #[test]
    fn unknown_prompt_source_never_counts_as_authored() {
        let (_, cov) = rec(&format!(
            r#"{{"type":"user",{BASE},"origin":{{"kind":"human"}},"promptSource":"a-source-shipped-tomorrow","message":{{"content":"hi"}}}}"#
        ));
        assert_eq!(
            cov.user_turns_by_authorship
                .get(Authorship::SourceUnrecognised.as_str()),
            Some(&1)
        );
        assert_eq!(cov.authorship_ratio(), Some(0.0));
    }

    #[test]
    fn absent_origin_is_unclaimed_and_outside_the_ratio() {
        let (_, cov) = rec(&format!(
            r#"{{"type":"user",{BASE},"message":{{"content":"[Request interrupted by user]"}}}}"#
        ));
        assert_eq!(
            cov.user_turns_by_authorship
                .get(Authorship::Unclaimed.as_str()),
            Some(&1)
        );
        assert_eq!(cov.authorship_ratio(), None);
    }

    #[test]
    fn tool_result_content_yields_no_turn_text() {
        let (recs, _) = rec(&format!(
            r#"{{"type":"user",{BASE},"origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":[{{"type":"tool_result","content":"out"}}]}}}}"#
        ));
        match &recs[0] {
            Record::User(u) => assert!(u.text.is_none()),
            _ => panic!("expected a user turn"),
        }
    }

    #[test]
    fn unconsumed_record_type_is_counted_by_name_not_dropped() {
        let (recs, cov) = rec(
            r#"{"type":"artifact-comment-monitor","uuid":"u","timestamp":"2026-08-26T00:00:00Z","sessionId":"s"}"#,
        );
        assert!(recs.is_empty());
        assert_eq!(
            cov.record_types_unconsumed.get("artifact-comment-monitor"),
            Some(&1)
        );
        assert_eq!(cov.records_malformed, 0);
    }

    #[test]
    fn malformed_line_does_not_abort_the_file() {
        let (recs, cov) = rec(&format!(
            "not json\n{{\"type\":\"user\",{BASE},\"origin\":{{\"kind\":\"human\"}},\"promptSource\":\"typed\",\"message\":{{\"content\":\"hi\"}}}}\n"
        ));
        assert_eq!(recs.len(), 1);
        assert_eq!(cov.records_malformed, 1);
        assert_eq!(cov.records_total, 2);
    }

    #[test]
    fn same_action_compares_whole_input_without_naming_a_field() {
        let a = ToolAction {
            tool: "Bash".into(),
            input: serde_json::json!({"command": "ls", "timeout": 5}),
        };
        let b = ToolAction {
            tool: "Bash".into(),
            input: serde_json::json!({"timeout": 5, "command": "ls"}),
        };
        let c = ToolAction {
            tool: "Bash".into(),
            input: serde_json::json!({"command": "ls -l"}),
        };
        assert!(a.same_action(&b));
        assert!(!a.same_action(&c));
    }

    #[test]
    fn assistant_actions_carry_tool_names() {
        let (recs, _) = rec(&format!(
            r#"{{"type":"assistant",{BASE},"message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"ls"}}}}]}}}}"#
        ));
        match &recs[0] {
            Record::Assistant(a) => assert_eq!(a.actions[0].tool, "Bash"),
            _ => panic!("expected an assistant turn"),
        }
    }
}
