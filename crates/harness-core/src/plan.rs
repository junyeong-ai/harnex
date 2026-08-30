//! # plan — the spec-workflow review grammar, held by a computer
//!
//! The spec-workflow and review-lenses patterns write three machine-read
//! shapes into a spec's artifacts: finding rows under `## Outstanding issues`
//! in the plan, decision bullets under `## Decision log` in the spec, and the
//! `<n>C/<n>B/<n>M/<n>m` counts a review-class firing records. The templates
//! state that contract in prose, and [`PlanAuditor`] is the computer the
//! prose names: open Critical/Blocker rows, rows that vanished instead of
//! gaining a terminal disposition, decision lines whose counts contradict
//! their token, and a Critical+Blocker total that will not fall. The measured
//! failure of the prose-only version of this floor is a gate that recorded
//! eleven firings while its own rule said stop at the second.
//!
//! The grammar is harness vocabulary — harnex's own templates emit every
//! token here, the same standing the sentinel grammars have. Gate NAMES stay
//! open (a project may add gates at install time): a decision line under any
//! gate parses, the counts obligations bind the two review-class gates the
//! templates ship, and any gate that writes counts opts into the convergence
//! comparison by doing so.
//!
//! ## What this module refuses to do
//!
//! - Never guesses at project layout. Callers name the files; where specs
//!   live is project vocabulary and stays out of this crate (Constitution
//!   VII).
//! - Never reads git. The baselines — the committed plan for the vanish
//!   check, the committed spec for the log's append-only check — are text the
//!   caller supplies; the shipped pre-commit arm pipes `git show` into both.
//! - Never judges substance. Whether a disposition's evidence is honest is
//!   review-held; this computer holds shape, presence and arithmetic.
//! - Never accepts a variant row silently. A list item carrying a bracketed
//!   severity that does not parse is a finding, not a row — the measured
//!   alternative is a grammar whose variants are invisible to every gate
//!   reading it.
//! - Never joins `check`. The gate runs where the files are named — the
//!   pre-commit arm and the skill — because a project-wide walk would need
//!   the layout this module refuses to guess.

use std::collections::BTreeMap;
use std::path::Path;

use crate::envelope::{Finding, Location, Severity};
use crate::wire_enum::wire_enum;

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Terminal dispositions a cleared finding row records, end-anchored as
    /// `[<disposition>: <evidence>]`. A row is never deleted; it ends open or
    /// it ends with one of these.
    pub enum Disposition {
        Fixed => "fixed",
        Refuted => "refuted",
        Accepted => "accepted",
    }
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    /// Finding ranks as a row spells them: `[Critical]` … `[Minor]`. Distinct
    /// from [`crate::envelope::Severity`], which ranks this toolkit's own
    /// findings — a review row's rank is data this module parses, not a
    /// judgment it makes.
    pub enum ReviewSeverity {
        Critical => "Critical",
        Blocker => "Blocker",
        Major => "Major",
        Minor => "Minor",
    }
}

impl ReviewSeverity {
    /// Whether an open row of this rank fails the review gate.
    pub fn blocks(self) -> bool {
        matches!(self, Self::Critical | Self::Blocker)
    }

    /// The letter this rank carries in a counts token (`0C/2B/3M/1m`).
    fn count_letter(self) -> char {
        match self {
            Self::Critical => 'C',
            Self::Blocker => 'B',
            Self::Major => 'M',
            Self::Minor => 'm',
        }
    }
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// The closed decision vocabulary of a gate firing.
    pub enum GateDecision {
        Approved => "approved",
        Rejected => "rejected",
        NeedsRevision => "needs_revision",
        Deferred => "deferred",
    }
}

/// The gates whose firings must write counts into their decision lines.
/// Gate names are otherwise open — a project may declare more at install
/// time — but these two ship in the templates and carry the convergence
/// contract.
pub const REVIEW_CLASS_GATES: [&str; 2] = ["design_review", "review"];

/// Section heading the finding rows live under, in the plan.
pub const OUTSTANDING_HEADING: &str = "Outstanding issues";

/// Section heading the decision bullets live under, in the spec.
pub const DECISION_LOG_HEADING: &str = "Decision log";

/// Rationale prefix that records the operator's acknowledgement of a
/// non-falling Critical+Blocker count, authorizing another round.
pub const ACKNOWLEDGED_PREFIX: &str = "acknowledged:";

/// One decision bullet from `## Decision log`, as
/// `<date> · <gate> · <token> [· <counts>] · <rationale>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionLine {
    pub gate: String,
    pub decision: GateDecision,
    pub counts: Option<Counts>,
    pub rationale: String,
}

/// A `<n>C/<n>B/<n>M/<n>m` token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub critical: u32,
    pub blocker: u32,
    pub major: u32,
    pub minor: u32,
}

impl Counts {
    /// The convergence total: Critical + Blocker. What must fall between
    /// consecutive `needs_revision` firings, and be zero on `approved`.
    pub fn blocking(self) -> u32 {
        self.critical + self.blocker
    }

    fn parse(segment: &str) -> Option<Self> {
        let mut parts = segment.split('/');
        let mut take = |severity: ReviewSeverity| {
            let part = parts.next()?;
            let digits = part.strip_suffix(severity.count_letter())?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()
        };
        let counts = Self {
            critical: take(ReviewSeverity::Critical)?,
            blocker: take(ReviewSeverity::Blocker)?,
            major: take(ReviewSeverity::Major)?,
            minor: take(ReviewSeverity::Minor)?,
        };
        parts.next().is_none().then_some(counts)
    }
}

/// Parse one decision bullet's text (marker already stripped).
///
/// Segments split on `·` and trim, because spacing around the separator is an
/// author's habit rather than part of the grammar. The rationale is the tail
/// rejoined, so a `·` inside it survives; the counts position is the fourth
/// segment and nothing else — a rationale whose first segment happens to be
/// counts-shaped is read as counts, which is what the line then declares.
pub fn parse_decision(text: &str) -> Option<DecisionLine> {
    let segments: Vec<&str> = text.split('·').map(str::trim).collect();
    if segments.len() < 4 {
        return None;
    }
    if !is_iso_date(segments[0]) {
        return None;
    }
    let gate = segments[1];
    if gate.is_empty()
        || !gate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let decision = GateDecision::from_str(segments[2])?;
    let (counts, tail) = match Counts::parse(segments[3]) {
        Some(counts) => (Some(counts), &segments[4..]),
        None => (None, &segments[3..]),
    };
    let rationale = tail.join(" · ");
    if rationale.is_empty() {
        return None;
    }
    Some(DecisionLine {
        gate: gate.to_string(),
        decision,
        counts,
        rationale,
    })
}

