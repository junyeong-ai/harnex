//! # record — the transcript shapes this module consumes, and what it could not read
//!
//! A Claude Code transcript is JSONL whose vocabulary is undocumented and
//! moves: the local corpus spans 21 runtime versions and 23 record `type`
//! values this binary does not read, several of which arrived recently.
//! Modelling that vocabulary as a closed enum would make a future record type a
//! parse failure, and a parse failure that is skipped reads as a shorter
//! session — a *better* number, arrived at silently. So only the types this
//! module consumes are closed; every other value is counted into [`Coverage`]
//! under its own name, and the two types with a sub-vocabulary are counted
//! under a qualified one (`attachment:hook_success`) so consuming one member
//! does not hide the growth of the rest.
//!
//! [`Authorship`] is the one classification made here, and it is made from the
//! runtime's own attribution rather than from the text. `origin.kind` is the
//! runtime's answer to "whose turn is this"; where it is absent the runtime
//! made no claim, and neither does this module. Reconstructing authorship from
//! phrasing would put a word list in source that tomorrow's phrasing is not in.
//!
//! A denied tool call is attributed the same way. The runtime writes its reason
//! into the result's message text, which would have to be pattern-matched; it
//! also writes `tool_use_id`, which resolves to the call that was denied
//! through the ordinary Anthropic tool protocol. Measured over 1,085 denials,
//! that link resolves 100% of them, so the text is never read.
//!
//! ## What this module refuses to do
//!
//! - Never infer authorship, or what was denied, from message text. An
//!   unrecognised prompt source classifies as
//!   [`Authorship::SourceUnrecognised`], never as authored, so an upstream
//!   addition lowers coverage instead of entering the statistics.
//! - Never model the full record vocabulary. Unconsumed types and subtypes are
//!   counted, not rejected and not dropped.
//! - Never carry prompt text or tool input into a serialised shape the caller
//!   did not ask for. Text is held for analysis and released; the one call
//!   input that survives its record rides [`Denial`], because a refusal is
//!   only legible as the thing that was refused.
//! - Never treat "read nothing" as "found nothing". A discovered file that
//!   cannot be opened is counted, and the caller is told.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::wire_enum::wire_enum;
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
///
/// It says the operator did not wait, not what they meant by it — see
/// [`UserTurn::follows_agent_output`].
const QUEUED_PROMPT_SOURCE: &str = "queued";

/// The attachment carrying a project memory file that entered context.
const RULE_LOAD_ATTACHMENT: &str = "nested_memory";

/// The system record carrying one Stop event's hook accounting.
const STOP_SUMMARY_SUBTYPE: &str = "stop_hook_summary";

/// The system record marking where the session's context was compacted.
const COMPACT_BOUNDARY_SUBTYPE: &str = "compact_boundary";

/// The slash command that compacts a session, and the tags its record wraps the
/// operator's arguments in.
const COMPACT_COMMAND: &str = "<command-name>/compact</command-name>";
const COMMAND_ARGS_OPEN: &str = "<command-args>";
const COMMAND_ARGS_CLOSE: &str = "</command-args>";

/// What the operator asked a compaction to keep, from one turn.
///
/// The runtime writes the command into a record of its own and the operator
/// never types that wrapper, so a turn it attributes to the operator quoting the
/// tag — a prompt about this very feature — is their prose and not a command.
/// Both conditions are the measured shape: over the local corpus all 373 command
/// records open with the tag and none carries a prompt source.
///
/// `None` where the turn is not a compact command. `Some("")` where it is one
/// given no arguments — the operator compacting without saying what to keep,
/// which is a different answer from the runtime compacting on its own.
pub(crate) fn compact_instruction(turn: &UserTurn) -> Option<String> {
    if turn.authorship == Authorship::Authored {
        return None;
    }
    let text = turn.text.as_deref()?;
    if !text.trim_start().starts_with(COMPACT_COMMAND) {
        return None;
    }
    let Some((_, rest)) = text.split_once(COMMAND_ARGS_OPEN) else {
        return Some(String::new());
    };
    let args = rest.split_once(COMMAND_ARGS_CLOSE).map_or(rest, |(a, _)| a);
    Some(args.trim().to_string())
}

