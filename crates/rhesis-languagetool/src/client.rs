use crate::{
    helpers::{normalize_address, ColorRules},
    models::LanguageToolDto,
    recommendation::{Range, Recommendation},
};

#[derive(Default)]
pub struct LanguageToolClient {
    address: String,
    rules: ColorRules,
}

impl LanguageToolClient {
    pub fn new<T: AsRef<str>>(base_url: T) -> LanguageToolClient {
        Self {
            address: crate::helpers::normalize_address(base_url.as_ref()),
            ..Default::default()
        }
    }

    pub fn update_address<T: AsRef<str>>(&mut self, base_url: T) {
        self.address = normalize_address(base_url.as_ref());
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// Lightweight readiness probe: true when the server answers `POST /v2/check`.
    pub async fn health_check(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let form_data = [
            ("text", "ok"),
            ("language", "en-US"),
            ("enabledOnly", "false"),
        ];

        match client
            .post(format!("{}/v2/check", self.address))
            .form(&form_data)
            .send()
            .await
        {
            Ok(resp) => resp.status() == 200,
            Err(_) => false,
        }
    }

    pub fn set_colors(&mut self, rules: Vec<(String, String)>) {
        log::debug!("applying {} color rules", rules.len());
        self.rules = ColorRules::from_wire(rules);
    }

    pub async fn get_recommendation(
        &self,
        input: impl AsRef<str>,
    ) -> Result<Vec<Recommendation>, String> {
        let input = input.as_ref();

        let client = reqwest::Client::new();

        let form_data = [
            ("text", input),
            ("language", "auto"),
            ("enabledOnly", "false"),
        ];

        let response = client
            .post(format!("{}/v2/check", self.address))
            .form(&form_data)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            // Transient: the worker marks the status and the next text change retries
            Err(e) => {
                return Err(format!("no response from {}: {}", self.address, e));
            }
        };

        if response.status() != 200 {
            return Err(format!("LanguageTool returned HTTP {}", response.status()));
        }

        let body = match response.json::<LanguageToolDto>().await {
            Ok(body) => body,
            Err(e) => {
                // The server replied, but with an unexpected shape
                return Err(format!("unexpected response from {}: {}", self.address, e));
            }
        };

        let recommendations = map_dto(body, &self.rules);

        Ok(recommendations)
    }
}

/// Convert a LanguageTool response body into UI recommendations using the
/// given color rules. Shared by the live client (HTTP answers) and the fake
/// provider (recorded fixtures), so fixtures carry the real conversion.
pub(crate) fn map_dto(body: LanguageToolDto, rules: &ColorRules) -> Vec<Recommendation> {
    const DEFAULT_COLOR: &str = "#FF0000";

    let language = body.language.code.to_uppercase();
    let mut recommendations = Vec::new();
    for x in body.matches {
        // Per match (independent of replacements): look the color up
        // once and move the DTO's own strings out — no re-copies.
        let color = rules
            .color(&x.rule.id, &x.rule.category.id)
            .unwrap_or(DEFAULT_COLOR)
            .to_owned();
        let rule_id = x.rule.id;
        let category_id = x.rule.category.id;
        let tooltip = x.message;
        let range = Range {
            start: x.offset,
            length: x.length,
        };
        for replacement in x.replacements {
            recommendations.push(Recommendation {
                color: color.clone(),
                range,
                value: replacement.value,
                category_id: category_id.clone(),
                rule_id: rule_id.clone(),
                tooltip: tooltip.clone(),
                language: language.clone(),
            });
        }
    }
    recommendations
}
