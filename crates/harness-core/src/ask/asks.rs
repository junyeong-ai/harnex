//! The asks file: what a page puts to the person, read against a closed schema.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::wire_enum::wire_enum;

wire_enum! {
    /// Whether an answer takes a line of the person's own words, and whether
    /// it may be left empty.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum NoteRule {
        None => "none",
        Optional => "optional",
        Required => "required",
    }
}

wire_enum! {
    /// The language everything harnex itself says on a page is in. The page's
    /// own words are the page's.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum Locale {
        #[default]
        En => "en",
        Ko => "ko",
    }
}

/// One answer a page offers for an ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Offered {
    pub name: String,
    pub note: NoteRule,
}

/// One thing a page asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// What the answer is kept under. Compared byte for byte.
    pub id: String,
    /// What the answer is called when it is named back to the person.
    pub label: String,
    /// The caller's fingerprint of what this ask showed. Returned with the
    /// answer unread, so the caller can tell later whether it still holds.
    pub version: String,
    pub answers: Vec<Offered>,
}

/// Asks answered all at once or not at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Together {
    pub label: String,
    pub ids: Vec<String>,
}

/// An asks file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Asks {
    #[serde(default)]
    pub locale: Locale,
    /// Files the page was made from. An answer set arriving after one of them
    /// changed is refused as stale; empty asks nothing of the files.
    #[serde(default)]
    pub sources: Vec<String>,
    pub asks: Vec<Ask>,
    #[serde(default)]
    pub together: Vec<Together>,
}

/// The longest id, version or answer name taken. Well past a fingerprint or a
/// name a person writes, and short enough that no page carries one by mistake.
const NAME_LIMIT: usize = 200;

