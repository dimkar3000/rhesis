use std::collections::HashMap;

use cxx_qt_lib::QString;

use crate::{
    interop::recommendation::{Range, Recommendation},
    languagetool::models::LanguageToolDto,
};

#[derive(Default)]
pub struct LanguageToolClient {
    address: String,
    rules: HashMap<String, QString>,
}

impl LanguageToolClient {
    pub fn new<T: AsRef<str>>(host: T, port: u16) -> LanguageToolClient {
        let adress = Self::normalize_address(host.as_ref());

        Self {
            address: format!("{}:{}", adress, port),
            ..Default::default()
        }
    }

    pub fn update_address<T: AsRef<str>>(&mut self, host: T, port: u16) {
        let adress = Self::normalize_address(host.as_ref());
        self.address = format!("{}:{}", adress, port);
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

    /// reqwest needs a full URL; bare addresses get the http:// scheme
    fn normalize_address(address: &str) -> String {
        let address = address.trim().trim_end_matches('/');
        if address.contains("://") {
            address.to_string()
        } else {
            format!("http://{address}")
        }
    }
}

impl LanguageToolClient {
    pub fn set_colors(&mut self, rules: Vec<(QString, QString)>) {
        log::debug!("applying {} color rules", rules.len());
        self.rules.clear();
        for (key, value) in rules {
            self.rules.insert(key.to_string(), value);
        }
    }

    fn select_color(&self, rule_id: QString, category_id: QString) -> QString {
        let mut color = QString::from("#FF0000");

        let category_key = format!("CATEGORY:{}", category_id);
        if self.rules.contains_key(&category_key) {
            color = self.rules[&category_key].clone();
        }

        let rule_key = format!("RULE:{}", rule_id);
        if self.rules.contains_key(&rule_key) {
            color = self.rules[&rule_key].clone();
        }

        log::debug!("Color Selected: {}", color);
        log::debug!("Category: {}", category_key);
        log::debug!("Rule: {}", rule_key);
        log::debug!("Rules: {:?}", self.rules);

        color
    }

    pub async fn get_recommendation(&self, input: impl AsRef<str>) -> Vec<Recommendation> {
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
            Err(e) => {
                // Transient: the worker retries on the next text change
                log::warn!("LanguageTool request to {} failed: {e:?}", self.address);
                return vec![];
            }
        };

        if response.status() != 200 {
            log::warn!(
                "LanguageTool returned status {} for {}",
                response.status(),
                self.address
            );
            return vec![];
        }

        let body = match response.json::<LanguageToolDto>().await {
            Ok(body) => body,
            Err(e) => {
                // The server replied, but with an unexpected shape
                log::error!("failed to parse LanguageTool response: {e:?}");
                return vec![];
            }
        };

        let lang_tag = body.language.code.to_uppercase();
        body.matches
            .into_iter()
            .flat_map(|x| {
                let lang_tag = lang_tag.clone();
                x.replacements
                    .into_iter()
                    .map(move |replacement| Recommendation {
                        color: self.select_color(
                            QString::from(x.rule.id.as_ref()),
                            QString::from(x.rule.category.id.as_ref()),
                        ),
                        range: Range {
                            start: x.offset,
                            length: x.length,
                        },
                        value: QString::from(replacement.value.as_ref()),
                        category_id: QString::from(x.rule.category.id.as_ref()),
                        rule_id: QString::from(x.rule.id.as_ref()),
                        tooltip: QString::from(x.message.as_ref()),
                        language: QString::from(&lang_tag),
                    })
            })
            .collect()
    }
}
