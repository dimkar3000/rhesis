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
    pub fn new<T: AsRef<str>>(base_url: T) -> LanguageToolClient {
        Self {
            address: Self::normalize_address(base_url.as_ref()),
            ..Default::default()
        }
    }

    /// Point the client at a new server base URL (`http://localhost:2689`,
    /// `https://lt.example.com`, ...). Bare addresses get the `http://` scheme.
    pub fn update_address<T: AsRef<str>>(&mut self, base_url: T) {
        self.address = Self::normalize_address(base_url.as_ref());
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// reqwest needs a full URL; bare addresses get the http:// scheme
    pub fn normalize_address(address: &str) -> String {
        let address = address.trim().trim_end_matches('/');
        if address.contains("://") {
            address.to_string()
        } else {
            format!("http://{address}")
        }
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

    pub async fn get_recommendation(&self, input: impl AsRef<str>) -> Result<Vec<Recommendation>, String> {
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

        let lang_tag = body.language.code.to_uppercase();
        let recommendations = body.matches
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
            .collect::<Vec<_>>();

        Ok(recommendations)
    }
}