/// Tools that invoke a harness element, and the input key naming it.
///
/// The only per-tool argument vocabulary this module admits, and it is admitted
/// because the value is an element's own name rather than anything the operator
/// wrote — which is what lets it be reported plainly. `Task` and `Agent` both
/// appear because the runtime renamed the tool and the corpus spans both.
/// Slash commands are absent: their `command` carries arguments as well as a
/// name, so it is operator text and not an element.
const ASSET_TOOL_KEYS: &[(&str, &str, &str)] = &[
    ("Skill", "skill", "skill"),
    ("Task", "subagent_type", "agent"),
    ("Agent", "subagent_type", "agent"),
];

/// A harness element a tool call invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssetCall {
    /// `skill` or `agent`, unified across the tool rename.
    pub kind: String,
    pub name: String,
}

fn asset_of(tool: &str, input: &serde_json::Value) -> Option<AssetCall> {
    let (key, kind) = ASSET_TOOL_KEYS
        .iter()
        .find_map(|(t, key, kind)| (*t == tool).then_some((*key, *kind)))?;
    Some(AssetCall {
        kind: kind.to_string(),
        name: input.get(key)?.as_str()?.to_string(),
    })
}

/// Record types this module reads. Everything else is counted by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsumedType {
    User,
    Assistant,
    Attachment,
    System,
}

impl ConsumedType {
    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "attachment" => Self::Attachment,
            "system" => Self::System,
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

wire_enum! {
    /// The runtime's attribution of a user turn, as recorded.
    ///
    /// Only [`Authorship::Authored`] enters prompt statistics. The rest are
    /// reported separately so a consumer sees what was set aside and why.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Authorship {
        /// `origin.kind == "human"` with a prompt source the operator types.
        Authored => "authored",
        /// `origin.kind == "human"` with a prompt source outside
        /// [`AUTHORED_PROMPT_SOURCES`], including an absent one. The runtime says a
        /// person is behind the turn but not that they wrote its text.
        SourceUnrecognised => "source-unrecognised",
        /// `origin.kind` present and not `human` — the runtime attributed the turn
        /// to something other than the operator.
        Attributed => "attributed",
        /// A subagent transcript turn.
        Sidechain => "sidechain",
        /// No `origin.kind`. The runtime made no authorship claim, so neither does
        /// this module. Measured at roughly 45% of text-bearing user records across
        /// every runtime version in the local corpus — it is the ordinary shape of
        /// interrupts, resumptions and injected context, not a defect.
        Unclaimed => "unclaimed",
    }
}

impl Authorship {
    /// Whether the runtime claimed a person was behind this turn.
    ///
    /// Two questions are answered from one classification and only this one
    /// decides the instruction boundary: whether the operator *wrote* the text
    /// is [`Self::Authored`], and it governs what the repetition statistics
    /// read. A prompt the operator chose rather than typed is still an
    /// instruction the agent worked under, and dropping it would attribute its
    /// turns, tokens and commits to whatever came before.
    ///
    /// The coverage ratio is taken over this population: turns the runtime
    /// never claimed are outside the question, not failures to answer it.
    pub fn claims_a_person(self) -> bool {
        matches!(self, Self::Authored | Self::SourceUnrecognised)
    }
}

/// A tool call the runtime refused to run.
///
/// `kind` stays a string. The runtime's two observed values separate the
/// harness refusing (`permission-rule`) from the operator refusing
/// (`user-rejected`), and a third value shipped upstream should appear as its
/// own row rather than be dropped by an enum that has not heard of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub kind: String,
    /// The tool named by the call this denial answered, resolved through
    /// `tool_use_id`. `None` when the call is not in the same transcript.
    pub tool: Option<String>,
    /// The refused call's own input, which is what makes two refusals the same
    /// refusal without this module deciding which of a tool's arguments
    /// matters. Operator-written, so it is grouped on always and reported only
    /// when the caller asks for text.
    pub input: Option<serde_json::Value>,
}

/// What a tool did: how often it was called, and how often a call came back an
/// error.
///
/// A refused call is not a failure — it never ran, and it is grouped in
/// `harness.denials` instead. The runtime marks a refusal with its own field
/// and marks the result an error as well, so counting the error flag alone
/// would report the harness's refusals as the tool's failures: measured over
/// one corpus, 528 of 1,944 flagged results are refusals, and no refusal
/// arrives without the flag.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct ToolUse {
    pub calls: usize,
    /// A floor: a result whose call is recorded in another transcript cannot be
    /// attributed to a tool and is not counted here — 1 of 7,194 over the local
    /// corpus.
    pub failed: usize,
}

