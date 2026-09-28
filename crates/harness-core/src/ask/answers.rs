//! What a page sends back, and the record an answered ask becomes.

use std::collections::BTreeSet;
use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::asks::{Asks, NoteRule, Offered};
use super::words::{Words, render};
use crate::error::{Error, Result};

/// One answer, with what was shown when it was chosen: the ask's label and
/// every answer offered beside it, so whether it still holds can be told from
/// the record alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub id: String,
    /// The ask's version as the caller gave it.
    pub version: String,
    pub label: String,
    pub offered: Vec<Offered>,
    pub answer: String,
    /// The person's own words, where the answer takes them.
    pub note: Option<String>,
    /// When the answer was chosen.
    pub at: Timestamp,
}

/// How an ask ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "outcome", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Outcome {
    Answered {
        url: String,
        answers: Vec<Answer>,
    },
    /// A source changed after the page was served, so nothing was taken.
    Stale {
        url: String,
    },
    /// No answer set arrived in time.
    Unanswered {
        url: String,
    },
}

/// Why an answer set is refused. Each is the person's to correct, so each is
/// said in the page's locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Unreadable,
    Empty,
    NotAsked,
    NotOffered {
        label: String,
    },
    NoteRequired {
        label: String,
        answer: String,
    },
    NoteForbidden {
        label: String,
        answer: String,
    },
    TogetherPartial {
        label: String,
        of: usize,
        answered: usize,
    },
}

impl Refusal {
    pub fn said(&self, words: &Words) -> String {
        match self {
            Self::Unreadable => words.unreadable.to_string(),
            Self::Empty => words.empty.to_string(),
            Self::NotAsked => words.not_asked.to_string(),
            Self::NotOffered { label } => render(words.not_offered, &[("label", label)]),
            Self::NoteRequired { label, answer } => {
                render(words.note_required, &[("label", label), ("answer", answer)])
            }
            Self::NoteForbidden { label, answer } => render(
                words.note_forbidden,
                &[("label", label), ("answer", answer)],
            ),
            Self::TogetherPartial {
                label,
                of,
                answered,
            } => render(
                words.together_partial,
                &[
                    ("label", label),
                    ("of", &of.to_string()),
                    ("answered", &answered.to_string()),
                ],
            ),
        }
    }
}

/// An answer set as the page sends it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sent {
    answers: Vec<Chosen>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chosen {
    id: String,
    answer: String,
    #[serde(default)]
    note: Option<String>,
}

/// The answers in `body`, checked against `asks`: each names something asked,
/// once, with an answer offered for it and a note exactly where that answer
/// takes one, and every set asked together is answered whole or not at all.
/// Anything on this machine can post to the page's address, so the body is
/// read against the asks and never against the page.
pub fn read(body: &[u8], asks: &Asks, at: Timestamp) -> std::result::Result<Vec<Answer>, Refusal> {
    let sent: Sent = serde_json::from_slice(body).map_err(|_| Refusal::Unreadable)?;
    if sent.answers.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut answers: Vec<Answer> = Vec::with_capacity(sent.answers.len());
    for chosen in sent.answers {
        let ask = asks.ask(&chosen.id).ok_or(Refusal::NotAsked)?;
        if answers.iter().any(|a| a.id == ask.id) {
            return Err(Refusal::NotAsked);
        }
        let offered = ask
            .answers
            .iter()
            .find(|o| o.name == chosen.answer)
            .ok_or_else(|| Refusal::NotOffered {
                label: ask.label.clone(),
            })?;
        let note = chosen
            .note
            .map(|note| note.trim().to_string())
            .filter(|note| !note.is_empty());
        match (offered.note, &note) {
            (NoteRule::Required, None) => {
                return Err(Refusal::NoteRequired {
                    label: ask.label.clone(),
                    answer: offered.name.clone(),
                });
            }
            (NoteRule::None, Some(_)) => {
                return Err(Refusal::NoteForbidden {
                    label: ask.label.clone(),
                    answer: offered.name.clone(),
                });
            }
            _ => {}
        }
        answers.push(Answer {
            id: ask.id.clone(),
            version: ask.version.clone(),
            label: ask.label.clone(),
            offered: ask.answers.clone(),
            answer: offered.name.clone(),
            note,
            at,
        });
    }
    for set in &asks.together {
        let answered = set
            .ids
            .iter()
            .filter(|id| answers.iter().any(|a| &a.id == *id))
            .count();
        if answered != 0 && answered != set.ids.len() {
            return Err(Refusal::TogetherPartial {
                label: set.label.clone(),
                of: set.ids.len(),
                answered,
            });
        }
    }
    Ok(answers)
}

