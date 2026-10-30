//! Completion provider seam: where suggestions come from.
//!
//! The production provider ([`CompletionProvider::Live`]) talks to a
//! LanguageTool server over HTTP through [`LanguageToolClient`].
//!
//! Selection is environmental and happens once per worker start: when
//! `RHESIS_FAKE_LT=<fixture id>` is set, [`CompletionProvider::chosen`]
//! loads the recorded fixture instead of the live client, and the worker
//! skips the whole server lifecycle (there is no server to spawn). The UI
//! harness and boot tiers seed the variable before the worker starts; in
//! production it is never set, so the live client is chosen and nothing
//! else changes.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    interop::recommendation::Recommendation,
    languagetool::{client::LanguageToolClient, models::LanguageToolDto},
};
use cxx_qt_lib::QString;

/// Where completion (check) requests are answered by the worker.
pub(crate) enum CompletionProvider {
    Live(LanguageToolClient),
    Fake(FakeProvider),
}

impl CompletionProvider {
    /// Get suggestions for `text`. An `Err` marks the provider unresponsive.
    pub(crate) async fn check(&self, text: &str) -> Result<Vec<Recommendation>, String> {
        match self {
            CompletionProvider::Live(client) => client.get_recommendation(text).await,
            CompletionProvider::Fake(fake) => fake.check(text),
        }
    }

    /// Lightweight readiness probe. The fake has no state to probe and
    /// lifecycle paths short-circuit before this, so this arm is vacuous.
    pub(crate) async fn health_check(&self) -> bool {
        match self {
            CompletionProvider::Live(client) => client.health_check().await,
            CompletionProvider::Fake(_) => true,
        }
    }

    /// The provider a fresh boot uses: when `RHESIS_FAKE_LT=<id>` is set, the
    /// recorded fixture answers every request — the UI harness and boot tiers
    /// seed the variable before the worker starts (see `src/ui_tests.rs`),
    /// so the same recorded fixtures run everywhere without a LanguageTool
    /// installation. A fixture that fails to load falls back to the live
    /// client with a warning, so a broken harness never breaks a real boot.
    pub(crate) fn chosen(base_address: &str) -> Self {
        match std::env::var_os("RHESIS_FAKE_LT") {
            Some(id) => match FakeProvider::from_fixture(&id.to_string_lossy()) {
                Ok(fake) => {
                    log::info!("using the recorded fake provider fixture {id:?}");
                    CompletionProvider::Fake(fake)
                }
                Err(e) => {
                    log::warn!(
                        "fake provider fixture {id:?} failed to load: {e}; \
                         falling back to the live client"
                    );
                    CompletionProvider::Live(LanguageToolClient::new(base_address))
                }
            },
            None => CompletionProvider::Live(LanguageToolClient::new(base_address)),
        }
    }

    /// Point the provider at a new server base URL.
    pub(crate) fn update_address(&mut self, base_address: &str) {
        match self {
            CompletionProvider::Live(client) => client.update_address(base_address),
            CompletionProvider::Fake(fake) => fake.update_address(base_address),
        }
    }

    /// Apply the user's color rules (`CATEGORY:x` / `RULE:y` keys).
    /// Note: fake entries are pre-mapped at construction, so late color
    /// changes do not re-color already recorded answers.
    pub(crate) fn set_colors(&mut self, rules: Vec<(QString, QString)>) {
        match self {
            CompletionProvider::Live(client) => client.set_colors(rules),
            CompletionProvider::Fake(fake) => fake.set_colors(rules),
        }
    }

    /// Base URL the provider currently points at.
    pub(crate) fn address(&self) -> &str {
        match self {
            CompletionProvider::Live(client) => client.address(),
            CompletionProvider::Fake(fake) => fake.address(),
        }
    }
}

