//! # routines — scheduled harness tasks whose record is a file
//!
//! A routine (`.claude/routines/<slug>.md`) gives a recurring harness task a
//! cadence and a record: `when` is the next due date, `produces` the file
//! whose presence IS completion, `prompt` the work, and the body the record.
//! [`states`] computes where each stands against a date the caller supplies;
//! the shape gate is [`crate::validate::RoutineValidator`], which consumes
//! [`RoutineDecl::from_file`].
//!
//! Scheduling the next tick is deliberately manual — an auto-derived date
//! would drift the cadence toward whenever the last run happened to land. A
//! retired routine keeps `status: superseded` in place as its own record.
//!
//! ## What this module refuses to do
//!
//! - **Never gate on the calendar.** Overdue is schedule state, not a
//!   defect: it surfaces through the query and the session surface, never
//!   through `check` — a finding that appears by clock alone would make
//!   every gate's answer a function of the day it runs.
//! - **Never derive the next tick.** Completion dates are when work
//!   happened; cadence is when it should.
//! - **Never run the prompt.** The work is the operator's session's; this
//!   module answers only what is due.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::validate::frontmatter;
use crate::wire_enum::wire_enum;

wire_enum! {
    /// Where a routine stands, computed against a supplied date.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum RoutineState {
        /// The declared record exists.
        Produced => "produced",
        /// Due date ahead, record not yet produced.
        Scheduled => "scheduled",
        /// Due date passed with no record.
        Overdue => "overdue",
        /// `when` or `produces` is undeclared — installed but never given
        /// its first tick, which the session surface reports loudly.
        Unscheduled => "unscheduled",
        /// Retired in place; the file stays as the record.
        Superseded => "superseded",
    }
}

/// A routine's parsed frontmatter. Closed schema: this grammar is wholly
/// this toolkit's, so an unknown key is a typo, never an extension.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RoutineDecl {
    /// Next due date, `YYYY-MM-DD`. Absent = never scheduled.
    pub when: Option<String>,
    /// The human rhythm, in prose (`quarterly`, `each release`).
    pub cadence: String,
    /// The record whose presence is completion, project-relative. Absent =
    /// never scheduled.
    pub produces: Option<String>,
    /// Who answers for it.
    pub owner: String,
    /// The work, addressed to the session that picks it up.
    pub prompt: String,
    /// `active` (default) or `superseded`.
    pub status: RoutineStatus,
}

wire_enum! {
    /// Declared lifecycle of the routine file itself.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum RoutineStatus {
        Active => "active",
        Superseded => "superseded",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoutine {
    #[serde(default)]
    when: Option<String>,
    cadence: String,
    #[serde(default)]
    produces: Option<String>,
    owner: String,
    prompt: String,
    #[serde(default)]
    status: Option<String>,
}

/// How a routine file failed to declare. The validator maps each to a
/// finding.
#[derive(Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// No frontmatter, unterminated frontmatter, or a shape the closed
    /// schema refuses.
    Malformed(String),
    /// `when` is not a `YYYY-MM-DD` date.
    BadDate(String),
    /// `produces` is not a literal project-relative path.
    BadPath(String),
    /// `status` is outside the closed set.
    BadStatus(String),
    /// A required field is empty.
    Empty(&'static str),
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(m) => write!(f, "routine frontmatter does not parse: {m}"),
            Self::BadDate(d) => write!(f, "`when: {d}` is not a YYYY-MM-DD date"),
            Self::BadPath(p) => {
                write!(f, "`produces: {p}` is not a literal project-relative path")
            }
            Self::BadStatus(s) => write!(
                f,
                "`status: {s}` is outside {}",
                RoutineStatus::ALL
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Self::Empty(field) => write!(f, "`{field}` is empty"),
        }
    }
}

