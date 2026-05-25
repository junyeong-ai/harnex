//! Verifier strategy: fetched URL with explicit fetch date.
//!
//! Validates that:
//! 1. The URL parses as a valid URL.
//! 2. The `[fetched: YYYY-MM-DD]` marker carries a parseable date.
//! 3. The date is not in the future.
//! 4. The date is within `max_age_days` of today (UTC).
//!
//! Never makes a network call. Freshness is a contract the author asserts;
//! verification checks the contract's structure, not the URL's contents.

use std::path::Path;

use jiff::{Timestamp, Unit, civil::Date, tz::TimeZone};
use url::Url;

use super::{Claim, ClaimKind, Verifier, VerifyOutcome};

pub(crate) struct FetchedUrlVerifier {
    provenance: String,
    max_age_days: u32,
}

impl FetchedUrlVerifier {
    pub(crate) fn new(provenance: String, max_age_days: u32) -> Self {
        Self {
            provenance,
            max_age_days,
        }
    }
}

impl Verifier for FetchedUrlVerifier {
    fn provenance(&self) -> &str {
        &self.provenance
    }

    fn verify(&self, claim: &Claim, _working_dir: &Path) -> VerifyOutcome {
        let (url, fetched_date) = match &claim.kind {
            ClaimKind::Url { url, fetched_date } => (url, fetched_date),
            _ => {
                return VerifyOutcome::Violation {
                    message: format!(
                        "provenance '{}' expects a `[fetched: YYYY-MM-DD] URL` claim shape",
                        self.provenance
                    ),
                    hint: Some(
                        "use [fetched: YYYY-MM-DD] https://… syntax for URL citations".into(),
                    ),
                };
            }
        };

        if Url::parse(url).is_err() {
            return VerifyOutcome::Violation {
                message: format!("URL '{url}' does not parse"),
                hint: Some("ensure scheme + host are present (https://example.com/path)".into()),
            };
        }

        let date_str = match fetched_date {
            Some(d) => d,
            None => {
                return VerifyOutcome::Violation {
                    message: "fetched URL claim missing fetched date".into(),
                    hint: Some("prefix the URL with [fetched: YYYY-MM-DD]".into()),
                };
            }
        };

        let date = match Date::strptime("%Y-%m-%d", date_str) {
            Ok(d) => d,
            Err(_) => {
                return VerifyOutcome::Violation {
                    message: format!("fetched date '{date_str}' is not YYYY-MM-DD"),
                    hint: Some("use ISO-8601 date format: [fetched: 2026-05-22]".into()),
                };
            }
        };

        let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
        let span = match date.until((Unit::Day, today)) {
            Ok(s) => s,
            Err(e) => {
                return VerifyOutcome::Violation {
                    message: format!("date arithmetic failed: {e}"),
                    hint: Some(
                        "ensure the fetched date is a valid calendar date (not Feb 30, etc.)"
                            .into(),
                    ),
                };
            }
        };
        let age_days: i64 = span.get_days() as i64;

        if age_days < 0 {
            return VerifyOutcome::Violation {
                message: format!("fetched date '{date_str}' is in the future"),
                hint: Some(
                    "the fetched date must be today or earlier (check system clock or typo)".into(),
                ),
            };
        }
        if (age_days as u64) > self.max_age_days as u64 {
            return VerifyOutcome::Violation {
                message: format!(
                    "fetched citation is {age_days} days old (max {})",
                    self.max_age_days
                ),
                hint: Some("re-fetch the source and update the date".into()),
            };
        }

        VerifyOutcome::Ok
    }
}
