//! # spec — when each closed vocabulary was last measured against the docs
//!
//! Every `KNOWN_*` set in this crate mirrors a surface of the Claude Code
//! spec, and the tests that guard them compare each set to the plugin's
//! reference doc — a closed loop, because both sides are ours. Nothing in the
//! repository compares either side to the upstream documentation, so the two
//! drift together and the suite stays green while doing it. That is the
//! failure harnex exists to prevent, and it has landed twice: a hook event
//! and four skill keys shipped absent, each turning a correct harness into a
//! finding.
//!
//! A stamp closes the loop without a network call. Each surface records the
//! date it was verified and a digest of exactly what was verified. The digest
//! is what makes the date honest: `spec_stamps_match_live_vocabularies` fails
//! the build when a set changes without its stamp, so a stamp cannot quietly
//! describe a vocabulary it no longer covers. Staleness is then a date
//! comparison — it reports that an answer is old, never that it is wrong,
//! which is the only claim available offline.
//!
//! ## What this module refuses to do
//!
//! - Never fetch. Article I keeps the network out of command time; a
//!   scheduled job may fetch and open a pull request, and this module reports
//!   the age of what that job last landed.
//! - Never parse a rendered doc to re-derive a set. Extraction from prose has
//!   a false-positive floor, and a wrong auto-update is worse than a stale
//!   one that says so.
//! - Never gate, and never become a finding. Staleness is a property of this
//!   binary, not of the project a command was pointed at, so it rides the
//!   envelope's `warnings[]` on every command. As a finding it would both
//!   misattribute the problem and make a zero-findings assertion fail on a
//!   calendar with no code change behind it.

use jiff::Unit;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::envelope::Warning;

use crate::validate::{agents, output_styles, settings, skills};

/// Age past which a surface is reported unverified. Generous on purpose: the
/// warning asks for a re-read, and one that fires monthly trains the reader to
/// dismiss it.
pub const MAX_AGE_DAYS: i64 = 90;

/// Envelope warning code for a vocabulary past [`MAX_AGE_DAYS`]. Stable: a
/// consumer filters on it.
pub const STALE_WARNING_CODE: &str = "spec-stamp-stale";

/// One Claude Code documentation surface and the vocabulary read from it.
#[derive(Debug, Clone, Copy)]
pub struct SpecSurface {
    /// Stable identifier, reported in findings.
    pub name: &'static str,
    /// Documentation page the vocabulary was read from.
    pub doc: &'static str,
    /// ISO date the vocabulary was last checked against that page.
    pub measured: &'static str,
    /// Digest of the vocabulary as measured. Held equal to the live constants
    /// by a test, so editing a set without re-measuring fails the build.
    pub digest: u64,
}