impl RoutineDecl {
    /// The declaration in a routine file's text.
    pub fn from_file(content: &str, source: &Path) -> std::result::Result<Self, ShapeError> {
        let fm = frontmatter::parse(content, source)
            .map_err(|e| ShapeError::Malformed(e.to_string()))?
            .ok_or_else(|| {
                ShapeError::Malformed("a routine is its frontmatter; none is present".into())
            })?;
        let raw: RawRoutine = yaml_serde::from_str(&fm.yaml_text)
            .map_err(|e| ShapeError::Malformed(e.to_string()))?;
        if let Some(when) = &raw.when
            && jiff::civil::Date::strptime("%Y-%m-%d", when).is_err()
        {
            return Err(ShapeError::BadDate(when.clone()));
        }
        if let Some(produces) = &raw.produces
            && !crate::path_guard::literal_relative(produces)
        {
            return Err(ShapeError::BadPath(produces.clone()));
        }
        let status = match &raw.status {
            None => RoutineStatus::Active,
            Some(s) => {
                RoutineStatus::from_str(s).ok_or_else(|| ShapeError::BadStatus(s.clone()))?
            }
        };
        for (field, value) in [
            ("cadence", &raw.cadence),
            ("owner", &raw.owner),
            ("prompt", &raw.prompt),
        ] {
            if value.trim().is_empty() {
                return Err(ShapeError::Empty(field));
            }
        }
        Ok(Self {
            when: raw.when,
            cadence: raw.cadence,
            produces: raw.produces,
            owner: raw.owner,
            prompt: raw.prompt.trim_end().to_string(),
            status,
        })
    }

    /// Where this routine stands on `today`.
    pub fn state(&self, root: &Path, today: jiff::civil::Date) -> RoutineState {
        if self.status == RoutineStatus::Superseded {
            return RoutineState::Superseded;
        }
        let (Some(when), Some(produces)) = (&self.when, &self.produces) else {
            return RoutineState::Unscheduled;
        };
        if root.join(produces).exists() {
            return RoutineState::Produced;
        }
        // Shape-validated at parse; a decl that reaches here parses.
        match jiff::civil::Date::strptime("%Y-%m-%d", when) {
            Ok(due) if due < today => RoutineState::Overdue,
            _ => RoutineState::Scheduled,
        }
    }
}

/// One routine as the query reports it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RoutineReport {
    /// The file's stem under `.claude/routines/`.
    pub slug: String,
    pub state: RoutineState,
    pub when: Option<String>,
    pub cadence: String,
    pub produces: Option<String>,
    pub owner: String,
    /// Present so the session that picks an overdue routine up has the work
    /// in hand.
    pub prompt: String,
}

