//! # ask — a decision put to the person on a page, and the answers that come back
//!
//! A session that needs a person's choice writes a page and an asks file
//! ([`asks::Asks`]): what is asked, the answers offered for each, and the
//! caller's version of what each ask showed. The answers come back as
//! [`answers::Answer`] records carrying that version and everything the ask
//! showed beside it, so [`current::current`] can tell later — against the asks
//! as they read then — which answers still hold before any is recorded.
//!
//! The contract is the transport's to honor and not its to define: an answer
//! record from any transport that fills the same shape is checked the same way.
//! [`words`] holds what harnex itself says around a page, per locale.
//!
//! ## What this module refuses to do
//!
//! - Never read a version. What an ask's content is — a section, a table row,
//!   an expected result — is known only to the caller that fingerprints it, so
//!   the version is carried and compared, never computed or interpreted here.
//! - Never judge an answer by the page. Anything on the machine can post to a
//!   served page, so an answer set is read against the asks file alone.
//! - Never take part of a set asked together, and never let an answer stand on
//!   a version, label or offer that moved.

pub mod answers;
pub mod asks;
pub mod current;
pub mod words;
