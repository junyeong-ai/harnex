//! # wire_enum — a closed set declared once
//!
//! A closed set that crosses the wire needs its variants, its strings, the
//! list something iterates, and the parse back. The compiler forces only the
//! exhaustive match: a variant left out of a hand-written `ALL` still
//! serialises through `as_str`, while whatever was built from `ALL` — a
//! schema, a measurement, a validator's check set — does not know it exists,
//! and nothing fails. Declaring all four from one list removes that state
//! rather than testing for it.
//!
//! `from_str` is derived from `ALL` rather than written as a second match, so
//! the round trip holds by construction.

/// Declares a closed wire enum: variants, [`ALL`](Self::ALL), `as_str` and
/// `from_str` from one list.
macro_rules! wire_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident => $wire:literal),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The wire string, exhaustive over the declaration that built
            /// [`Self::ALL`], so the two cannot disagree.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }

            /// The inverse of [`Self::as_str`], derived from [`Self::ALL`]
            /// rather than kept in step by hand.
            pub fn from_str(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == s)
            }
        }
    };
}

pub(crate) use wire_enum;

#[cfg(test)]
mod tests {
    wire_enum! {
        /// Two variants and a doc on each, which is the shape both call sites
        /// use.
        pub enum Sample {
            /// First.
            One => "one",
            /// Second.
            Two => "two-wire",
        }
    }

    #[test]
    fn every_variant_reaches_all_and_round_trips_through_its_wire_string() {
        assert_eq!(Sample::ALL, &[Sample::One, Sample::Two]);
        for variant in Sample::ALL {
            assert_eq!(Sample::from_str(variant.as_str()), Some(*variant));
        }
        assert_eq!(
            Sample::from_str("One"),
            None,
            "the wire string, not the ident"
        );
        assert_eq!(Sample::Two.as_str(), "two-wire");
    }
}