/// The answers an answered outcome recorded, read from `path` — what an
/// earlier `serve` printed as its data, or the same record another transport
/// wrote.
pub fn read_answered(path: &Path) -> Result<Vec<Answer>> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::IoFailure {
        path: path.to_path_buf(),
        source,
    })?;
    answered(&text).map_err(|message| Error::AskInputInvalid {
        path: path.to_path_buf(),
        message,
    })
}

fn answered(text: &str) -> std::result::Result<Vec<Answer>, String> {
    let answers = match serde_json::from_str(text).map_err(|e| e.to_string())? {
        Outcome::Answered { answers, .. } => answers,
        Outcome::Stale { .. } => return Err("the outcome is `stale`, which took no answers".into()),
        Outcome::Unanswered { .. } => {
            return Err("the outcome is `unanswered`, which took no answers".into());
        }
    };
    if answers.is_empty() {
        return Err("the outcome is `answered` but records no answer".into());
    }
    let mut ids = BTreeSet::new();
    for answer in &answers {
        if !ids.insert(answer.id.as_str()) {
            return Err(format!("`{}` is answered twice", answer.id));
        }
        if answer
            .note
            .as_deref()
            .is_some_and(|note| note.trim().is_empty())
        {
            return Err(format!("`{}` records a blank note", answer.id));
        }
        let offered = answer
            .offered
            .iter()
            .find(|o| o.name == answer.answer)
            .ok_or_else(|| {
                format!(
                    "`{}` records `{}`, which it did not offer",
                    answer.id, answer.answer
                )
            })?;
        let broken = match (offered.note, &answer.note) {
            (NoteRule::Required, None) => Some("without the note it requires"),
            (NoteRule::None, Some(_)) => Some("with a note it does not take"),
            _ => None,
        };
        if let Some(broken) = broken {
            return Err(format!(
                "`{}` records `{}` {broken}",
                answer.id, answer.answer
            ));
        }
    }
    Ok(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::asks::Locale;

    fn asks() -> Asks {
        Asks::parse(
            &serde_json::json!({
                "asks": [
                    {"id": "d-1", "label": "결정 1", "version": "v1", "answers": [
                        {"name": "지금", "note": "none"},
                        {"name": "나중", "note": "optional"}]},
                    {"id": "d-2", "label": "설계", "version": "v2", "answers": [
                        {"name": "동의", "note": "none"},
                        {"name": "고칠 곳", "note": "required"}]},
                    {"id": "c-1", "label": "기준 1", "version": "v3", "answers": [
                        {"name": "통과", "note": "none"}, {"name": "실패", "note": "none"}]},
                    {"id": "c-2", "label": "기준 2", "version": "v4", "answers": [
                        {"name": "통과", "note": "none"}, {"name": "실패", "note": "none"}]}
                ],
                "together": [{"label": "기준", "ids": ["c-1", "c-2"]}]
            })
            .to_string(),
        )
        .unwrap()
    }

    fn sent(answers: serde_json::Value) -> std::result::Result<Vec<Answer>, Refusal> {
        read(
            serde_json::json!({ "answers": answers })
                .to_string()
                .as_bytes(),
            &asks(),
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn an_answer_carries_what_was_shown_beside_it() {
        let answers = sent(serde_json::json!([
            {"id": "d-2", "answer": "고칠 곳", "note": "  3번 칸  "}
        ]))
        .unwrap();
        assert_eq!(answers.len(), 1);
        let answer = &answers[0];
        assert_eq!(
            (answer.version.as_str(), answer.label.as_str()),
            ("v2", "설계")
        );
        assert_eq!(answer.offered, asks().asks[1].answers);
        assert_eq!(answer.note.as_deref(), Some("3번 칸"));
    }

    #[test]
    fn an_answer_set_that_breaks_the_asks_is_refused_with_its_reason() {
        let label = |s: &str| s.to_string();
        for (answers, refusal) in [
            (serde_json::json!([]), Refusal::Empty),
            (
                serde_json::json!([{"id": "d-9", "answer": "지금"}]),
                Refusal::NotAsked,
            ),
            (
                serde_json::json!([{"id": "d-1", "answer": "지금"}, {"id": "d-1", "answer": "나중"}]),
                Refusal::NotAsked,
            ),
            (
                serde_json::json!([{"id": "d-1", "answer": "모름"}]),
                Refusal::NotOffered {
                    label: label("결정 1"),
                },
            ),
            (
                serde_json::json!([{"id": "d-2", "answer": "고칠 곳", "note": "   "}]),
                Refusal::NoteRequired {
                    label: label("설계"),
                    answer: label("고칠 곳"),
                },
            ),
            (
                serde_json::json!([{"id": "d-1", "answer": "지금", "note": "x"}]),
                Refusal::NoteForbidden {
                    label: label("결정 1"),
                    answer: label("지금"),
                },
            ),
            (
                serde_json::json!([{"id": "c-1", "answer": "통과"}]),
                Refusal::TogetherPartial {
                    label: label("기준"),
                    of: 2,
                    answered: 1,
                },
            ),
        ] {
            assert_eq!(sent(answers.clone()).unwrap_err(), refusal, "{answers}");
        }
    }

    #[test]
    fn a_body_outside_the_closed_shape_is_unreadable() {
        for body in [
            "not json",
            "[]",
            r#"{"answers": [{"id": "d-1", "answer": "지금", "extra": 1}]}"#,
            r#"{"answers": [{"id": "d-1", "answer": "나중", "note": 3}]}"#,
            r#"{"version": "x", "answers": []}"#,
        ] {
            assert_eq!(
                read(body.as_bytes(), &asks(), Timestamp::UNIX_EPOCH).unwrap_err(),
                Refusal::Unreadable,
                "{body}"
            );
        }
    }

    #[test]
    fn a_partial_answer_set_outside_any_set_asked_together_is_taken() {
        let answers = sent(serde_json::json!([
            {"id": "c-1", "answer": "통과"}, {"id": "c-2", "answer": "실패"},
            {"id": "d-1", "answer": "나중", "note": null}
        ]))
        .unwrap();
        assert_eq!(answers.len(), 3);
    }

    #[test]
    fn each_refusal_is_said_in_the_page_locale() {
        let refusal = Refusal::TogetherPartial {
            label: "기준".into(),
            of: 3,
            answered: 1,
        };
        assert_eq!(
            refusal.said(Locale::Ko.words()),
            "기준: 함께 답한다. 3개 가운데 1개만 답했다."
        );
        assert!(refusal.said(Locale::En.words()).contains("1 of 3"));
    }

    #[test]
    fn an_answered_record_is_read_back_and_checked() {
        let answers = sent(serde_json::json!([{"id": "d-2", "answer": "동의"}])).unwrap();
        let record = Outcome::Answered {
            url: "http://127.0.0.1:1/t/page/p.html".into(),
            answers,
        };
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(answered(&text).unwrap().len(), 1);

        let stale = serde_json::to_string(&Outcome::Stale { url: "u".into() }).unwrap();
        assert!(answered(&stale).unwrap_err().contains("stale"));

        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["answers"][0]["answer"] = "고칠 곳".into();
        assert!(
            answered(&value.to_string())
                .unwrap_err()
                .contains("without the note")
        );
        value["answers"][0]["note"] = " ".into();
        assert!(
            answered(&value.to_string())
                .unwrap_err()
                .contains("blank note")
        );
        value["answers"][0]["note"] = serde_json::Value::Null;
        value["answers"][0]["answer"] = "모름".into();
        assert!(
            answered(&value.to_string())
                .unwrap_err()
                .contains("did not offer")
        );
        value["answers"] = serde_json::json!([]);
        assert!(
            answered(&value.to_string())
                .unwrap_err()
                .contains("no answer")
        );
    }
}