/// One tool invocation, reduced to what this module reads.
///
/// The arguments do not survive the record: a Bash input carries the
/// operator's command text, and what a call did is read from the tool it
/// named. [`ASSET_TOOL_KEYS`] is the one place a tool's argument vocabulary
/// enters this crate, and it is one because a harness element's name is not
/// operator text.
#[derive(Debug, Clone)]
pub struct ToolAction {
    pub tool: String,
    /// The harness element this call invoked, if it invoked one.
    pub asset: Option<AssetCall>,
}

/// A user turn, reduced to what this module reads.
#[derive(Debug, Clone)]
pub struct UserTurn {
    pub citation: Citation,
    pub authorship: Authorship,
    /// What the person wrote, joined from the message's text blocks. `None`
    /// when there are none: a tool result is a tool result, not a turn.
    pub text: Option<String>,
    /// Whether the runtime marked this turn as submitted while the agent was
    /// working. Only meaningful on an [`Authorship::Authored`] turn.
    pub queued: bool,
    /// Whether the agent produced anything since the previous user turn.
    ///
    /// The two readings of [`UserTurn::queued`] separate here and nowhere
    /// else. A queued turn with nothing produced yet is the operator still
    /// composing; a queued turn after the agent has spoken is the operator
    /// reacting to what it did. Measured across the local corpus the split is
    /// 430 to 437, so treating every queued turn as one instruction folds half
    /// of them into an instruction they were arguing with.
    pub follows_agent_output: bool,
    /// Whether the runtime recorded this turn as the point an agent turn was
    /// cut short.
    ///
    /// A floor, not a count: measured against the runtime's own wording over
    /// the local corpus the field is present on 216 of 394 interruptions
    /// (54.8%). Reading it as the number of interruptions understates them.
    pub interrupted: bool,
    /// The commit a tool result on this record reported.
    ///
    /// A floor. The runtime attaches `gitOperation` to some commits and not
    /// others — measured against one project's history, 29 of git's 42 over
    /// the span it recorded any at all. Anything denominated in observed
    /// commits therefore reads high, and `repository.authored_in_span` is what
    /// makes the gap visible.
    pub commit: Option<String>,
    /// The file a tool result on this record reported editing.
    pub edited_file: Option<PathBuf>,
    /// The refusal a tool result on this record reported.
    pub denial: Option<Denial>,
    /// The tool of a call that ran and came back an error, resolved through
    /// `tool_use_id`. `None` where the call was refused instead — a refusal is
    /// [`UserTurn::denial`] and never both.
    pub failed_tool: Option<String>,
}

/// An assistant turn, reduced to the actions it took.
#[derive(Debug, Clone)]
pub struct AssistantTurn {
    pub citation: Citation,
    pub actions: Vec<ToolAction>,
    pub tokens: TokenUse,
    /// The message this turn is a block of. A message is one charge however
    /// many records report it, and the runtime writes those records into more
    /// than one file.
    pub message: Option<String>,
    /// The model that produced this turn, as the runtime named it.
    pub model: Option<String>,
    /// Whether a subagent took it. A subagent's transcript carries its
    /// parent's session id, so nothing else separates the two threads, and
    /// they do not share a context: a compaction of the parent leaves a
    /// running subagent's own window untouched.
    pub sidechain: bool,
}

/// Where a session's context was compacted, and what it cost.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Compaction {
    pub citation: Citation,
    /// The runtime's own word for why. `manual` and `auto` over the local
    /// corpus, kept as a string because the vocabulary is upstream.
    pub trigger: String,
    pub pre_tokens: u64,
    pub post_tokens: u64,
    /// The session's running total, not this event's drop. It equals
    /// `pre_tokens - post_tokens` only on a session's first compaction — 52 of
    /// 53 measured — and rises monotonically after, so summing it across a
    /// session counts the same tokens again at every boundary.
    pub cumulative_dropped_tokens: u64,
    pub duration_ms: u64,
    /// How much the operator asked the compaction to keep, in characters.
    /// `None` where no `/compact` preceded the boundary — the runtime compacted
    /// on its own. `Some(0)` where the operator compacted and asked for nothing.
    pub instruction_chars: Option<usize>,
    /// What they asked, verbatim. Operator text, so it is withheld unless the
    /// caller asked for text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

/// What one turn, instruction or window spent.
///
/// Four counts and no total: they price differently and this module does not
/// know by how much. A single number would need a price list that is neither
/// ours nor stable, so the reader gets the counts and money is never named.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenUse {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl TokenUse {
    pub fn add(&mut self, other: TokenUse) {
        self.input += other.input;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.output += other.output;
    }
}