impl SpecSurface {
    pub const ALL: &'static [Self] = &[
        Self {
            name: "hooks",
            doc: "/en/hooks",
            measured: "2026-08-17",
            digest: 0x115e_9208_4732_18a3,
        },
        Self {
            name: "skills",
            doc: "/en/skills",
            measured: "2026-08-17",
            digest: 0x7eef_4d1d_783a_4980,
        },
        Self {
            name: "sub-agents",
            doc: "/en/sub-agents",
            measured: "2026-08-17",
            digest: 0xd562_4c75_b2f4_6403,
        },
        Self {
            name: "output-styles",
            doc: "/en/output-styles",
            measured: "2026-08-17",
            digest: 0x87fa_1462_4b13_6476,
        },
        Self {
            name: "settings",
            doc: "/en/settings",
            measured: "2026-08-17",
            digest: 0xd0c3_9c78_19f1_90e2,
        },
    ];

    /// Every closed set this surface stamps, labelled, as the owning validator
    /// declares them beside its constants.
    ///
    /// That a *new* constant reaches its module's `SPEC_SETS` is
    /// discipline-held. Two mechanisms could bind it and neither is worth its
    /// price. A scan of this crate's own source is forbidden outright by
    /// `keep-soften-cut` — a pattern match over source has a false-positive
    /// floor, and `DANGEROUS_ALLOW_BASES` is a live instance of what it would
    /// misread. Deriving each constant from its `SPEC_SETS` row instead works
    /// and costs nothing at runtime, but it puts every vocabulary one hop from
    /// its values — and the activity these constants exist for is a person
    /// comparing them line by line against a documentation page. Taxing the
    /// reading to defend the registration is the wrong trade, the more so
    /// because an author can still write a bare constant and bypass it.
    ///
    /// So the declaration buys visibility, not enforcement: an omission shows
    /// up in one place per module instead of only by reading every check.
    pub fn vocabulary(&self) -> &'static [(&'static str, &'static [&'static str])] {
        const NONE: &[(&str, &[&str])] = &[];
        match self.name {
            "hooks" => settings::HOOK_SPEC_SETS,
            "skills" => skills::SPEC_SETS,
            "sub-agents" => agents::SPEC_SETS,
            "output-styles" => output_styles::SPEC_SETS,
            "settings" => settings::SPEC_SETS,
            _ => NONE,
        }
    }

    /// Digest of the live vocabulary, for comparison against [`Self::digest`].
    pub fn live_digest(&self) -> u64 {
        digest(self.vocabulary())
    }

    /// Whole days between the measurement and `today`. Negative when the
    /// stamp is dated ahead of the clock.
    ///
    /// The largest unit is stated rather than inherited: a span that balanced
    /// into months would report only its day component, and the check would
    /// then read a year-old stamp as days old.
    pub fn age_days(&self, today: Date) -> Option<i64> {
        let measured = Date::strptime("%Y-%m-%d", self.measured).ok()?;
        measured
            .until((Unit::Day, today))
            .ok()
            .map(|s| i64::from(s.get_days()))
    }
}

/// FNV-1a over every labelled set.
///
/// Hand-rolled because the digest is committed to source: `DefaultHasher` is
/// explicitly unstable across releases, so a toolchain bump would rewrite
/// every stamp and the guard would read as drift.
///
/// The label is hashed with its values so a set boundary is visible: chaining
/// the values alone would let a value move from one set to another — a real
/// spec change — without moving the digest. A separator keeps `["ab", "c"]`
/// and `["a", "bc"]` distinct.
pub fn digest(sets: &[(&str, &[&str])]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    const UNIT: u8 = 0x1f;
    const GROUP: u8 = 0x1d;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8], sep: u8| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= u64::from(sep);
        hash = hash.wrapping_mul(PRIME);
    };
    for (label, values) in sets {
        feed(label.as_bytes(), GROUP);
        for value in *values {
            feed(value.as_bytes(), UNIT);
        }
    }
    hash
}

/// Warnings for every vocabulary past [`MAX_AGE_DAYS`] as of `today`.
pub fn stale_warnings(today: Date) -> Vec<Warning> {
    SpecSurface::ALL
        .iter()
        .filter_map(|surface| {
            let age = surface.age_days(today)?;
            (age > MAX_AGE_DAYS).then(|| Warning {
                code: STALE_WARNING_CODE.to_string(),
                message: format!(
                    "the '{}' vocabulary was last read from {} on {} — {age} days ago, past the \
                     {MAX_AGE_DAYS}-day window. It is unverified, not known wrong; re-read the \
                     page, then update that surface's `measured` and `digest` in `spec.rs`",
                    surface.name, surface.doc, surface.measured
                ),
            })
        })
        .collect()
}