/// One entry of a fixture file: the input text and either the recorded
/// LanguageTool response (same shape as a live `/v2/check` answer) or a
/// canned failure (`error`) to exercise unresponsive-provider paths.
#[derive(serde::Deserialize)]
struct FixtureEntry<'a> {
    text: String,
    #[serde(default)]
    response: Option<LanguageToolDto<'a>>,
    #[serde(default)]
    error: Option<String>,
}

/// Test/smoke provider: answers `check` requests from a recorded
/// text → answer table instead of HTTP. Every request is appended to a
/// shared log accessible via [`FakeProvider::request_log`] (clone-friendly
/// probe for tests: e.g. asserting the debounce collapsed N keystrokes
/// into one request).
///
/// Built either programmatically ([`FakeProvider::new`] plus the
/// `record_*`/`fail` builders, unit tests) or from a fixture file
/// (UI harness via `RHESIS_FAKE_LT`).
pub(crate) struct FakeProvider {
    /// input text → canned answer; an `Err` entry reports an outage.
    entries: HashMap<String, Result<Vec<Recommendation>, String>>,
    /// User color rules, applied to answers created after receiving them.
    rules: HashMap<String, QString>,
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            rules: HashMap::new(),
            address: String::new(),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Load the fixture `<id>.json` from `tests/fixtures/lt/`: a JSON array of
    /// `{ "text": ..., "response": <LT answer> | "error": "..." }` objects.
    /// Response mapping goes through the same code path as the live client,
    /// so fixtures carry the real DTO → [`Recommendation`] conversion.
    fn from_fixture(id: &str) -> Result<FakeProvider, String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lt")
            .join(format!("{id}.json"));
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read fixture {}: {e}", path.display()))?;
        let entries: Vec<FixtureEntry> = serde_json::from_str(&raw)
            .map_err(|e| format!("fixture {} is not valid: {e}", path.display()))?;

