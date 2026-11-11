use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use rhesis_protocol::recommendation::Recommendation;

use crate::{
    client::map_dto, color_rules::ColorRules, constants::FIXTURE_DIR, dto::LanguageToolDto,
};

pub(crate) struct FakeProvider {
    /// input text → canned answer; an `Err` entry reports an outage.
    entries: HashMap<String, Result<Vec<Recommendation>, String>>,
    /// User color rules, applied to answers created after receiving them.
    rules: ColorRules,
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            rules: ColorRules::default(),
            address: String::new(),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl FakeProvider {
    pub(crate) fn from_fixture(id: &str) -> Result<FakeProvider, String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE_DIR)
            .join(format!("{id}.json"));
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read fixture {}: {e}", path.display()))?;
        let entries: Vec<FixtureEntry> = serde_json::from_str(&raw)
            .map_err(|e| format!("fixture {} is not valid: {e}", path.display()))?;

        let mut fake = FakeProvider::default();
        for entry in entries {
            match entry.response {
                Some(dto) => {
                    let recommendations = map_dto(dto, &fake.rules);
                    fake.entries.insert(entry.text, Ok(recommendations));
                }
                None => {
                    fake.entries.insert(
                        entry.text,
                        Err(entry.error.unwrap_or_else(|| {
                            format!("fixture {id}: entry has no 'response' and no 'error'")
                        })),
                    );
                }
            };
        }
        Ok(fake)
    }

    pub(crate) fn check(&self, text: &str) -> Result<Vec<Recommendation>, String> {
        if let Ok(mut log) = self.requests.lock() {
            log.push(text.to_string());
        }
        match self.entries.get(text) {
            Some(Ok(recommendations)) => Ok(recommendations.clone()),
            Some(Err(message)) => Err(message.clone()),
            None => Ok(Vec::new()),
        }
    }

    pub(crate) fn update_address(&mut self, base_address: &str) {
        self.address = base_address.to_string();
    }

    pub(crate) fn set_colors(&mut self, rules: Vec<(String, String)>) {
        self.rules = ColorRules::new(rules);
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }
}

#[derive(serde::Deserialize)]
struct FixtureEntry {
    text: String,
    #[serde(default)]
    response: Option<LanguageToolDto>,
    #[serde(default)]
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion_provider::CompletionProvider;

    /// The test-side construction surface of the fake provider — kept out
    /// of the production impl block above; a child-module impl can reach
    /// the private fields directly.
    impl FakeProvider {
        /// Builder: record a canned answer for `text`.
        fn respond(mut self, text: &str, recommendations: Vec<Recommendation>) -> Self {
            self.entries.insert(text.to_string(), Ok(recommendations));
            self
        }

        /// Builder: map a real-shaped response through the live client's
        /// conversion (exercising the DTO mapping).
        fn respond_dto(mut self, text: &str, dto: LanguageToolDto) -> Self {
            let recommendations = map_dto(dto, &self.rules);
            self.entries.insert(text.to_string(), Ok(recommendations));
            self
        }

        /// Builder: make `text` fail like an unresponsive provider.
        fn fail(mut self, text: &str, message: impl Into<String>) -> Self {
            self.entries.insert(text.to_string(), Err(message.into()));
            self
        }

        /// Shared handle to the request log; every `check` call appends the text.
        fn request_log(&self) -> Arc<Mutex<Vec<String>>> {
            self.requests.clone()
        }
    }

    /// The provider API is async (the live arm is reqwest); tests drive it
    /// through a throwaway runtime — same shape as the worker's block_on.
    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// Range stays default (empty): the fake fixtures only exercise
    /// delivery, not the painted geometry.
    fn rec(value: &str) -> Recommendation {
        Recommendation {
            value: value.to_string(),
            ..Default::default()
        }
    }

    /// A small LanguageTool-shaped response equivalent to a real server
    /// answer for "I has a mistake" (recorded live variants live in
    /// `tests/fixtures/lt/`). The trimmed DTO models only the consumed
    /// sections, so the builder's shape is the parsed shape.
    fn sample_dto() -> LanguageToolDto {
        use crate::dto::*;

        LanguageToolDto {
            language: Language {
                code: "en-US".into(),
            },
            matches: vec![Match {
                message: "Did you mean \"have\"".into(),
                replacements: vec![Replacement {
                    value: "have".into(),
                }],
                offset: 2,
                length: 3,
                rule: Rule {
                    id: "IT_IS_I_HAS".into(),
                    category: Category {
                        id: "GRAMMAR".into(),
                    },
                },
            }],
        }
    }

    #[test]
    fn fake_answers_recorded_text_only() {
        let fake = FakeProvider::default().respond("hallo", vec![rec("hello")]);
        let provider = CompletionProvider::Fake(fake);

        let answered = block_on(provider.check("hallo"));
        assert!(matches!(answered, Ok(r) if r.len() == 1));

        // Unknown text means "no findings", not an error (the real server
        // answers with zero matches too).
        let unmatched = block_on(provider.check("nothing to see"));
        assert!(matches!(unmatched, Ok(r) if r.is_empty()));
    }

    #[test]
    fn fake_request_log_records_every_check() {
        let fake = FakeProvider::default().respond("a", vec![rec("b")]);
        let log = fake.request_log();
        let provider = CompletionProvider::Fake(fake);

        let _ = block_on(provider.check("a"));
        let _ = block_on(provider.check("c"));

        assert_eq!(*log.lock().unwrap(), ["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn fake_failure_entry_reports_outage() {
        let fake = FakeProvider::default().fail("boom", "connection lost");
        let provider = CompletionProvider::Fake(fake);
        assert_eq!(
            block_on(provider.check("boom")),
            Err("connection lost".to_string())
        );
    }

    #[test]
    fn fake_maps_fixture_dto_through_live_conversion() {
        let fake = FakeProvider::default().respond_dto("I has a mistake", sample_dto());
        let provider = CompletionProvider::Fake(fake);
        let suggestions = block_on(provider.check("I has a mistake")).unwrap();
        assert_eq!(suggestions[0].value, "have");
        assert_eq!(suggestions[0].range.start, 2);
        assert_eq!(suggestions[0].range.length, 3);
        assert_eq!(suggestions[0].language, "EN-US");
        assert_eq!(suggestions[0].color, "#FF0000");
    }

    #[test]
    fn fixture_checker_loads_and_answers() {
        let fake = FakeProvider::from_fixture("checker").expect("fixture loads");
        let log = fake.request_log();
        let provider = CompletionProvider::Fake(fake);

        let recs =
            block_on(provider.check("She like football and play the guitar quicly.")).unwrap();
        // HE_LIKE has two replacements ("is like", "likes"), so one of the
        // two matches becomes two recommendations.
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].rule_id, "HE_LIKE");
        assert_eq!(recs[0].value, "is like");
        assert_eq!(recs[0].range.start, 4);
        assert_eq!(recs[0].range.length, 4);
        assert_eq!(recs[0].language, "EN-US");
        assert_eq!(recs[1].rule_id, "HE_LIKE");
        assert_eq!(recs[1].value, "likes");
        assert_eq!(recs[2].rule_id, "MORFOLOGIK_RULE_EN_US");
        assert_eq!(recs[2].value, "quickly");
        assert_eq!(recs[2].range.start, 38);
        assert_eq!(recs[2].range.length, 6);

        let clean = block_on(provider.check("This sentence has no problems.")).unwrap();
        assert!(clean.is_empty());
        assert_eq!(
            *log.lock().unwrap(),
            [
                "She like football and play the guitar quicly.".to_string(),
                "This sentence has no problems.".to_string()
            ]
        );
    }
}