/// `YYYY-MM-DD` by shape first, then by calendar. The shape check is not
/// redundant: `strptime` accepts an unpadded month, and a date only mostly in
/// the grammar is a line only mostly in the log.
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .into_iter()
            .all(|i: usize| b[i].is_ascii_digit())
        && jiff::civil::Date::strptime("%Y-%m-%d", s).is_ok()
}

/// One finding row: `- [<Severity>] <text>`, optionally ending with its
/// terminal disposition `[<disposition>: <evidence>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRow {
    pub severity: ReviewSeverity,
    /// The finding text, disposition trailer excluded, whitespace collapsed —
    /// the identity the vanish check compares under.
    pub text: String,
    pub disposition: Option<Disposition>,
}

impl FindingRow {
    pub fn open(&self) -> bool {
        self.disposition.is_none()
    }
}

/// Parse one row's joined text (marker already stripped).
///
/// The disposition trailer is recognized only when the raw text literally
/// ends with it: a row that quotes the grammar in a code span, or follows the
/// trailer with more prose, reads as open — the loud direction, because an
/// open row blocks and a closed one releases.
pub fn parse_row(text: &str) -> Option<FindingRow> {
    let inner = text.strip_prefix('[')?;
    let (rank, rest) = inner.split_once(']')?;
    let severity = ReviewSeverity::from_str(rank)?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let (body, disposition) = match trailing_disposition(rest) {
        Some((disposition, body)) => (body, Some(disposition)),
        None => (rest, None),
    };
    let text = normalize(body);
    if text.is_empty() {
        return None;
    }
    Some(FindingRow {
        severity,
        text,
        disposition,
    })
}

/// The disposition a row's text ends with, and the text before it.
fn trailing_disposition(text: &str) -> Option<(Disposition, &str)> {
    let text = text.trim_end();
    if !text.ends_with(']') {
        return None;
    }
    let open = text.rfind('[')?;
    let inner = &text[open + 1..text.len() - 1];
    let (token, evidence) = inner.split_once(':')?;
    let disposition = Disposition::from_str(token)?;
    if evidence.trim().is_empty() || evidence.contains(['[', ']']) {
        return None;
    }
    Some((disposition, &text[..open]))
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a list item that failed to parse is nevertheless finding-shaped:
/// it carries a bracketed token that names a severity once bold markers and
/// case are ignored. `* **[critical]** …` is invisible to a parser and a
/// finding to a reader, and silence about it is how a grammar's variants
/// disarm every gate that reads it. Code spans are stripped first — a row
/// legitimately quotes grammar it is a finding about.
fn finding_shaped(text: &str) -> bool {
    // Fullwidth brackets are what a CJK IME plausibly emits around the same
    // token; the net reads them as the brackets they are.
    let stripped = strip_code_spans(text).replace('［', "[").replace('］', "]");
    let mut rest = stripped.as_str();
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            return false;
        };
        let token = after[..close].trim_matches(['*', '_', ' ']);
        if ReviewSeverity::ALL
            .iter()
            .any(|s| s.as_str().eq_ignore_ascii_case(token))
        {
            return true;
        }
        // Advance past the opening bracket only: pairing this `[` with the
        // next `]` and skipping everything between let one stray `[` earlier
        // on the line swallow a real `**[Critical]**` after it.
        rest = after;
    }
    false
}

/// Remove CommonMark code spans: a run of N backticks up to the next run of
/// exactly N. An unmatched run stays literal, as the spec reads it.
fn strip_code_spans(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let run = i - start;
            if let Some(close) = find_backtick_run(&text[i..], run) {
                i += close + run;
                continue;
            }
            out.push_str(&text[start..i]);
            continue;
        }
        let ch = text[i..].chars().next().expect("in-bounds char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Byte offset just past a run of exactly `len` backticks in `text`.
fn find_backtick_run(text: &str, len: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - start == len {
                return Some(start);
            }
            continue;
        }
        i += 1;
    }
    None
}

/// A `## <heading>` section read out of a document, fence-aware.
enum Section {
    Found {
        items: Vec<Item>,
        /// Section lines that are neither an item nor a continuation of one —
        /// the wide net reads these too, because a row a reader sees and no
        /// parser claims is precisely what must not pass in silence.
        loose: Vec<(u32, String)>,
    },
    Missing {
        fenced_copy: bool,
    },
    Unreadable {
        line: u32,
        reason: String,
    },
}

/// One top-level list item in a section: its 1-based line, its marker, and
/// its text with continuation lines joined.
struct Item {
    line: u32,
    canonical_marker: bool,
    text: String,
}

/// The ATX heading a line spells, as (level, title) — up to three leading
/// spaces, one to six `#`s, whitespace (or end of line), and an optional
/// closing run of `#`s, per CommonMark. A reader that recognizes fewer
/// heading spellings than a renderer turns a cosmetic trailing `#` into a
/// missing section, and a missing section disarms every check that reads it.
fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let unindented = line.trim_start_matches(' ');
    if line.len() - unindented.len() > 3 {
        return None;
    }
    let level = unindented.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &unindented[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let title = rest.trim_matches([' ', '\t']);
    let stripped = title.trim_end_matches('#');
    if stripped.len() == title.len() {
        return Some((level, title));
    }
    // A closing sequence counts only when whitespace separates it from the
    // title (or it is the whole remainder) — `## a#b` keeps its `#`.
    if stripped.is_empty() {
        return Some((level, stripped));
    }
    match stripped.ends_with([' ', '\t']) {
        true => Some((level, stripped.trim_end_matches([' ', '\t']))),
        false => Some((level, title)),
    }
}