        let mut fake = FakeProvider::new();
        for entry in entries {
            match entry.response {
                Some(dto) => {
                    let recommendations = super::client::map_dto(dto, &fake.rules);
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

    /// Builder: record a canned answer for `text` (unit tests).
    #[cfg(test)]
    fn respond(mut self, text: &str, recommendations: Vec<Recommendation>) -> Self {
        self.entries.insert(text.to_string(), Ok(recommendations));
        self
    }

    /// Builder: map a real-shaped response through the live client's
    /// conversion (unit tests exercising the DTO mapping).
    #[cfg(test)]
    fn respond_dto(mut self, text: &str, dto: LanguageToolDto<'static>) -> Self {
        let recommendations = super::client::map_dto(dto, &self.rules);
        self.entries.insert(text.to_string(), Ok(recommendations));
        self
    }

    /// Builder: make `text` fail like an unresponsive provider (unit tests).
    #[cfg(test)]
    fn fail(mut self, text: &str, message: impl Into<String>) -> Self {
        self.entries.insert(text.to_string(), Err(message.into()));
        self
    }

    /// Shared handle to the request log; every `check` call appends the text.
    #[cfg(test)]
    fn request_log(&self) -> Arc<Mutex<Vec<String>>> {
        self.requests.clone()
    }

    fn check(&self, text: &str) -> Result<Vec<Recommendation>, String> {
        if let Ok(mut log) = self.requests.lock() {
            log.push(text.to_string());
        }
        match self.entries.get(text) {
            Some(Ok(recommendations)) => Ok(recommendations.clone()),
            Some(Err(message)) => Err(message.clone()),
            None => Ok(Vec::new()),
        }
    }

    fn update_address(&mut self, base_address: &str) {
        self.address = base_address.to_string();
    }

    fn set_colors(&mut self, rules: Vec<(QString, QString)>) {
        self.rules.clear();
        for (key, value) in rules {
            self.rules.insert(key.to_string(), value);
        }
    }

    fn address(&self) -> &str {
        &self.address
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            value: QString::from(value),
            ..Default::default()
        }
    }

    /// A small LanguageTool-shaped response equivalent to a real server
    /// answer for "I has a mistake" (recorded live variants live in
    /// `tests/fixtures/lt/`). Fully owned so tests can hold and move it.
    fn sample_dto() -> LanguageToolDto<'static> {
        use crate::languagetool::models::*;

        LanguageToolDto {
            software: Software {
                name: "LanguageTool".into(),
                version: "6.8.0".into(),
                build_date: "2026-01-01".into(),
                api_version: 1,
                premium: false,
                premium_hint: "".into(),
                status: "".into(),
            },
            warnings: Warnings {
                incomplete_results: false,
            },
            language: Language {
                name: "English (US)".into(),
                code: "en-US".into(),
                detected_language: LanguageDetectedLanguage {
                    name: "English".into(),
                    code: "en-US".into(),
                    confidence: 1.0,
                    source: "text".into(),
                },
            },
            matches: vec![Match {
                message: "Did you mean \"have\"".into(),
                short_message: "Possible agreement error".into(),
                replacements: vec![Replacement {
                    value: "have".into(),
                }],
                offset: 2,
                length: 3,
                context: Context {
                    text: "I has a mistake".into(),
                    offset: 0,
                    length: 15,
                },
                sentence: "I has a mistake".into(),
                match_type: Type {
                    type_name: "UnknownText".into(),
                },
                rule: Rule {
                    id: "IT_IS_I_HAS".into(),
                    description: "It is / has mismatch".into(),
                    issue_type: "grammar".into(),
                    category: Category {
                        id: "GRAMMAR".into(),
                        name: "Grammar".into(),
                    },
                },
                ignore_for_incomplete_sentence: false,
                context_for_sure_match: -1,
            }],
            sentence_ranges: vec![vec![0, 15]],
            extended_sentence_ranges: vec![ExtendedSentenceRange {
                from: 0,
                to: 15,
                detected_languages: vec![DetectedLanguageElement {
                    language: "en".into(),
                    rate: 1.0,
                }],
            }],
        }
    }

    #[test]
    fn fake_answers_recorded_text_only() {
        let fake = FakeProvider::new().respond("hallo", vec![rec("hello")]);
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
        let fake = FakeProvider::new().respond("a", vec![rec("b")]);
        let log = fake.request_log();
        let provider = CompletionProvider::Fake(fake);

        let _ = block_on(provider.check("a"));
        let _ = block_on(provider.check("c"));

        assert_eq!(*log.lock().unwrap(), ["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn fake_failure_entry_reports_outage() {
        let fake = FakeProvider::new().fail("boom", "connection lost");
        let provider = CompletionProvider::Fake(fake);
        assert_eq!(
            block_on(provider.check("boom")),
            Err("connection lost".to_string())
        );
    }

    #[test]
    fn fake_maps_fixture_dto_through_live_conversion() {
        let fake = FakeProvider::new().respond_dto("I has a mistake", sample_dto());
        let provider = CompletionProvider::Fake(fake);
        let suggestions = block_on(provider.check("I has a mistake")).unwrap();
        assert_eq!(suggestions[0].value, QString::from("have"));
        assert_eq!(suggestions[0].range.start, 2);
        assert_eq!(suggestions[0].range.length, 3);
        assert_eq!(suggestions[0].language, QString::from("EN-US"));
        assert_eq!(suggestions[0].color, QString::from("#FF0000"));
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
        assert_eq!(recs[0].rule_id, QString::from("HE_LIKE"));
        assert_eq!(recs[0].value, QString::from("is like"));
        assert_eq!(recs[0].range.start, 4);
        assert_eq!(recs[0].range.length, 4);
        assert_eq!(recs[0].language, QString::from("EN-US"));
        assert_eq!(recs[1].rule_id, QString::from("HE_LIKE"));
        assert_eq!(recs[1].value, QString::from("likes"));
        assert_eq!(recs[2].rule_id, QString::from("MORFOLOGIK_RULE_EN_US"));
        assert_eq!(recs[2].value, QString::from("quickly"));
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
