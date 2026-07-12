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
    pub fn new_local(port: &str) -> Self {
        Self {
            address: format!("http://localhost:{port}"),
            ..Default::default()
        }
    }

    #[allow(dead_code)]
    pub fn new_remote(address: &str) -> Self {
        Self {
            address: address.trim().trim_end_matches('/').into(),
            ..Default::default()
        }
    }
}

impl LanguageToolClient {
    pub fn set_colors(&mut self, rules: Vec<(QString, QString)>) {
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

        let mut results = Vec::new();

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

        if let Ok(response) = response {
            if response.status() == 200 {
                let body = response.json::<LanguageToolDto>().await;

                if let Err(e) = body {
                    dbg!("failed to get response: {:?}", e);
                    return vec![];
                }

                let body = body.unwrap();
                let lang_tag = body.language.code.to_uppercase();
                results = body
                    .matches
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
            }
        }

        results
    }
}