/// Extract the items under `## <heading>`.
///
/// One reader for both sections, because two implementations of "which lines
/// are in the section" is how a fenced decoy heading passes one gate and
/// fails another. CommonMark-shaped: fences open on 3+ backticks or tildes
/// (an info string containing a backtick is not a fence), close on a run of
/// the same character at least as long, and a heading or list marker inside
/// one is content. Unreadable — a duplicate heading, or a fence left open
/// across the section — is its own outcome, never conflated with empty: the
/// difference between "no open findings" and "findings this reader cannot
/// see" is the difference the append-only contract exists to protect.
fn section_of(text: &str, heading: &str) -> Section {
    let wanted = format!("## {heading}");
    let mut fence: Option<(u8, usize, u32)> = None;
    let mut found_at: Option<u32> = None;
    let mut ended = false;
    let mut items: Vec<Item> = Vec::new();
    let mut loose: Vec<(u32, String)> = Vec::new();
    let mut open_item: Option<Item> = None;

    for (idx, raw) in text.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let unindented = line.trim_start_matches(' ');
        let indent = line.len() - unindented.len();

        if let Some((char_, len, _)) = fence {
            if indent <= 3
                && unindented.bytes().take_while(|&b| b == char_).count() >= len
                && unindented.trim_end().bytes().all(|b| b == char_)
            {
                fence = None;
            }
            continue;
        }
        if indent <= 3 {
            let ticks = unindented.bytes().take_while(|&b| b == b'`').count();
            let tildes = unindented.bytes().take_while(|&b| b == b'~').count();
            if ticks >= 3 && !unindented[ticks..].contains('`') {
                fence = Some((b'`', ticks, line_no));
                continue;
            }
            if tildes >= 3 {
                fence = Some((b'~', tildes, line_no));
                continue;
            }
        }

        let is_wanted = atx_heading(line) == Some((2, heading));
        match found_at {
            None => {
                if is_wanted {
                    found_at = Some(line_no);
                }
            }
            Some(at) if !ended => {
                if is_wanted {
                    return Section::Unreadable {
                        line: line_no,
                        reason: format!(
                            "`{wanted}` appears again at line {line_no} (first at line {at}) \
                             — two sections with one name cannot be read as one"
                        ),
                    };
                }
                if let Some((level, _)) = atx_heading(line)
                    && level <= 2
                {
                    ended = true;
                    continue;
                }
                collect(
                    &mut items,
                    &mut loose,
                    &mut open_item,
                    line_no,
                    line,
                    unindented,
                    indent,
                );
            }
            Some(_) => {
                if is_wanted {
                    return Section::Unreadable {
                        line: line_no,
                        reason: format!(
                            "`{wanted}` appears again at line {line_no} — two sections with one \
                             name cannot be read as one"
                        ),
                    };
                }
            }
        }
    }

    if let (Some((_, _, opened)), false) = (fence, ended)
        && found_at.is_some()
    {
        return Section::Unreadable {
            line: opened,
            reason: format!(
                "a code fence opened at line {opened} never closes, so where the section ends \
                 cannot be read"
            ),
        };
    }
    if let Some(item) = open_item.take() {
        items.push(item);
    }
    match found_at {
        Some(_) => Section::Found { items, loose },
        None => Section::Missing {
            fenced_copy: text.contains(&wanted),
        },
    }
}

/// Grow `items` — or `loose` — by one line of section content.
///
/// A top-level item opens at indent ≤ 3 with a space-indented marker; every
/// following non-blank, non-item line continues it wherever it is indented —
/// CommonMark's lazy continuation, and the direction that cannot silently
/// split a row from its disposition. A blank line closes the open item. A
/// non-blank line that neither opens nor continues an item — tab-indented,
/// blockquoted, indented past the item margin, an out-of-range ordinal — goes
/// to `loose`, where the wide net still reads it: dropping it is how a row a
/// reader sees becomes a row no gate does.
fn collect(
    items: &mut Vec<Item>,
    loose: &mut Vec<(u32, String)>,
    open_item: &mut Option<Item>,
    line_no: u32,
    line: &str,
    unindented: &str,
    indent: usize,
) {
    if line.trim().is_empty() {
        if let Some(item) = open_item.take() {
            items.push(item);
        }
        return;
    }
    if indent <= 3
        && let Some(text) = item_text(unindented)
    {
        if let Some(item) = open_item.take() {
            items.push(item);
        }
        *open_item = Some(Item {
            line: line_no,
            canonical_marker: unindented.starts_with("- "),
            text: text.to_string(),
        });
        return;
    }
    if let Some(item) = open_item.as_mut() {
        item.text.push(' ');
        item.text.push_str(line.trim());
        return;
    }
    loose.push((line_no, line.trim().to_string()));
}

/// The text after a list marker, for any CommonMark marker.
fn item_text(unindented: &str) -> Option<&str> {
    if let Some(rest) = unindented
        .strip_prefix("- ")
        .or_else(|| unindented.strip_prefix("* "))
        .or_else(|| unindented.strip_prefix("+ "))
    {
        return Some(rest);
    }
    let digits = unindented
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if (1..=9).contains(&digits) {
        let rest = &unindented[digits..];
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(text);
        }
    }
    None
}

/// Cross-input auditor over a spec's plan, decision log, and committed
/// baseline. Every input is text the caller read; see the module doc for what
/// stays out.
pub struct PlanAuditor<'a> {
    plan_path: &'a Path,
    /// `None` when the plan file no longer exists — a deletion, which the
    /// baseline's open rows then witness.
    plan: Option<&'a str>,
    spec: Option<(&'a Path, &'a str)>,
    baseline: Option<&'a str>,
    /// The committed baseline of the spec, holding the decision log to its
    /// append-only contract — without it, editing an earlier bullet's counts
    /// launders the convergence comparison the log exists to compute.
    baseline_spec: Option<&'a str>,
}

impl<'a> PlanAuditor<'a> {
    pub fn new(
        plan_path: &'a Path,
        plan: Option<&'a str>,
        spec: Option<(&'a Path, &'a str)>,
        baseline: Option<&'a str>,
        baseline_spec: Option<&'a str>,
    ) -> Self {
        Self {
            plan_path,
            plan,
            spec,
            baseline,
            baseline_spec,
        }
    }

    pub fn audit(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        let rows = self.plan_rows(&mut findings);
        if let Some((spec_path, spec_text)) = self.spec {
            self.audit_log(spec_path, spec_text, rows.as_deref(), &mut findings);
            self.audit_log_rewrite(spec_path, spec_text, &mut findings);
        }
        self.audit_vanish(rows.as_deref(), &mut findings);
        findings
    }

