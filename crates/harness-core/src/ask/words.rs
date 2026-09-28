//! What harnex itself says on a page and to it, in each locale.
//!
//! One catalog serves both sides: the transport renders the refusals it sends,
//! and the page script receives the catalog and renders the rest. A `{name}`
//! is a placeholder, filled in one pass so a filled value is never read as
//! another placeholder. The words use letters, digits and ASCII punctuation
//! only, because a page that subsets its font to its own text has no glyph for
//! a symbol it never printed.

use serde::Serialize;

use super::asks::Locale;

/// Every sentence harnex says around a decision page.
#[derive(Debug, Serialize)]
pub struct Words {
    /// The page sent something that is not an answer set.
    pub unreadable: &'static str,
    pub empty: &'static str,
    pub not_asked: &'static str,
    /// `{label}`
    pub not_offered: &'static str,
    /// `{label}` `{answer}`
    pub note_required: &'static str,
    /// `{label}` `{answer}`
    pub note_forbidden: &'static str,
    /// `{label}` `{of}` `{answered}`
    pub together_partial: &'static str,
    pub too_large: &'static str,
    pub stale: &'static str,
    pub send: &'static str,
    /// `{answered}` `{asked}`
    pub progress: &'static str,
    /// `{time}`
    pub until: &'static str,
    pub sent: &'static str,
    pub unreachable: &'static str,
    /// `{label}`
    pub page_missing: &'static str,
    /// `{label}`
    pub page_answers_differ: &'static str,
    /// `{label}` `{answer}`
    pub page_note_missing: &'static str,
    /// `{label}`
    pub page_version_differs: &'static str,
    /// `{id}`
    pub page_unasked: &'static str,
    pub page_no_send: &'static str,
}

const EN: Words = Words {
    unreadable: "The answers sent could not be read.",
    empty: "No answer was chosen.",
    not_asked: "An answer was sent for something this page does not ask.",
    not_offered: "{label}: that is not one of the answers offered.",
    note_required: "{label}: '{answer}' needs a note.",
    note_forbidden: "{label}: '{answer}' takes no note.",
    together_partial: "{label}: these are answered together, and {answered} of {of} were answered.",
    too_large: "The answers sent are too large.",
    stale: "The source documents changed after this page opened, so these answers were not taken. The session has been told.",
    send: "Send",
    progress: "Answered {answered} / {asked}",
    until: "Taking answers until {time}",
    sent: "Sent. The session has the answers, and this window can be closed.",
    unreachable: "The session could not be reached. The command waiting for answers may have ended.",
    page_missing: "{label}: this page has no place for it.",
    page_answers_differ: "{label}: the answers on this page differ from the ones asked.",
    page_note_missing: "{label}: '{answer}' has no field for its note.",
    page_version_differs: "{label}: this page shows a different version from the one asked.",
    page_unasked: "{id}: this page has a place for something not asked.",
    page_no_send: "This page has no place for the send button.",
};

const KO: Words = Words {
    unreadable: "보낸 답을 읽지 못했다.",
    empty: "고른 답이 없다.",
    not_asked: "이 페이지가 묻지 않은 것에 답했다.",
    not_offered: "{label}: 고를 수 있는 답이 아니다.",
    note_required: "{label}: '{answer}'에는 적을 것이 있다.",
    note_forbidden: "{label}: '{answer}'에는 적을 것이 없다.",
    together_partial: "{label}: 함께 답한다. {of}개 가운데 {answered}개만 답했다.",
    too_large: "보낸 답이 너무 크다.",
    stale: "이 페이지를 연 뒤 원천 문서가 바뀌어서 이 답은 받지 않았다. 세션에 그렇게 알렸다.",
    send: "보내기",
    progress: "답한 것 {answered} / {asked}",
    until: "{time}까지 답을 받는다",
    sent: "보냈다. 세션이 답을 받았다. 이 창은 닫아도 된다.",
    unreachable: "세션에 닿지 않았다. 답을 기다리던 명령이 끝났을 수 있다.",
    page_missing: "{label}: 묻는 자리가 페이지에 없다.",
    page_answers_differ: "{label}: 페이지의 답이 묻는 답과 다르다.",
    page_note_missing: "{label}: '{answer}'에 적을 칸이 없다.",
    page_version_differs: "{label}: 페이지가 보인 판과 묻는 판이 다르다.",
    page_unasked: "{id}: 묻지 않는 자리가 페이지에 있다.",
    page_no_send: "보내기 자리가 페이지에 없다.",
};

impl Locale {
    pub fn words(self) -> &'static Words {
        match self {
            Self::En => &EN,
            Self::Ko => &KO,
        }
    }
}

/// `template` with each `{name}` in `values` replaced, in one pass.
pub fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| Some((close, values.iter().find(|(k, _)| *k == &after[..close])?)))
        {
            Some((close, (_, value))) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn placeholders(template: &str) -> BTreeSet<&str> {
        template
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect()
    }

    fn fields(locale: Locale) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(locale.words()).unwrap() {
            serde_json::Value::Object(map) => map,
            other => panic!("words serialise as an object, not {other}"),
        }
    }

    #[test]
    fn every_locale_fills_each_sentence_with_the_same_placeholders() {
        let reference = fields(Locale::En);
        for locale in Locale::ALL {
            let fields = fields(*locale);
            for (key, template) in &reference {
                let theirs = fields[key].as_str().unwrap();
                assert_eq!(
                    placeholders(template.as_str().unwrap()),
                    placeholders(theirs),
                    "{} `{key}`",
                    locale.as_str()
                );
            }
        }
    }

    #[test]
    fn the_words_carry_no_symbol_a_subset_font_would_lack() {
        for locale in Locale::ALL {
            for (key, template) in fields(*locale) {
                let template = template.as_str().unwrap();
                assert!(
                    template
                        .chars()
                        .all(|c| c.is_alphanumeric() || c.is_ascii()),
                    "{} `{key}`: {template}",
                    locale.as_str()
                );
            }
        }
    }

    #[test]
    fn render_fills_once_and_leaves_what_it_was_not_given() {
        assert_eq!(
            render(
                "{label}: '{answer}'",
                &[("label", "{answer}"), ("answer", "a")]
            ),
            "{answer}: 'a'"
        );
        assert_eq!(render("{x} {", &[]), "{x} {");
        assert_eq!(render("n = {n}.", &[("n", "3")]), "n = 3.");
    }
}