/// Warnings as of the current UTC day — what every command attaches.
pub fn stale_warnings_now() -> Vec<Warning> {
    stale_warnings(jiff::Timestamp::now().to_zoned(TimeZone::UTC).date())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_date(name: &str) -> Date {
        let surface = SpecSurface::ALL
            .iter()
            .find(|s| s.name == name)
            .expect("surface exists");
        Date::strptime("%Y-%m-%d", surface.measured).unwrap()
    }

    #[test]
    fn no_warning_on_the_day_of_measurement() {
        assert!(stale_warnings(measured_date("hooks")).is_empty());
    }

    #[test]
    fn no_warning_on_the_boundary_day() {
        let today = measured_date("hooks")
            .checked_add(jiff::Span::new().days(MAX_AGE_DAYS))
            .unwrap();
        assert!(
            stale_warnings(today).is_empty(),
            "the boundary day is still inside the window"
        );
    }

    #[test]
    fn every_surface_warns_once_past_the_window() {
        let today = SpecSurface::ALL
            .iter()
            .filter_map(|s| Date::strptime("%Y-%m-%d", s.measured).ok())
            .max()
            .unwrap()
            .checked_add(jiff::Span::new().days(MAX_AGE_DAYS + 1))
            .unwrap();
        let warnings = stale_warnings(today);
        assert_eq!(warnings.len(), SpecSurface::ALL.len());
        assert!(warnings.iter().all(|w| w.code == STALE_WARNING_CODE));
    }

    #[test]
    fn the_now_helper_agrees_with_the_tested_function() {
        // `stale_warnings_now` is what every command calls; the clock read is
        // the only thing it adds, so pin it to the function the cases above
        // exercise rather than leaving the wiring untested.
        let today = jiff::Timestamp::now().to_zoned(TimeZone::UTC).date();
        assert_eq!(
            stale_warnings_now().len(),
            stale_warnings(today).len(),
            "the command-facing helper must report what the tested function does"
        );
    }

    #[test]
    fn a_stamp_dated_ahead_of_the_clock_does_not_warn() {
        let today = measured_date("hooks")
            .checked_sub(jiff::Span::new().days(10))
            .unwrap();
        assert!(stale_warnings(today).is_empty());
    }

    #[test]
    fn digest_separates_values_that_concatenate_alike() {
        assert_ne!(
            digest(&[("s", &["ab", "c"])]),
            digest(&[("s", &["a", "bc"])])
        );
    }

    #[test]
    fn digest_is_order_sensitive() {
        assert_ne!(digest(&[("s", &["a", "b"])]), digest(&[("s", &["b", "a"])]));
    }

    #[test]
    fn digest_sees_a_value_moving_across_a_set_boundary() {
        // Chaining the values alone would hide this: both spellings flatten to
        // the same sequence, yet one is a real spec change.
        assert_ne!(
            digest(&[("one", &["a", "b", "x"]), ("two", &["m"])]),
            digest(&[("one", &["a", "b"]), ("two", &["x", "m"])])
        );
    }

    #[test]
    fn digest_sees_a_set_being_renamed() {
        assert_ne!(digest(&[("one", &["a"])]), digest(&[("uno", &["a"])]));
    }

    #[test]
    fn every_surface_names_a_non_empty_vocabulary() {
        for surface in SpecSurface::ALL {
            assert!(
                !surface.vocabulary().is_empty(),
                "surface '{}' stamps nothing — its match arm is missing, and a digest over an \
                 empty set would report fresh forever",
                surface.name
            );
            for (label, values) in surface.vocabulary() {
                assert!(
                    !values.is_empty(),
                    "surface '{}' stamps an empty set '{label}'",
                    surface.name
                );
            }
        }
    }

    #[test]
    fn every_measured_date_parses() {
        let today = Date::constant(2026, 8, 17);
        for surface in SpecSurface::ALL {
            assert!(
                surface.age_days(today).is_some(),
                "surface '{}' has an unparseable `measured` date '{}'",
                surface.name,
                surface.measured
            );
        }
    }

    #[test]
    fn age_counts_whole_days_across_month_and_year_boundaries() {
        let surface = SpecSurface::ALL[0];
        let measured = Date::strptime("%Y-%m-%d", surface.measured).unwrap();
        // A span balanced into months would report a day component near zero
        // here; the check must see the whole elapsed distance.
        for days in [45_i64, 200, 400, 1000] {
            let today = measured.checked_add(jiff::Span::new().days(days)).unwrap();
            assert_eq!(
                surface.age_days(today),
                Some(days),
                "age across {days} days must count days, not a balanced span"
            );
        }
    }

    #[test]
    fn age_counts_whole_days_from_the_measurement() {
        let surface = SpecSurface::ALL[0];
        let measured = Date::strptime("%Y-%m-%d", surface.measured).unwrap();
        assert_eq!(surface.age_days(measured), Some(0));
        assert_eq!(
            surface.age_days(measured.tomorrow().unwrap()),
            Some(1),
            "one day after the measurement is one day old"
        );
    }
}
