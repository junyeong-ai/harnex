//! Whether answers taken earlier still hold against the asks as they read now.

use serde::Serialize;

use super::answers::Answer;
use super::asks::Asks;
use crate::wire_enum::wire_enum;

wire_enum! {
    /// Where an earlier answer stands against the asks as they read now.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
    #[serde(rename_all = "kebab-case")]
    pub enum AnswerState {
        /// Asked now exactly as it was answered.
        Current => "current",
        /// Still asked, but not as it was answered: its version, its label or
        /// what it offered moved, or a set it is answered with did.
        Changed => "changed",
        /// No longer asked.
        Gone => "gone",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Standing {
    pub id: String,
    pub state: AnswerState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Current {
    pub items: Vec<Standing>,
}

impl Current {
    /// Whether every answer still holds.
    pub fn holds(&self) -> bool {
        self.items.iter().all(|s| s.state == AnswerState::Current)
    }
}

/// Each answer's standing against `now`, in the order they were answered. An
/// answer holds only while its ask is asked under the same id, version and
/// label with the same answers offered, since the caller's version may not
/// cover what the page offered. A set asked together holds whole or not at
/// all, under the sets `now` declares.
pub fn current(answers: &[Answer], now: &Asks) -> Current {
    let mut items: Vec<Standing> = answers
        .iter()
        .map(|answer| Standing {
            id: answer.id.clone(),
            state: match now.ask(&answer.id) {
                None => AnswerState::Gone,
                Some(ask)
                    if ask.version == answer.version
                        && ask.label == answer.label
                        && ask.answers == answer.offered =>
                {
                    AnswerState::Current
                }
                Some(_) => AnswerState::Changed,
            },
        })
        .collect();
    for set in &now.together {
        let members: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, s)| set.ids.contains(&s.id))
            .map(|(i, _)| i)
            .collect();
        let whole = members.len() == set.ids.len()
            && members
                .iter()
                .all(|&i| items[i].state == AnswerState::Current);
        if !members.is_empty() && !whole {
            for i in members {
                if items[i].state == AnswerState::Current {
                    items[i].state = AnswerState::Changed;
                }
            }
        }
    }
    Current { items }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::ask::answers::read;

    fn asks(value: serde_json::Value) -> Asks {
        Asks::parse(&value.to_string()).unwrap()
    }

    fn file() -> serde_json::Value {
        serde_json::json!({
            "asks": [
                {"id": "q:a", "label": "질문 A", "version": "va", "answers": [
                    {"name": "예", "note": "none"}, {"name": "아니오", "note": "none"}]},
                {"id": "v:1", "label": "기준 1", "version": "v1", "answers": [
                    {"name": "통과", "note": "none"}, {"name": "실패", "note": "required"}]},
                {"id": "v:2", "label": "기준 2", "version": "v2", "answers": [
                    {"name": "통과", "note": "none"}, {"name": "실패", "note": "required"}]}
            ],
            "together": [{"label": "기준", "ids": ["v:1", "v:2"]}]
        })
    }

    fn answered() -> Vec<Answer> {
        read(
            serde_json::json!({"answers": [
                {"id": "q:a", "answer": "예"},
                {"id": "v:1", "answer": "통과"},
                {"id": "v:2", "answer": "통과"}
            ]})
            .to_string()
            .as_bytes(),
            &asks(file()),
            Timestamp::UNIX_EPOCH,
        )
        .unwrap()
    }

    fn states(now: serde_json::Value) -> Vec<(String, AnswerState)> {
        current(&answered(), &asks(now))
            .items
            .into_iter()
            .map(|s| (s.id, s.state))
            .collect()
    }

    fn state_of(now: serde_json::Value, id: &str) -> AnswerState {
        states(now).into_iter().find(|(i, _)| i == id).unwrap().1
    }

    #[test]
    fn answers_asked_again_unchanged_all_hold() {
        let now = current(&answered(), &asks(file()));
        assert!(now.holds());
        assert_eq!(now.items.len(), 3);
    }

    #[test]
    fn a_moved_version_label_or_offer_changes_only_that_answer() {
        for (field, value) in [
            ("version", serde_json::json!("va2")),
            ("label", serde_json::json!("질문 A (고침)")),
            (
                "answers",
                serde_json::json!([
                    {"name": "예", "note": "none"}, {"name": "아니오", "note": "optional"}]),
            ),
        ] {
            let mut now = file();
            now["asks"][0][field] = value;
            assert_eq!(
                state_of(now.clone(), "q:a"),
                AnswerState::Changed,
                "{field}"
            );
            assert_eq!(state_of(now, "v:1"), AnswerState::Current, "{field}");
        }
    }

    #[test]
    fn an_ask_no_longer_asked_is_gone() {
        let mut now = file();
        now["asks"].as_array_mut().unwrap().remove(0);
        assert_eq!(state_of(now, "q:a"), AnswerState::Gone);
    }

    #[test]
    fn a_set_asked_together_holds_whole_or_not_at_all() {
        let mut now = file();
        now["asks"][2]["version"] = "v2b".into();
        assert_eq!(state_of(now.clone(), "v:1"), AnswerState::Changed);
        assert_eq!(state_of(now, "q:a"), AnswerState::Current);

        let mut now = file();
        now["asks"].as_array_mut().unwrap().push(serde_json::json!(
            {"id": "v:3", "label": "기준 3", "version": "v3", "answers": [
                {"name": "통과", "note": "none"}, {"name": "실패", "note": "required"}]}));
        now["together"][0]["ids"] = serde_json::json!(["v:1", "v:2", "v:3"]);
        assert_eq!(
            state_of(now, "v:1"),
            AnswerState::Changed,
            "a member added since leaves the set answered in part"
        );
    }

    #[test]
    fn serde_spells_each_state_as_its_wire_string() {
        for state in AnswerState::ALL {
            assert_eq!(serde_json::to_value(state).unwrap(), state.as_str());
        }
    }
}