/// Every routine under `root`, with its state on `today`. A file whose shape
/// is broken is an error here — the validator names the defect, and a query
/// that skipped it silently would report a schedule with a hole.
pub fn states(root: &Path, today: jiff::civil::Date) -> Result<Vec<RoutineReport>> {
    use crate::validate::{RoutineValidator, SurfaceValidator};
    let pattern = crate::glob_root::rooted(root, <RoutineValidator as SurfaceValidator>::GLOB)?;
    let mut out = Vec::new();
    let mut paths: Vec<_> = Vec::new();
    for entry in glob::glob(&pattern).map_err(|e| Error::ConfigInvalid {
        message: format!("routines glob: {e}"),
        location: None,
    })? {
        paths.push(entry.map_err(|e| {
            let path = e.path().to_path_buf();
            Error::IoFailure {
                path,
                source: e.into(),
            }
        })?);
    }
    paths.sort();
    for path in paths {
        let content = std::fs::read_to_string(&path).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: e,
        })?;
        let decl = RoutineDecl::from_file(&content, &path).map_err(|e| Error::ConfigInvalid {
            message: format!("{}: {e}", path.display()),
            location: None,
        })?;
        let slug = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(RoutineReport {
            slug,
            state: decl.state(root, today),
            when: decl.when,
            cadence: decl.cadence,
            produces: decl.produces,
            owner: decl.owner,
            prompt: decl.prompt,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rename_all = "kebab-case"` spells the wire string a second time; this
    /// holds the two spellings equal, per the wire_enum convention.
    #[test]
    fn serde_and_wire_spellings_agree() {
        for state in RoutineState::ALL {
            let json = serde_json::to_string(state).unwrap();
            assert_eq!(json, format!("\"{}\"", state.as_str()));
        }
        for status in RoutineStatus::ALL {
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(json, format!("\"{}\"", status.as_str()));
        }
    }

    fn date(s: &str) -> jiff::civil::Date {
        jiff::civil::Date::strptime("%Y-%m-%d", s).unwrap()
    }

    fn routine(extra: &str) -> String {
        format!(
            "---\ncadence: quarterly\nowner: harness\nprompt: run the pass\n{extra}---\nrecord\n"
        )
    }

    #[test]
    fn every_state_is_reachable_and_deterministic() {
        let root = tempfile::tempdir().unwrap();
        let today = date("2026-09-01");
        let cases = [
            (
                "when: 2026-10-01\nproduces: out/q4.md\n",
                RoutineState::Scheduled,
            ),
            (
                "when: 2026-08-01\nproduces: out/q3.md\n",
                RoutineState::Overdue,
            ),
            ("", RoutineState::Unscheduled),
            ("when: 2026-08-01\n", RoutineState::Unscheduled),
            (
                "when: 2026-08-01\nproduces: out/q3.md\nstatus: superseded\n",
                RoutineState::Superseded,
            ),
        ];
        for (extra, expect) in cases {
            let decl = RoutineDecl::from_file(&routine(extra), Path::new("r.md")).unwrap();
            assert_eq!(decl.state(root.path(), today), expect, "{extra}");
        }
        std::fs::create_dir_all(root.path().join("out")).unwrap();
        std::fs::write(root.path().join("out/q3.md"), "done").unwrap();
        let done = RoutineDecl::from_file(
            &routine("when: 2026-08-01\nproduces: out/q3.md\n"),
            Path::new("r.md"),
        )
        .unwrap();
        assert_eq!(done.state(root.path(), today), RoutineState::Produced);
    }

    #[test]
    fn each_shape_deviation_is_its_own_error() {
        for (extra, expect) in [
            ("when: next-quarter\nproduces: out.md\n", "YYYY-MM-DD"),
            ("when: 2026-10-01\nproduces: /abs\n", "project-relative"),
            ("status: retired\n", "outside"),
            ("unknown_key: x\n", "does not parse"),
        ] {
            let err = RoutineDecl::from_file(&routine(extra), Path::new("r.md")).unwrap_err();
            assert!(err.to_string().contains(expect), "{extra}: {err}");
        }
        let empty = "---\ncadence: quarterly\nowner: harness\nprompt: \"  \"\n---\n";
        let err = RoutineDecl::from_file(empty, Path::new("r.md")).unwrap_err();
        assert_eq!(err, ShapeError::Empty("prompt"));
        assert!(
            RoutineDecl::from_file("no frontmatter\n", Path::new("r.md")).is_err(),
            "a routine is its frontmatter"
        );
    }

    #[test]
    fn states_reads_the_tree_and_refuses_a_broken_schedule() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".claude/routines");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("curate.md"),
            routine("when: 2026-08-01\nproduces: out.md\n"),
        )
        .unwrap();
        std::fs::write(dir.join("census.md"), routine("")).unwrap();
        let reports = states(root.path(), date("2026-09-01")).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].slug, "census");
        assert_eq!(reports[0].state, RoutineState::Unscheduled);
        assert_eq!(reports[1].state, RoutineState::Overdue);

        std::fs::write(dir.join("broken.md"), "---\ncadence: q\n---\n").unwrap();
        assert!(states(root.path(), date("2026-09-01")).is_err());
    }
}