impl Asks {
    /// Read and check the asks file at `path`.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::IoFailure {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text).map_err(|message| Error::AskInputInvalid {
            path: path.to_path_buf(),
            message,
        })
    }

    /// Parse and check an asks file's text; the error says what is wrong in
    /// the file's own terms.
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        let asks: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        asks.check()?;
        Ok(asks)
    }

    fn check(&self) -> std::result::Result<(), String> {
        for (i, source) in self.sources.iter().enumerate() {
            if source.is_empty() {
                return Err(format!("`sources[{i}]` is empty"));
            }
        }
        if self.asks.is_empty() {
            return Err("`asks` names nothing to ask".into());
        }
        let mut ids = BTreeSet::new();
        for (i, ask) in self.asks.iter().enumerate() {
            let at = format!("`asks[{i}]`");
            name(&ask.id, &format!("{at}.id"))?;
            name(&ask.version, &format!("{at}.version"))?;
            text(&ask.label, &format!("{at}.label"))?;
            if !ids.insert(ask.id.as_str()) {
                return Err(format!("{at}.id `{}` is asked twice", ask.id));
            }
            if ask.answers.len() < 2 {
                return Err(format!("{at} offers fewer than two answers"));
            }
            let mut names = BTreeSet::new();
            for (j, offered) in ask.answers.iter().enumerate() {
                name(&offered.name, &format!("{at}.answers[{j}].name"))?;
                if !names.insert(offered.name.as_str()) {
                    return Err(format!("{at} offers `{}` twice", offered.name));
                }
            }
        }
        let mut grouped = BTreeSet::new();
        for (i, set) in self.together.iter().enumerate() {
            let at = format!("`together[{i}]`");
            text(&set.label, &format!("{at}.label"))?;
            if set.ids.len() < 2 {
                return Err(format!("{at} holds fewer than two asks"));
            }
            for id in &set.ids {
                if !ids.contains(id.as_str()) {
                    return Err(format!("{at} names `{id}`, which is not asked"));
                }
                if !grouped.insert(id.as_str()) {
                    return Err(format!(
                        "`{id}` is named twice among the sets asked together"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The ask kept under `id`.
    pub fn ask(&self, id: &str) -> Option<&Ask> {
        self.asks.iter().find(|ask| ask.id == id)
    }
}

/// An id, a version or an answer name: what is compared byte for byte, so
/// nothing that reads the same and compares different — surrounding space, a
/// control character — is taken.
fn name(value: &str, at: &str) -> std::result::Result<(), String> {
    if value.is_empty() {
        return Err(format!("{at} is empty"));
    }
    if value.trim() != value {
        return Err(format!("{at} `{value}` begins or ends with white space"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{at} holds a control character"));
    }
    if value.chars().count() > NAME_LIMIT {
        return Err(format!("{at} is longer than {NAME_LIMIT} characters"));
    }
    Ok(())
}

/// Words shown to the person, which only have to be there.
fn text(value: &str, at: &str) -> std::result::Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{at} is empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asks(value: serde_json::Value) -> std::result::Result<Asks, String> {
        Asks::parse(&value.to_string())
    }

    fn one() -> serde_json::Value {
        serde_json::json!({
            "asks": [{
                "id": "approved:기준 3",
                "label": "기준 3",
                "version": "a1b2c3d4e5f6",
                "answers": [
                    {"name": "그대로 동의한다", "note": "none"},
                    {"name": "고칠 곳이 있다", "note": "required"}
                ]
            }]
        })
    }

    #[test]
    fn a_minimal_file_takes_its_defaults() {
        let read = asks(one()).unwrap();
        assert_eq!(read.locale, Locale::En);
        assert!(read.sources.is_empty() && read.together.is_empty());
        assert_eq!(read.asks[0].id, "approved:기준 3");
    }

    #[test]
    fn a_key_the_schema_does_not_name_is_refused() {
        let mut file = one();
        file["asks"][0]["hint"] = "x".into();
        assert!(asks(file).unwrap_err().contains("hint"));
        let mut file = one();
        file["title"] = "x".into();
        assert!(asks(file).unwrap_err().contains("title"));
    }

    #[test]
    fn a_name_that_reads_the_same_and_compares_different_is_refused() {
        for (field, value, said) in [
            ("id", "", "is empty"),
            ("id", " d-1", "white space"),
            ("version", "abc\n", "white space"),
            ("id", "d\u{7}1", "control character"),
        ] {
            let mut file = one();
            file["asks"][0][field] = value.into();
            let error = asks(file).unwrap_err();
            assert!(error.contains(said), "{field} {value:?}: {error}");
        }
        let mut file = one();
        file["asks"][0]["id"] = "x".repeat(NAME_LIMIT + 1).into();
        assert!(asks(file).unwrap_err().contains("longer than"));
        let mut file = one();
        file["asks"][0]["id"] = "가".repeat(NAME_LIMIT).into();
        assert!(asks(file).is_ok(), "the limit counts characters, not bytes");
    }

    #[test]
    fn an_ask_offers_at_least_two_answers_each_once() {
        let mut file = one();
        file["asks"][0]["answers"] = serde_json::json!([{"name": "a", "note": "none"}]);
        assert!(asks(file).unwrap_err().contains("fewer than two"));
        let mut file = one();
        file["asks"][0]["answers"][1]["name"] = "그대로 동의한다".into();
        assert!(asks(file).unwrap_err().contains("twice"));
        let mut file = one();
        file["asks"][0]["answers"][1]["note"] = "maybe".into();
        assert!(asks(file).is_err());
    }

    #[test]
    fn ids_are_unique_and_a_file_asks_something() {
        let mut file = one();
        let again = file["asks"][0].clone();
        file["asks"].as_array_mut().unwrap().push(again);
        assert!(asks(file).unwrap_err().contains("asked twice"));
        assert!(
            asks(serde_json::json!({"asks": []}))
                .unwrap_err()
                .contains("nothing to ask")
        );
    }

    #[test]
    fn a_set_asked_together_names_two_asked_ids_none_twice() {
        let mut file = one();
        let mut second = file["asks"][0].clone();
        second["id"] = "approved:기준 4".into();
        file["asks"].as_array_mut().unwrap().push(second);
        let both = serde_json::json!(["approved:기준 3", "approved:기준 4"]);

        file["together"] = serde_json::json!([{"label": "기준", "ids": both}]);
        assert!(asks(file.clone()).is_ok());

        file["together"] = serde_json::json!([{"label": "기준", "ids": ["approved:기준 3"]}]);
        assert!(asks(file.clone()).unwrap_err().contains("fewer than two"));

        file["together"] =
            serde_json::json!([{"label": "기준", "ids": ["approved:기준 3", "nope"]}]);
        assert!(asks(file.clone()).unwrap_err().contains("not asked"));

        file["together"] =
            serde_json::json!([{"label": "a", "ids": both}, {"label": "b", "ids": both}]);
        assert!(asks(file).unwrap_err().contains("named twice"));
    }

    #[test]
    fn serde_spells_each_vocabulary_as_its_wire_string() {
        for rule in NoteRule::ALL {
            assert_eq!(serde_json::to_value(rule).unwrap(), rule.as_str());
        }
        for locale in Locale::ALL {
            assert_eq!(serde_json::to_value(locale).unwrap(), locale.as_str());
        }
    }
}
