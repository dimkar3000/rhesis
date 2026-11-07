use rhesis_protocol::Recommendation;

use crate::{client::LanguageToolClient, fake_provider::FakeProvider};

pub(crate) enum CompletionProvider {
    Live(LanguageToolClient),
    Fake(FakeProvider),
}

impl CompletionProvider {
    pub(crate) async fn check(&self, text: &str) -> Result<Vec<Recommendation>, String> {
        match self {
            CompletionProvider::Live(client) => client.get_recommendation(text).await,
            CompletionProvider::Fake(fake) => fake.check(text),
        }
    }

    pub(crate) async fn health_check(&self) -> bool {
        match self {
            CompletionProvider::Live(client) => client.health_check().await,
            CompletionProvider::Fake(_) => true,
        }
    }

    pub(crate) fn new_live(base_address: &str) -> Self {
        CompletionProvider::Live(LanguageToolClient::new(base_address))
    }

    pub(crate) fn new_fake(fixture_id: &str) -> Result<Self, String> {
        FakeProvider::from_fixture(fixture_id).map(CompletionProvider::Fake)
    }

    pub(crate) fn update_address(&mut self, base_address: &str) {
        match self {
            CompletionProvider::Live(client) => client.update_address(base_address),
            CompletionProvider::Fake(fake) => fake.update_address(base_address),
        }
    }

    pub(crate) fn set_colors(&mut self, rules: Vec<(String, String)>) {
        match self {
            CompletionProvider::Live(client) => client.set_colors(rules),
            CompletionProvider::Fake(fake) => fake.set_colors(rules),
        }
    }

    pub(crate) fn address(&self) -> &str {
        match self {
            CompletionProvider::Live(client) => client.address(),
            CompletionProvider::Fake(fake) => fake.address(),
        }
    }
}