/// A project memory file that entered context.
#[derive(Debug, Clone)]
pub struct RuleLoad {
    pub citation: Citation,
    pub path: PathBuf,
    /// Characters of the file as it entered context.
    pub chars: usize,
}

/// One hook run inside a Stop event.
#[derive(Debug, Clone)]
pub struct HookRun {
    pub command: String,
    pub duration_ms: u64,
}

/// One Stop event's hook accounting.
#[derive(Debug, Clone)]
pub struct StopSummary {
    pub citation: Citation,
    pub hooks: Vec<HookRun>,
    pub errors: usize,
    /// Whether these hooks kept the agent from stopping.
    pub prevented_continuation: bool,
}

#[derive(Debug, Clone)]
pub enum Record {
    User(UserTurn),
    Assistant(AssistantTurn),
    RuleLoad(RuleLoad),
    StopSummary(StopSummary),
    Compaction(Compaction),
}

impl Record {
    pub fn citation(&self) -> &Citation {
        match self {
            Self::User(u) => &u.citation,
            Self::Assistant(a) => &a.citation,
            Self::RuleLoad(r) => &r.citation,
            Self::StopSummary(s) => &s.citation,
            Self::Compaction(c) => &c.citation,
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
    /// Transcripts under the configured roots. The corpus, not the window: a
    /// scoped run opens every one of them and answers about the few that
    /// match, which is what it costs and not what it measured.
    pub files_discovered: usize,
    pub files_read: usize,
    pub files_unreadable: usize,
    /// Of those, the ones a record in the window came from. The file count
    /// that belongs beside `records_total`.
    pub files_in_window: usize,
    pub records_total: usize,
    pub records_malformed: usize,
    /// Records the runtime carried in from the session this one was forked
    /// from. The work they describe happened there and is counted there, so a
    /// window that reads both files counts it once — measured over one corpus,
    /// 54,965 such records, every one of which is also in the file it came
    /// from.
    ///
    /// Zero on a baseline whose `oracle_version` predates this counter, which
    /// is what that field is for.
    pub records_forked: usize,
    /// Records already counted from another of the same session's transcripts.
    /// Dispatching several subagents at once copies the state they start from
    /// into each of their files, and one event is counted once.
    pub records_duplicated: usize,
    /// Turns that reported what they spent without naming the message they
    /// belong to, so the charge could not be held against its message and was
    /// taken as new. Zero across every record of the local corpus; a non-zero
    /// here says the shape moved and the token counts are a ceiling.
    pub turns_charged_without_a_message: usize,
    /// Record kinds present in the input that this module does not consume.
    /// A type with a sub-vocabulary is keyed `type:subtype`, so consuming one
    /// member still leaves the growth of its siblings visible.
    pub record_types_unconsumed: BTreeMap<String, usize>,
    /// User turns by [`Authorship`], keyed by its stable string.
    pub user_turns_by_authorship: BTreeMap<String, usize>,
    /// Runtime versions observed across the input.
    pub runtime_versions: BTreeSet<String>,
    /// Models observed across the input. A window whose model mix moved is a
    /// window whose token counts moved for a reason that is not the operator.
    pub models: BTreeSet<String>,
    /// Sessions the window drew from. The denominator for anything about how
    /// work was split up rather than how it was instructed.
    pub sessions: usize,
    /// Timestamp of the earliest record counted, and of the latest.
    ///
    /// The span the numbers describe, which is not the span the caller asked
    /// for: a window opened before the corpus starts, or closed after the last
    /// session, still reports what it actually saw.
    pub observed_from: Option<Timestamp>,
    pub observed_to: Option<Timestamp>,
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

    fn observe_time(&mut self, t: Timestamp) {
        if self.observed_from.is_none_or(|from| t < from) {
            self.observed_from = Some(t);
        }
        if self.observed_to.is_none_or(|to| t > to) {
            self.observed_to = Some(t);
        }
    }

    fn count_authorship(&mut self, a: Authorship) {
        *self
            .user_turns_by_authorship
            .entry(a.as_str().to_string())
            .or_default() += 1;
    }

    fn count_unconsumed(&mut self, key: String) {
        *self.record_types_unconsumed.entry(key).or_default() += 1;
    }
}

#[derive(Deserialize)]
struct RawCompactMetadata {
    trigger: Option<String>,
    #[serde(rename = "preTokens")]
    pre_tokens: Option<u64>,
    #[serde(rename = "postTokens")]
    post_tokens: Option<u64>,
    #[serde(rename = "cumulativeDroppedTokens")]
    cumulative_dropped_tokens: Option<u64>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
}

#[derive(Deserialize)]
struct RawOrigin {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: Option<String>,
    content: Option<serde_json::Value>,
    model: Option<String>,
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl RawUsage {
    fn tokens(&self) -> TokenUse {
        TokenUse {
            input: self.input_tokens.unwrap_or_default(),
            cache_creation: self.cache_creation_input_tokens.unwrap_or_default(),
            cache_read: self.cache_read_input_tokens.unwrap_or_default(),
            output: self.output_tokens.unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
struct RawHookInfo {
    command: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
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
    /// Set by the runtime on a record it replayed out of the session this one
    /// was forked from. Its contents name that session; its presence is what
    /// says the record is not this session's event.
    #[serde(rename = "forkedFrom")]
    forked_from: Option<serde_json::Value>,
    message: Option<RawMessage>,
    /// Polymorphic upstream: an object for most tools, a bare string for some.
    /// Modelled as an untyped value so a string never fails the whole record.
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
    #[serde(rename = "toolDenialKind")]
    tool_denial_kind: Option<String>,
    attachment: Option<serde_json::Value>,
    subtype: Option<String>,
    #[serde(rename = "hookInfos")]
    hook_infos: Option<Vec<RawHookInfo>>,
    #[serde(rename = "hookErrors")]
    hook_errors: Option<Vec<serde_json::Value>>,
    #[serde(rename = "preventedContinuation")]
    prevented_continuation: Option<bool>,
    #[serde(rename = "interruptedMessageId")]
    interrupted_message_id: Option<String>,
    #[serde(rename = "compactMetadata")]
    compact_metadata: Option<RawCompactMetadata>,
    cwd: Option<PathBuf>,
}

/// Whether a result block reported the call it answers as an error.
fn errored(content: &serde_json::Value) -> bool {
    blocks_of(Some(content)).iter().any(|b| {
        b.get("type").and_then(serde_json::Value::as_str) == Some("tool_result")
            && b.get("is_error").and_then(serde_json::Value::as_bool) == Some(true)
    })
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
/// What a person wrote in this message, if they wrote anything.
///
/// The text blocks, joined; `None` when there are none. A tool result carries
/// `tool_result` blocks and no text, so it still yields nothing — which is the
/// separation this is for. Requiring every block to be text would have dropped
/// the whole turn over an attachment beside it.
fn text_of(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let out: Vec<String> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .filter_map(|b| Some(b.get("text")?.as_str()?.to_string()))
                .collect();
            (!out.is_empty()).then(|| out.join("\n"))
        }
        _ => None,
    }
}

fn blocks_of(content: Option<&serde_json::Value>) -> &[serde_json::Value] {
    content
        .and_then(serde_json::Value::as_array)
        .map_or(&[], |v| v)
}

/// `tool_use_id` of the first tool result in a message.
fn tool_use_id_of(content: Option<&serde_json::Value>) -> Option<&str> {
    blocks_of(content)
        .iter()
        .find(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("tool_result"))?
        .get("tool_use_id")?
        .as_str()
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

/// Which records of a transcript are in the window.
///
/// Every filter here is applied per record rather than per file, because a
/// file is not the unit any of them names: `since` cuts inside a transcript
/// that spans the boundary, a worktree puts records of two projects in one
/// file, and a subagent's transcript is a separate file carrying its parent's
/// session. That last one is why `session` reads the record's own id — the
/// subagent's work comes with the parent it belongs to, and no filename rule
/// would find it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Window<'a> {
    pub since: Option<Timestamp>,
    pub project: Option<&'a Path>,
    pub session: Option<&'a str>,
}

/// Read one transcript, appending what it understood into `coverage`.
///
/// A malformed line, a record without the identity a citation needs, and a
/// record kind this module does not consume are each counted rather than
/// raised: the file is still readable and the rest of it still counts. Only an
/// unreadable file is the caller's problem, and that is signalled by the `Err`.
pub fn read_transcript(
    path: &Path,
    window: Window<'_>,
    coverage: &mut Coverage,
) -> std::io::Result<Vec<Record>> {
    use std::io::BufRead;

    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = Vec::new();
    // A tool call always precedes its result inside one transcript, so a single
    // forward pass resolves every denial to the tool it answered.
    let mut tool_calls: HashMap<String, (String, serde_json::Value)> = HashMap::new();
    // Which record currently carries each assistant message's spend. The
    // runtime writes one record per content block and repeats the message's
    // usage on every one, so the charge belongs to the message and is held by
    // exactly one of its records.
    let mut charged: HashMap<String, usize> = HashMap::new();
    let mut agent_output_since_user_turn = false;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawRecord>(&line) else {
            coverage.records_total += 1;
            coverage.records_malformed += 1;
            continue;
        };
        // Coverage counts the window, not the file: the ratio it publishes is
        // the one `require_coverage` gates on. A record too damaged to carry a
        // timestamp cannot be placed in time and is counted here rather than
        // dropped, which overstates the damage in the window and never hides it.
        if window
            .since
            .is_some_and(|s| raw.timestamp.is_some_and(|t| t < s))
        {
            continue;
        }
        // A worktree runs under a directory below the project it belongs to,
        // so containment rather than equality is what places a record. Every
        // record type this module consumes carries `cwd`; one that does not is
        // outside a project window rather than in every one.
        if let Some(project) = window.project {
            match &raw.cwd {
                Some(cwd) if cwd.starts_with(project) => {}
                _ => continue,
            }
        }
        if let Some(session) = window.session {
            match &raw.session_id {
                Some(id) if id == session => {}
                _ => continue,
            }
        }
        coverage.records_total += 1;
        // A fork's transcript replays the conversation it was forked from so
        // the session stands alone. Those records are the earlier session's
        // events, carrying its uuids and its timestamps, and counting them
        // again here would report one instruction as two.
        if raw.forked_from.is_some() {
            coverage.records_forked += 1;
            continue;
        }
        if let Some(t) = raw.timestamp {
            coverage.observe_time(t);
        }
        if let Some(v) = &raw.version {
            coverage.runtime_versions.insert(v.clone());
        }
        let Some(kind) = raw.kind.as_deref() else {
            coverage.records_malformed += 1;
            continue;
        };
        let Some(consumed) = ConsumedType::from_str(kind) else {
            coverage.count_unconsumed(kind.to_string());
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
                let text = content.and_then(text_of);
                // A tool result is a `user` record carrying no turn. Counting
                // one as a turn the runtime declined to attribute describes the
                // protocol rather than the operator — measured over one
                // project, 3,751 of the 3,859 records that would land in
                // `unclaimed` are tool results.
                if text.is_some() {
                    coverage.count_authorship(authorship);
                }
                let result = raw.tool_use_result.as_ref();
                let denial = raw.tool_denial_kind.as_ref().map(|k| {
                    let call = tool_use_id_of(content).and_then(|id| tool_calls.get(id));
                    Denial {
                        kind: k.clone(),
                        tool: call.map(|(tool, _)| tool.clone()),
                        input: call.map(|(_, input)| input.clone()),
                    }
                });
                let failed_tool = (raw.tool_denial_kind.is_none() && content.is_some_and(errored))
                    .then(|| {
                        tool_use_id_of(content)
                            .and_then(|id| tool_calls.get(id))
                            .map(|(tool, _)| tool.clone())
                    })
                    .flatten();
                out.push(Record::User(UserTurn {
                    citation,
                    authorship,
                    text,
                    queued: raw.prompt_source.as_deref() == Some(QUEUED_PROMPT_SOURCE),
                    follows_agent_output: agent_output_since_user_turn,
                    interrupted: raw.interrupted_message_id.is_some(),
                    commit: result.and_then(commit_of),
                    edited_file: result.and_then(edited_file_of),
                    denial,
                    failed_tool,
                }));
                // Only a turn the runtime attributed to a person closes the
                // interval. A tool result is also a `user` record, and letting
                // one reset this would report the operator as having spoken
                // when the protocol did.
                if authorship.claims_a_person() {
                    agent_output_since_user_turn = false;
                }
            }
            ConsumedType::Assistant => {
                let mut actions = Vec::new();
                for block in blocks_of(content) {
                    if block.get("type").and_then(serde_json::Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let Some(tool) = block.get("name").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    actions.push(ToolAction {
                        tool: tool.to_string(),
                        asset: asset_of(tool, &input),
                    });
                    if let Some(id) = block.get("id").and_then(serde_json::Value::as_str) {
                        tool_calls.insert(id.to_string(), (tool.to_string(), input));
                    }
                }
                agent_output_since_user_turn = true;
                let message = raw.message.as_ref();
                let model = message.and_then(|m| m.model.clone());
                if let Some(model) = &model {
                    coverage.models.insert(model.clone());
                }
                let tokens = message
                    .and_then(|m| m.usage.as_ref())
                    .map(RawUsage::tokens)
                    .unwrap_or_default();
                let spent = message.and_then(|m| m.usage.as_ref()).is_some();
                let named = message.and_then(|m| m.id.clone());
                out.push(Record::Assistant(AssistantTurn {
                    citation,
                    actions,
                    tokens,
                    message: named,
                    model,
                    sidechain: raw.is_sidechain.unwrap_or(false),
                }));
                // The latest record of a message holds its charge: while a
                // message is still being written its earlier records report a
                // partial output count and the last one reports the settled
                // one. Clearing the record that held it before keeps the
                // message counted once at its final value.
                match message.and_then(|m| m.id.as_deref()) {
                    Some(id) => {
                        if let Some(previous) = charged.insert(id.to_string(), out.len() - 1)
                            && let Record::Assistant(turn) = &mut out[previous]
                        {
                            turn.tokens = TokenUse::default();
                        }
                    }
                    // Charging it is the only answer left — dropping it would
                    // undercount — and a charge nothing can be held against is
                    // the shape that made this counter necessary, so it is
                    // reported rather than assumed away.
                    None if spent => coverage.turns_charged_without_a_message += 1,
                    None => {}
                }
            }
            ConsumedType::Attachment => {
                let attachment = raw.attachment.as_ref();
                let subtype = attachment
                    .and_then(|a| a.get("type"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if subtype != RULE_LOAD_ATTACHMENT {
                    coverage.count_unconsumed(format!("{kind}:{subtype}"));
                    continue;
                }
                let Some(loaded) = attachment
                    .and_then(|a| a.get("path"))
                    .and_then(serde_json::Value::as_str)
                else {
                    coverage.records_malformed += 1;
                    continue;
                };
                let chars = attachment
                    .and_then(|a| a.get("content"))
                    .and_then(|c| c.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .map(|s| s.chars().count())
                    .unwrap_or(0);
                out.push(Record::RuleLoad(RuleLoad {
                    citation,
                    path: PathBuf::from(loaded),
                    chars,
                }));
            }
            ConsumedType::System => {
                let subtype = raw.subtype.as_deref().unwrap_or_default();
                if subtype == COMPACT_BOUNDARY_SUBTYPE {
                    let Some(meta) = raw.compact_metadata else {
                        coverage.records_malformed += 1;
                        continue;
                    };
                    out.push(Record::Compaction(Compaction {
                        citation,
                        trigger: meta.trigger.unwrap_or_default(),
                        pre_tokens: meta.pre_tokens.unwrap_or_default(),
                        post_tokens: meta.post_tokens.unwrap_or_default(),
                        cumulative_dropped_tokens: meta
                            .cumulative_dropped_tokens
                            .unwrap_or_default(),
                        duration_ms: meta.duration_ms.unwrap_or_default(),
                        // Joined to the command that caused it once the whole
                        // session is read — the command's record is written
                        // after this one. See `session::attach_instructions`.
                        instruction_chars: None,
                        instruction: None,
                    }));
                    continue;
                }
                if subtype != STOP_SUMMARY_SUBTYPE {
                    coverage.count_unconsumed(format!("{kind}:{subtype}"));
                    continue;
                }
                let hooks = raw
                    .hook_infos
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|h| {
                        Some(HookRun {
                            command: h.command?,
                            duration_ms: h.duration_ms.unwrap_or(0),
                        })
                    })
                    .collect();
                out.push(Record::StopSummary(StopSummary {
                    citation,
                    hooks,
                    errors: raw.hook_errors.map(|e| e.len()).unwrap_or(0),
                    prevented_continuation: raw.prevented_continuation.unwrap_or(false),
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
        let recs = read_transcript(&path, Window::default(), &mut cov).unwrap();
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
    fn an_unconsumed_subtype_is_counted_under_its_qualified_name() {
        let (recs, cov) = rec(&format!(
            "{}\n{}",
            format_args!(
                r#"{{"type":"attachment",{BASE},"attachment":{{"type":"hook_success"}}}}"#
            ),
            format_args!(r#"{{"type":"system",{BASE},"subtype":"local_command"}}"#)
        ));
        assert!(recs.is_empty());
        assert_eq!(
            cov.record_types_unconsumed.get("attachment:hook_success"),
            Some(&1)
        );
        assert_eq!(
            cov.record_types_unconsumed.get("system:local_command"),
            Some(&1)
        );
    }

    #[test]
    fn every_record_read_lands_in_exactly_one_bucket() {
        // One of each way a line can end: kept, unreadable, a type this module
        // does not consume, a consumed type missing what places it in time, and
        // a copy the runtime replayed out of another session.
        let (kept, cov) = rec(&format!(
            "{}\n{}\n{}\n{}\n{}",
            format_args!(
                r#"{{"type":"user",{BASE},"origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":"hi"}}}}"#
            ),
            "not json",
            format_args!(r#"{{"type":"artifact-comment-monitor",{BASE}}}"#),
            r#"{"type":"user","uuid":"u2","sessionId":"s","message":{"content":"no timestamp"}}"#,
            format_args!(
                r#"{{"type":"user",{BASE},"forkedFrom":{{"sessionId":"earlier"}},"origin":{{"kind":"human"}},"promptSource":"typed","message":{{"content":"replayed"}}}}"#
            ),
        ));

        let accounted = kept.len()
            + cov.records_malformed
            + cov.records_forked
            + cov.record_types_unconsumed.values().sum::<usize>();
        assert_eq!(
            accounted, cov.records_total,
            "a record read and neither kept nor counted has gone missing"
        );
        assert_eq!(
            (kept.len(), cov.records_forked, cov.records_malformed),
            (1, 1, 2)
        );
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
    fn a_denial_resolves_to_the_tool_it_answered_without_reading_the_message() {
        let (recs, _) = rec(&format!(
            "{}\n{}",
            format_args!(
                r#"{{"type":"assistant",{BASE},"message":{{"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"rm -rf /"}}}}]}}}}"#
            ),
            format_args!(
                r#"{{"type":"user","uuid":"u2","timestamp":"2026-08-26T00:00:01Z","sessionId":"s1","toolDenialKind":"permission-rule","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":"Permission to use Bash with command rm -rf / has been denied."}}]}}}}"#
            )
        ));
        match &recs[1] {
            Record::User(u) => {
                let d = u.denial.as_ref().expect("denial");
                assert_eq!(d.kind, "permission-rule");
                assert_eq!(d.tool.as_deref(), Some("Bash"));
            }
            _ => panic!("expected a user turn"),
        }
    }

    #[test]
    fn a_denial_whose_call_is_not_in_this_transcript_names_no_tool() {
        let (recs, _) = rec(
            r#"{"type":"user","uuid":"u2","timestamp":"2026-08-26T00:00:01Z","sessionId":"s1","toolDenialKind":"user-rejected","message":{"content":[{"type":"tool_result","tool_use_id":"elsewhere","content":"no"}]}}"#,
        );
        match &recs[0] {
            Record::User(u) => assert!(u.denial.as_ref().unwrap().tool.is_none()),
            _ => panic!("expected a user turn"),
        }
    }

    #[test]
    fn a_rule_load_carries_its_path_and_the_characters_it_cost() {
        let (recs, _) = rec(&format!(
            r#"{{"type":"attachment",{BASE},"attachment":{{"type":"nested_memory","path":"/repo/.claude/rules/testing.md","content":{{"content":"abcde"}}}}}}"#
        ));
        match &recs[0] {
            Record::RuleLoad(r) => {
                assert_eq!(r.path, PathBuf::from("/repo/.claude/rules/testing.md"));
                assert_eq!(r.chars, 5);
            }
            _ => panic!("expected a rule load"),
        }
    }

    #[test]
    fn a_stop_summary_carries_each_hook_and_whether_it_held_the_agent() {
        let (recs, _) = rec(&format!(
            r#"{{"type":"system",{BASE},"subtype":"stop_hook_summary","hookInfos":[{{"command":"afplay chime &","durationMs":2543}}],"hookErrors":[],"preventedContinuation":false}}"#
        ));
        match &recs[0] {
            Record::StopSummary(s) => {
                assert_eq!(s.hooks[0].command, "afplay chime &");
                assert_eq!(s.hooks[0].duration_ms, 2543);
                assert_eq!(s.errors, 0);
                assert!(!s.prevented_continuation);
            }
            _ => panic!("expected a stop summary"),
        }
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