    /// The decision log is append-only: the baseline's bullets must stand as
    /// a prefix of the current log, verbatim. An edited, reordered or removed
    /// bullet rewrites the history every convergence comparison reads — the
    /// same laundering the vanish check refuses for finding rows. An
    /// unreadable or absent baseline holds nothing.
    fn audit_log_rewrite(&self, spec_path: &Path, spec_text: &str, findings: &mut Vec<Finding>) {
        let Some(baseline_spec) = self.baseline_spec else {
            return;
        };
        let Section::Found { items: held, .. } = section_of(baseline_spec, DECISION_LOG_HEADING)
        else {
            return;
        };
        let Section::Found { items: current, .. } = section_of(spec_text, DECISION_LOG_HEADING)
        else {
            // The current log's own Missing/Unreadable finding stands.
            return;
        };
        for (index, item) in held.iter().enumerate() {
            let kept = current
                .get(index)
                .is_some_and(|c| normalize(&c.text) == normalize(&item.text));
            if !kept {
                findings.push(Finding {
                    slug: "plan-log-rewritten".into(),
                    severity: Severity::Blocker,
                    location: Location::file(spec_path),
                    message: format!(
                        "the committed decision log's bullet {} is edited, moved or gone: {}",
                        index + 1,
                        normalize(&item.text)
                    ),
                    hint: Some(
                        "the log is append-only — a gate that fires again appends a new bullet; \
                         restore the committed bullets verbatim and record the new decision \
                         after them"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
                return;
            }
        }
    }

    /// The plan's rows, with the row-level findings; `None` when the section
    /// could not be read (its own finding already stands, and comparisons
    /// against rows nobody could read would judge the wrong thing).
    fn plan_rows(&self, findings: &mut Vec<Finding>) -> Option<Vec<FindingRow>> {
        let text = self.plan?;
        match section_of(text, OUTSTANDING_HEADING) {
            Section::Missing { fenced_copy } => {
                findings.push(Finding {
                    slug: "plan-outstanding-missing".into(),
                    severity: Severity::Major,
                    location: Location::file(self.plan_path),
                    message: if fenced_copy {
                        format!(
                            "`## {OUTSTANDING_HEADING}` exists only inside a code fence or \
                             malformed context — no readable section carries the findings"
                        )
                    } else {
                        format!("no `## {OUTSTANDING_HEADING}` section")
                    },
                    hint: Some(format!(
                        "restore the `## {OUTSTANDING_HEADING}` section from this project's \
                         spec plan template — the gates write findings there and every reader \
                         of this plan starts from it"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                None
            }
            Section::Unreadable { line, reason } => {
                findings.push(unreadable(self.plan_path, line, &reason));
                None
            }
            Section::Found { items, loose } => {
                let mut rows = Vec::new();
                for item in items {
                    match (item.canonical_marker, parse_row(&item.text)) {
                        (true, Some(row)) => {
                            if row.open() && row.severity.blocks() {
                                findings.push(Finding {
                                    slug: "plan-open-blocker".into(),
                                    severity: Severity::Blocker,
                                    location: Location::line(self.plan_path, item.line),
                                    message: format!(
                                        "open [{}] finding: {}",
                                        row.severity.as_str(),
                                        row.text
                                    ),
                                    hint: Some(
                                        "fix it and end the row with `[fixed: what pinned it]`, \
                                         refute it with `[refuted: the ground truth]`, or record \
                                         `[accepted: who accepted it and why]` — the review gate \
                                         passes on zero open Critical/Blocker rows"
                                            .into(),
                                    ),
                                    auto_fixable: false,
                                    fix_command: None,
                                });
                            }
                            // A severity token buried past the row's own —
                            // a nested sub-row merged in as continuation —
                            // must not ride out inside a lower rank's text.
                            let body = &item.text[item.text.find(']').map_or(0, |i| i + 1)..];
                            if finding_shaped(body) {
                                findings.push(unparseable_row(
                                    self.plan_path,
                                    item.line,
                                    &item.text,
                                ));
                            }
                            rows.push(row);
                        }
                        _ if finding_shaped(&item.text) => {
                            findings.push(unparseable_row(self.plan_path, item.line, &item.text));
                        }
                        _ => {}
                    }
                }
                for (line, text) in loose {
                    if finding_shaped(&text) {
                        findings.push(unparseable_row(self.plan_path, line, &text));
                    }
                }
                Some(rows)
            }
        }
    }

    fn audit_log(
        &self,
        spec_path: &Path,
        spec_text: &str,
        rows: Option<&[FindingRow]>,
        findings: &mut Vec<Finding>,
    ) {
        let items = match section_of(spec_text, DECISION_LOG_HEADING) {
            Section::Missing { fenced_copy } => {
                findings.push(Finding {
                    slug: "plan-log-missing".into(),
                    severity: Severity::Major,
                    location: Location::file(spec_path),
                    message: if fenced_copy {
                        format!(
                            "`## {DECISION_LOG_HEADING}` exists only inside a code fence or \
                             malformed context"
                        )
                    } else {
                        format!("no `## {DECISION_LOG_HEADING}` section")
                    },
                    hint: Some(format!(
                        "restore the `## {DECISION_LOG_HEADING}` section from this project's \
                         spec template — an unrecorded gate is a gate that did not fire"
                    )),
                    auto_fixable: false,
                    fix_command: None,
                });
                return;
            }
            Section::Unreadable { line, reason } => {
                findings.push(unreadable(spec_path, line, &reason));
                return;
            }
            Section::Found { items, .. } => items,
        };

        let mut decisions = Vec::new();
        for item in items {
            match parse_decision(&item.text) {
                Some(line) if item.canonical_marker => decisions.push((item.line, line)),
                // Decision-shaped is the grammar's own separator, or a bullet
                // opening on a date — a lookalike dot (`•` for `·`) must not
                // drop a line out of every accounting check in silence.
                _ if item.text.contains('·') || item.text.get(..10).is_some_and(is_iso_date) => {
                    findings.push(Finding {
                        slug: "plan-log-unparseable".into(),
                        severity: Severity::Major,
                        location: Location::line(spec_path, item.line),
                        message: format!(
                            "decision bullet no gate can read: {}",
                            normalize(&item.text)
                        ),
                        hint: Some(
                            "write decisions as `- <YYYY-MM-DD> · <gate> · approved|rejected|\
                             needs_revision|deferred [· <n>C/<n>B/<n>M/<n>m] · <rationale>` — \
                             the separator is `·` (U+00B7)"
                                .into(),
                        ),
                        auto_fixable: false,
                        fix_command: None,
                    });
                }
                _ => {}
            }
        }

        // Keyed case-folded: `Review` and `review` are one gate to a reader,
        // and letting them track separately reset the comparison a case-typo
        // was enough to escape.
        let mut last_blocking: BTreeMap<String, u32> = BTreeMap::new();
        for (line_no, decision) in &decisions {
            let gate = decision.gate.as_str();
            let gate_key = gate.to_ascii_lowercase();
            let review_class = REVIEW_CLASS_GATES.contains(&gate_key.as_str());
            if review_class
                && matches!(
                    decision.decision,
                    GateDecision::Approved | GateDecision::NeedsRevision
                )
                && decision.counts.is_none()
            {
                findings.push(Finding {
                    slug: "plan-log-counts-missing".into(),
                    severity: Severity::Major,
                    location: Location::line(spec_path, *line_no),
                    message: format!(
                        "`{gate}` recorded `{}` without counts",
                        decision.decision.as_str()
                    ),
                    hint: Some(
                        "a review-class firing writes `<n>C/<n>B/<n>M/<n>m` into its line — the \
                         next firing's convergence comparison reads it there, never from memory"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
            match decision.decision {
                GateDecision::NeedsRevision => {
                    if let Some(counts) = decision.counts {
                        if let Some(&previous) = last_blocking.get(&gate_key)
                            && counts.blocking() >= previous
                            && !decision.rationale.starts_with(ACKNOWLEDGED_PREFIX)
                        {
                            findings.push(Finding {
                                slug: "plan-log-not-falling".into(),
                                severity: Severity::Blocker,
                                location: Location::line(spec_path, *line_no),
                                message: format!(
                                    "`{gate}` re-fired with Critical+Blocker at {} — not below \
                                     the previous firing's {previous}",
                                    counts.blocking()
                                ),
                                hint: Some(format!(
                                    "escalate to the operator instead of firing again; riding on \
                                     takes their recorded acknowledgement — a rationale beginning \
                                     `{ACKNOWLEDGED_PREFIX}` naming why another round is justified"
                                )),
                                auto_fixable: false,
                                fix_command: None,
                            });
                        }
                        last_blocking.insert(gate_key.clone(), counts.blocking());
                    }
                }
                GateDecision::Approved | GateDecision::Rejected | GateDecision::Deferred => {
                    last_blocking.remove(&gate_key);
                }
            }
            if decision.decision == GateDecision::Approved
                && let Some(counts) = decision.counts
                && counts.blocking() > 0
            {
                findings.push(Finding {
                    slug: "plan-log-approved-nonzero".into(),
                    severity: Severity::Blocker,
                    location: Location::line(spec_path, *line_no),
                    message: format!(
                        "`{gate}` recorded `approved` carrying {} open Critical+Blocker",
                        counts.blocking()
                    ),
                    hint: Some(
                        "approved means zero Critical and zero Blocker — record `needs_revision` \
                         with the counts, or clear the findings first"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }

        if let Some(rows) = rows
            && let Some((line_no, decision)) = decisions.iter().rev().find(|(_, d)| {
                REVIEW_CLASS_GATES
                    .iter()
                    .any(|g| g.eq_ignore_ascii_case(&d.gate))
            })
            && decision.decision == GateDecision::Approved
        {
            let open_blocking = rows
                .iter()
                .filter(|r| r.open() && r.severity.blocks())
                .count();
            if open_blocking > 0 {
                findings.push(Finding {
                    slug: "plan-log-approved-open".into(),
                    severity: Severity::Blocker,
                    location: Location::line(spec_path, *line_no),
                    message: format!(
                        "the last review-class decision is `{}` `approved`, yet the plan holds \
                         {open_blocking} open Critical/Blocker row(s)",
                        decision.gate
                    ),
                    hint: Some(
                        "an approval and an open blocking finding cannot both stand — dispose of \
                         the rows or record `needs_revision`"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
    }

    /// Every open row the baseline held must survive in the current plan —
    /// verbatim, still open or now carrying its disposition. An unreadable or
    /// absent baseline holds nothing; an absent current plan is a deletion
    /// every baseline row witnesses.
    fn audit_vanish(&self, rows: Option<&[FindingRow]>, findings: &mut Vec<Finding>) {
        let Some(baseline) = self.baseline else {
            return;
        };
        let Section::Found { items, .. } = section_of(baseline, OUTSTANDING_HEADING) else {
            return;
        };
        // Identity is rank AND text: matching text alone let a one-word
        // severity downgrade dismiss a Critical without a disposition — the
        // cheapest possible laundering of the contract this check holds.
        let current: Vec<(ReviewSeverity, &str)> = match (self.plan, rows) {
            // The current section is unreadable; its own finding stands and
            // a vanish verdict against rows nobody read would be noise.
            (Some(_), None) => return,
            (None, _) => Vec::new(),
            (Some(_), Some(rows)) => rows.iter().map(|r| (r.severity, r.text.as_str())).collect(),
        };
        for item in items {
            let Some(row) = (item.canonical_marker) // structural filter first
                .then(|| parse_row(&item.text))
                .flatten()
            else {
                continue;
            };
            if row.open() && !current.contains(&(row.severity, row.text.as_str())) {
                findings.push(Finding {
                    slug: "plan-row-vanished".into(),
                    severity: Severity::Blocker,
                    location: Location::file(self.plan_path),
                    message: format!(
                        "open [{}] finding the committed plan held is gone: {}",
                        row.severity.as_str(),
                        row.text
                    ),
                    hint: Some(
                        "a row is never deleted, reworded or downgraded — restore it verbatim at \
                         its rank and end it with `[fixed: …]`, `[refuted: …]` or `[accepted: …]`"
                            .into(),
                    ),
                    auto_fixable: false,
                    fix_command: None,
                });
            }
        }
    }
}

fn unparseable_row(path: &Path, line: u32, text: &str) -> Finding {
    Finding {
        slug: "plan-row-unparseable".into(),
        severity: Severity::Major,
        location: Location::line(path, line),
        message: format!("finding-shaped row no gate can read: {}", normalize(text)),
        hint: Some(
            "write each finding as its own flat `- [Critical|Blocker|Major|Minor] <finding>` \
             row — a variant marker, case, bolding, nesting or indentation is invisible to the \
             gates that read this section"
                .into(),
        ),
        auto_fixable: false,
        fix_command: None,
    }
}

fn unreadable(path: &Path, line: u32, reason: &str) -> Finding {
    Finding {
        slug: "plan-section-unreadable".into(),
        severity: Severity::Blocker,
        location: Location::line(path, line),
        message: reason.to_string(),
        hint: Some(
            "an unreadable section and an empty one are different states — repair the structure \
             so the gates can tell them apart"
                .into(),
        ),
        auto_fixable: false,
        fix_command: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn audit(plan: &str) -> Vec<Finding> {
        PlanAuditor::new(&PathBuf::from("plan.md"), Some(plan), None, None, None).audit()
    }

    fn audit_with_spec(plan: &str, spec: &str) -> Vec<Finding> {
        let spec_path = PathBuf::from("spec.md");
        PlanAuditor::new(
            &PathBuf::from("plan.md"),
            Some(plan),
            Some((&spec_path, spec)),
            None,
            None,
        )
        .audit()
    }

    fn audit_against(baseline: &str, plan: Option<&str>) -> Vec<Finding> {
        PlanAuditor::new(&PathBuf::from("plan.md"), plan, None, Some(baseline), None).audit()
    }

    fn audit_log_against(baseline_spec: &str, current_spec: &str) -> Vec<Finding> {
        let spec_path = PathBuf::from("spec.md");
        PlanAuditor::new(
            &PathBuf::from("plan.md"),
            Some(&plan("")),
            Some((&spec_path, current_spec)),
            None,
            Some(baseline_spec),
        )
        .audit()
    }

    fn slugs(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.slug.as_str()).collect()
    }

    fn plan(rows: &str) -> String {
        format!("# t — Plan\n\n## Outstanding issues\n\n{rows}\n")
    }

    fn spec(log: &str) -> String {
        format!("# t\n\n## Decision log\n\n{log}\n")
    }

    #[test]
    fn an_open_blocking_row_is_the_gate() {
        let findings = audit(&plan("- [Critical] the migration drops rows"));
        assert_eq!(slugs(&findings), ["plan-open-blocker"]);
        assert_eq!(findings[0].severity, Severity::Blocker);
        assert_eq!(findings[0].location.line, Some(5));
    }

    #[test]
    fn a_disposed_row_and_open_followup_rows_pass() {
        let findings = audit(&plan(
            "- [Critical] the migration drops rows [fixed: test_migration_keeps_rows]\n\
             - [Blocker] no rollback path [refuted: DDL here is transactional]\n\
             - [Major] naming drifts from the glossary\n\
             - [Minor] a comment restates the code [accepted: reviewer, docs-only]",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn every_variant_spelling_is_loud_not_invisible() {
        // The measured silent-disarm class: each of these is a finding to a
        // reader and nothing to a naive parser.
        for row in [
            "* [Critical] star marker",
            "+ [Critical] plus marker",
            "1. [Critical] ordered marker",
            "- **[Critical]** bolded rank",
            "- [critical] lower-cased rank",
            "- [ Critical ] padded rank",
            "- __[Blocker]__ underscored",
            "- [Critical]", // rank with no finding text
        ] {
            let findings = audit(&plan(row));
            assert_eq!(slugs(&findings), ["plan-row-unparseable"], "row: {row}");
        }
    }

    #[test]
    fn prose_and_quoted_grammar_are_not_finding_shaped() {
        let findings = audit(&plan(
            "<Findings a gate transcribed. Rows end with a terminal disposition.>\n\n\
             - a note that quotes `[Critical]` inside a code span",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_quoted_disposition_does_not_close_a_row() {
        // The trailer must literally end the raw text; a code span's closing
        // backtick means it does not. Open reads loud, never silently closed.
        let findings = audit(&plan(
            "- [Critical] rows must end with `[fixed: what pinned it]`",
        ));
        assert_eq!(slugs(&findings), ["plan-open-blocker"]);
    }

    #[test]
    fn a_disposition_followed_by_prose_reads_open() {
        let findings = audit(&plan(
            "- [Blocker] flaky test [fixed: pinned the clock] but still worth watching",
        ));
        assert_eq!(slugs(&findings), ["plan-open-blocker"]);
    }

    #[test]
    fn empty_or_bracketed_evidence_is_not_a_disposition() {
        for row in [
            "- [Critical] finding [fixed: ]",
            "- [Critical] finding [fixed: see [1]]",
        ] {
            assert_eq!(slugs(&audit(&plan(row))), ["plan-open-blocker"], "{row}");
        }
    }

    #[test]
    fn a_wrapped_row_joins_its_continuation_lines() {
        let findings = audit(&plan(
            "- [Critical] a finding whose text wraps\n  across lines [fixed: the pin]",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_lazy_continuation_at_column_zero_still_belongs_to_its_row() {
        let findings = audit(&plan(
            "- [Critical] a finding wrapped\nlazily [fixed: the pin]",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_fenced_example_row_is_content_not_a_finding() {
        let findings = audit(&plan(
            "```\n- [Critical] an example inside a fence\n```\n\n- [Minor] real row",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_fenced_decoy_heading_does_not_anchor_the_section() {
        let text = "# t\n\n```\n## Outstanding issues\n```\n\n## Outstanding issues\n\n\
                    - [Critical] real finding\n";
        let findings = audit(text);
        assert_eq!(slugs(&findings), ["plan-open-blocker"]);
    }

    #[test]
    fn crlf_input_reads_the_same() {
        let text = plan("- [Critical] finding [fixed: the pin]").replace('\n', "\r\n");
        assert!(audit(&text).is_empty());
    }

    #[test]
    fn a_missing_section_is_a_finding_not_a_pass() {
        let findings = audit("# t — Plan\n\n## Tasks\n");
        assert_eq!(slugs(&findings), ["plan-outstanding-missing"]);
    }

    #[test]
    fn a_heading_swallowed_by_a_fence_says_so() {
        let text = "```\n## Outstanding issues\n- [Critical] x\n```\n";
        let findings = audit(text);
        assert_eq!(slugs(&findings), ["plan-outstanding-missing"]);
        assert!(findings[0].message.contains("code fence"));
    }

    #[test]
    fn a_duplicate_heading_is_unreadable_not_a_guess() {
        let text = "## Outstanding issues\n\n- [Minor] a\n\n## Outstanding issues\n";
        assert_eq!(slugs(&audit(text)), ["plan-section-unreadable"]);
    }

    #[test]
    fn an_unclosed_fence_across_the_section_is_unreadable() {
        let text = "## Outstanding issues\n\n- [Minor] a\n\n```bash\nnever closed\n";
        assert_eq!(slugs(&audit(text)), ["plan-section-unreadable"]);
    }

    #[test]
    fn an_unclosed_fence_after_the_section_ended_is_someone_elses_problem() {
        let text = "## Outstanding issues\n\n- [Minor] a\n\n## Next\n\n```\nunclosed\n";
        assert!(audit(text).is_empty());
    }

    #[test]
    fn a_subsection_stays_inside_the_section() {
        let text = "## Outstanding issues\n\n### by area\n\n- [Critical] nested finding\n";
        assert_eq!(slugs(&audit(text)), ["plan-open-blocker"]);
    }

    #[test]
    fn deeper_fence_shapes_read_as_commonmark_does() {
        // A ``` inside a ~~~ fence is content; an info string with a backtick
        // is not a fence; a longer closing run closes a shorter opener.
        let text = "## Outstanding issues\n\n~~~\n```\n- [Critical] swallowed\n~~~\n\n\
                    - [Minor] real\n";
        assert!(audit(text).is_empty());
    }

    // ---- decision log ----

    #[test]
    fn the_shipped_example_line_parses() {
        let line = parse_decision(
            "2026-01-15 · review · needs_revision · 0C/2B/3M/1m · two Blockers in the \
             migration path; see plan.md ## Outstanding issues",
        )
        .expect("the template's own example must parse");
        assert_eq!(line.gate, "review");
        assert_eq!(line.decision, GateDecision::NeedsRevision);
        assert_eq!(line.counts.map(Counts::blocking), Some(2));
    }

    #[test]
    fn a_countless_line_and_a_dotted_rationale_parse() {
        let line =
            parse_decision("2026-01-15 · clarify · approved · scope · answered · deferred x")
                .expect("parses");
        assert_eq!(line.counts, None);
        assert_eq!(line.rationale, "scope · answered · deferred x");
    }

    #[test]
    fn malformed_decision_bullets_are_loud() {
        let findings = audit_with_spec(
            &plan(""),
            &spec("- 2026-1-5 · review · needs_revision · 0C/1B/0M/0m · short date"),
        );
        assert_eq!(slugs(&findings), ["plan-log-unparseable"]);
        for text in [
            "not-a-date · review · approved · fine",
            "2026-01-15 · review · looks_good · fine",
            "2026-01-15 · review · approved",
            "2026-01-15 · · approved · fine",
        ] {
            assert!(parse_decision(text).is_none(), "{text}");
        }
    }

    #[test]
    fn a_variant_marker_on_a_decision_bullet_is_loud() {
        let findings = audit_with_spec(
            &plan(""),
            &spec("* 2026-01-15 · review · approved · 0C/0B/1M/0m · fine"),
        );
        assert_eq!(slugs(&findings), ["plan-log-unparseable"]);
    }

    #[test]
    fn the_template_comment_and_prose_are_ignored() {
        let findings = audit_with_spec(
            &plan(""),
            &spec("<!-- One bullet per gate firing, appended, never rewritten. -->"),
        );
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_review_class_firing_without_counts_is_a_finding() {
        let findings = audit_with_spec(
            &plan(""),
            &spec("- 2026-01-15 · review · needs_revision · two Blockers remain"),
        );
        assert_eq!(slugs(&findings), ["plan-log-counts-missing"]);
    }

    #[test]
    fn a_custom_gate_owes_no_counts_and_may_opt_in() {
        let log = "- 2026-01-15 · security_review · needs_revision · found issues\n\
                   - 2026-01-16 · security_review · needs_revision · 0C/2B/0M/0m · two\n\
                   - 2026-01-17 · security_review · needs_revision · 0C/1B/0M/0m · one left";
        let findings = audit_with_spec(&plan(""), &spec(log));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn a_count_that_does_not_fall_escalates() {
        let log = "- 2026-01-15 · review · needs_revision · 1C/1B/0M/0m · first\n\
                   - 2026-01-16 · review · needs_revision · 0C/2B/0M/0m · level";
        let findings = audit_with_spec(&plan(""), &spec(log));
        assert_eq!(slugs(&findings), ["plan-log-not-falling"]);
        assert_eq!(findings[0].severity, Severity::Blocker);
    }

    #[test]
    fn an_acknowledged_non_falling_count_rides_on() {
        let log = "- 2026-01-15 · review · needs_revision · 1C/1B/0M/0m · first\n\
                   - 2026-01-16 · review · needs_revision · 1C/1B/0M/0m · acknowledged: \
                   operator judged the scope split worth another round";
        assert!(audit_with_spec(&plan(""), &spec(log)).is_empty());
    }

    #[test]
    fn a_falling_count_converges_and_an_approval_resets_the_cycle() {
        let log = "- 2026-01-15 · review · needs_revision · 2C/1B/0M/0m · first\n\
                   - 2026-01-16 · review · needs_revision · 0C/1B/0M/0m · falling\n\
                   - 2026-01-17 · review · approved · 0C/0B/2M/1m · clean\n\
                   - 2026-02-01 · review · needs_revision · 0C/1B/0M/0m · new cycle";
        assert!(audit_with_spec(&plan(""), &spec(log)).is_empty());
    }

    #[test]
    fn two_gates_converge_independently() {
        let log = "- 2026-01-15 · design_review · needs_revision · 0C/2B/0M/0m · plan\n\
                   - 2026-01-16 · review · needs_revision · 0C/2B/0M/0m · diff";
        assert!(audit_with_spec(&plan(""), &spec(log)).is_empty());
    }

    #[test]
    fn a_missing_count_does_not_launder_the_comparison() {
        let log = "- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two\n\
                   - 2026-01-16 · review · needs_revision · forgot the counts\n\
                   - 2026-01-17 · review · needs_revision · 0C/2B/0M/0m · still two";
        let findings = audit_with_spec(&plan(""), &spec(log));
        assert_eq!(
            slugs(&findings),
            ["plan-log-counts-missing", "plan-log-not-falling"]
        );
    }

    #[test]
    fn approved_with_nonzero_blocking_counts_is_a_contradiction() {
        let findings = audit_with_spec(
            &plan(""),
            &spec("- 2026-01-15 · review · approved · 1C/0B/0M/0m · oops"),
        );
        assert_eq!(slugs(&findings), ["plan-log-approved-nonzero"]);
    }

    #[test]
    fn an_approval_cannot_stand_over_open_blocking_rows() {
        let findings = audit_with_spec(
            &plan("- [Blocker] unhandled rollback"),
            &spec("- 2026-01-15 · review · approved · 0C/0B/0M/0m · clean"),
        );
        assert_eq!(
            slugs(&findings),
            ["plan-open-blocker", "plan-log-approved-open"]
        );
    }

    #[test]
    fn a_later_needs_revision_makes_open_rows_expected() {
        let log = "- 2026-01-15 · review · approved · 0C/0B/0M/0m · clean\n\
                   - 2026-01-20 · review · needs_revision · 0C/1B/0M/0m · regression";
        let findings = audit_with_spec(&plan("- [Blocker] regression"), &spec(log));
        assert_eq!(slugs(&findings), ["plan-open-blocker"]);
    }

    #[test]
    fn a_non_review_approval_after_the_review_does_not_hide_the_contradiction() {
        let log = "- 2026-01-15 · review · approved · 0C/0B/0M/0m · clean\n\
                   - 2026-01-16 · resume · approved · kept going";
        let findings = audit_with_spec(&plan("- [Critical] live"), &spec(log));
        assert_eq!(
            slugs(&findings),
            ["plan-open-blocker", "plan-log-approved-open"]
        );
    }

    // ---- vanish ----

    #[test]
    fn a_surviving_row_and_a_disposed_row_both_satisfy_the_baseline() {
        let baseline = plan("- [Major] naming drifts\n- [Minor] comment restates");
        let current = plan(
            "- [Major] naming drifts [fixed: renamed per glossary]\n- [Minor] comment restates",
        );
        assert!(audit_against(&baseline, Some(&current)).is_empty());
    }

    #[test]
    fn a_deleted_row_of_any_severity_is_a_blocker() {
        let baseline = plan("- [Minor] a small thing");
        let findings = audit_against(&baseline, Some(&plan("")));
        assert_eq!(slugs(&findings), ["plan-row-vanished"]);
        assert_eq!(findings[0].severity, Severity::Blocker);
    }

    #[test]
    fn a_reworded_row_is_a_vanished_row() {
        let baseline = plan("- [Major] naming drifts from the glossary");
        let findings = audit_against(&baseline, Some(&plan("- [Major] naming could be better")));
        assert_eq!(slugs(&findings), ["plan-row-vanished"]);
    }

    #[test]
    fn rewrapping_a_row_is_not_rewording_it() {
        let baseline = plan("- [Major] a finding whose text wraps across lines");
        let current = plan("- [Major] a finding whose\n  text wraps across lines");
        assert!(audit_against(&baseline, Some(&current)).is_empty());
    }

    #[test]
    fn a_deleted_plan_is_witnessed_by_every_open_baseline_row() {
        let baseline = plan("- [Critical] a\n- [Minor] b [accepted: x, y]");
        let findings = audit_against(&baseline, None);
        assert_eq!(slugs(&findings), ["plan-row-vanished"]);
    }

    #[test]
    fn an_unreadable_baseline_holds_nothing() {
        let baseline = "## Outstanding issues\n\n- [Critical] x\n\n## Outstanding issues\n";
        assert!(audit_against(baseline, Some(&plan(""))).is_empty());
    }

    #[test]
    fn an_unreadable_current_section_is_its_own_finding_not_a_vanish_verdict() {
        let baseline = plan("- [Critical] x");
        let current = "## Outstanding issues\n\n```\nunclosed\n";
        let findings = audit_against(&baseline, Some(current));
        assert_eq!(slugs(&findings), ["plan-section-unreadable"]);
    }

    #[test]
    fn a_new_plan_with_no_baseline_owes_nothing() {
        let findings = PlanAuditor::new(
            &PathBuf::from("plan.md"),
            Some(&plan("- [Major] fresh finding")),
            None,
            None,
            None,
        )
        .audit();
        assert!(findings.is_empty());
    }

    // ---- the wide net over what the parser does not claim ----

    #[test]
    fn a_severity_downgrade_is_a_vanished_row() {
        // Text alone as the identity let `[Critical] x` become `[Minor] x`
        // with no disposition and no finding — the cheapest laundering.
        let baseline = plan("- [Critical] the migration drops rows");
        let findings = audit_against(&baseline, Some(&plan("- [Minor] the migration drops rows")));
        assert_eq!(slugs(&findings), ["plan-row-vanished"]);
    }

    #[test]
    fn a_row_no_parser_claims_is_loud_wherever_it_hides() {
        for row in [
            "\t- [Critical] tab-indented",
            "> - [Critical] blockquoted",
            "1234567890. [Critical] ordinal past the CommonMark limit",
            "- ［Critical］ fullwidth brackets",
        ] {
            let findings = audit(&plan(row));
            assert_eq!(slugs(&findings), ["plan-row-unparseable"], "row: {row}");
        }
        // Indented past the item margin as the section's first content line.
        let text = "## Outstanding issues\n\n    - [Critical] indented-code position\n";
        assert_eq!(slugs(&audit(text)), ["plan-row-unparseable"]);
    }

    #[test]
    fn a_nested_row_merged_into_a_parent_is_loud_not_buried() {
        for body in [
            "- [Minor] parent note\n    - [Critical] real nested finding",
            "- [Minor] parent note\n\t- [Critical] tab-nested finding",
        ] {
            let findings = audit(&plan(body));
            assert_eq!(slugs(&findings), ["plan-row-unparseable"], "body: {body}");
        }
    }

    #[test]
    fn a_stray_bracket_does_not_hide_the_marker_after_it() {
        let findings = audit(&plan("- unmatched [ then real **[Critical]** marker"));
        assert_eq!(slugs(&findings), ["plan-row-unparseable"]);
    }

    #[test]
    fn loose_prose_without_a_severity_stays_quiet() {
        let findings = audit(&plan(
            "> a blockquoted note\n\t- a tab-indented bullet naming nothing",
        ));
        assert!(findings.is_empty(), "{findings:#?}");
    }

    // ---- ATX heading spellings ----

    #[test]
    fn commonmark_heading_spellings_anchor_the_section() {
        for text in [
            "## Outstanding issues #\n\n- [Critical] behind a closing hash\n",
            "## Outstanding issues ##\n\n- [Critical] behind a closing run\n",
            "##  Outstanding issues\n\n- [Critical] behind a double space\n",
            "  ## Outstanding issues\n\n- [Critical] behind an indented heading\n",
        ] {
            assert_eq!(slugs(&audit(text)), ["plan-open-blocker"], "text: {text}");
        }
    }

    #[test]
    fn a_closing_hash_on_the_log_heading_keeps_the_accounting_alive() {
        let spec = "# t\n\n## Decision log #\n\n\
                    - 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · first\n\
                    - 2026-01-16 · review · needs_revision · 0C/2B/0M/0m · level\n";
        let findings = audit_with_spec(&plan(""), spec);
        assert_eq!(slugs(&findings), ["plan-log-not-falling"]);
    }

    #[test]
    fn a_heading_mangled_baseline_still_holds_its_rows() {
        let baseline = "## Outstanding issues ##\n\n- [Critical] committed finding\n";
        let findings = audit_against(baseline, Some(&plan("")));
        assert_eq!(slugs(&findings), ["plan-row-vanished"]);
    }

    // ---- log accounting hardening ----

    #[test]
    fn a_gate_name_case_change_does_not_reset_the_comparison() {
        let log = "- 2026-01-15 · review · needs_revision · 3C/2B/0M/0m · first\n\
                   - 2026-01-16 · Review · needs_revision · 5C/4B/0M/0m · rising";
        let findings = audit_with_spec(&plan(""), &spec(log));
        assert_eq!(slugs(&findings), ["plan-log-not-falling"]);
    }

    #[test]
    fn a_lookalike_separator_is_loud_not_dropped() {
        let findings = audit_with_spec(
            &plan(""),
            &spec(
                "- 2026-01-15 \u{2022} review \u{2022} approved \u{2022} 1C/0B/0M/0m \u{2022} fine",
            ),
        );
        assert_eq!(slugs(&findings), ["plan-log-unparseable"]);
    }

    // ---- the log's append-only baseline ----

    #[test]
    fn an_appended_log_satisfies_its_baseline() {
        let held = spec("- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two");
        let current = spec(
            "- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two\n\
             - 2026-01-16 · review · approved · 0C/0B/0M/0m · clean",
        );
        assert!(audit_log_against(&held, &current).is_empty());
    }

    #[test]
    fn an_edited_committed_bullet_is_a_rewritten_log() {
        let held = spec("- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two");
        let current = spec("- 2026-01-15 · review · needs_revision · 0C/9B/0M/0m · two");
        let findings = audit_log_against(&held, &current);
        assert!(
            slugs(&findings).contains(&"plan-log-rewritten"),
            "{findings:#?}"
        );
    }

    #[test]
    fn a_removed_committed_bullet_is_a_rewritten_log() {
        let held = spec(
            "- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two\n\
             - 2026-01-16 · review · needs_revision · 0C/1B/0M/0m · one",
        );
        let current = spec("- 2026-01-15 · review · needs_revision · 0C/2B/0M/0m · two");
        let findings = audit_log_against(&held, &current);
        assert!(
            slugs(&findings).contains(&"plan-log-rewritten"),
            "{findings:#?}"
        );
    }

    #[test]
    fn an_unreadable_baseline_log_holds_nothing() {
        let held = "# t\n\n## Decision log\n\n```\nunclosed\n";
        let current = spec("- 2026-01-15 · review · approved · 0C/0B/0M/0m · clean");
        assert!(audit_log_against(held, &current).is_empty());
    }
}
