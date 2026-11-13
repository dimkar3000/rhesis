use std::future::Future;

use rhesis_protocol::recommendation::Recommendation;

use crate::client::LanguageToolClient;

pub trait CompletionProvider: Send {
    fn check_async(
        &self,
        text: &str,
    ) -> impl Future<Output = Result<Vec<Recommendation>, String>> + Send;

    fn health_check_async(&self) -> impl Future<Output = bool> + Send;

    fn update_address(&mut self, base_address: &str);

    fn set_colors(&mut self, rules: Vec<(String, String)>);

    fn address(&self) -> &str;

    fn manages_local_server(&self) -> bool;
}

impl CompletionProvider for LanguageToolClient {
    async fn check_async(&self, text: &str) -> Result<Vec<Recommendation>, String> {
        self.get_recommendation(text).await
    }

    async fn health_check_async(&self) -> bool {
        LanguageToolClient::health_check(self).await
    }

    fn update_address(&mut self, base_address: &str) {
        LanguageToolClient::update_address(self, base_address)
    }

    fn set_colors(&mut self, rules: Vec<(String, String)>) {
        LanguageToolClient::set_colors(self, rules)
    }

    fn address(&self) -> &str {
        LanguageToolClient::address(self)
    }

    fn manages_local_server(&self) -> bool {
        true
    }
}
