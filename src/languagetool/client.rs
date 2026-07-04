use crate::{
    interop::bridge::ffi::{Range, Recommendation},
    languagetool::models::LanguageToolDto,
};

#[derive(Default)]
pub struct LanguageToolClient {
    address: String,
}

impl LanguageToolClient {
    pub fn new_local(port: &str) -> Self {
        Self {
            address: format!("http://localhost:{port}"),
        }
    }

    #[allow(dead_code)]
    pub fn new_remote(address: &str) -> Self {
        Self {
            address: address.trim().trim_end_matches("/").to_string(),
        }
    }
}

impl LanguageToolClient {
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
                // dbg!(&body);
                results = body
                    .matches
                    .into_iter()
                    .flat_map(|x| {
                        x.replacements
                            .into_iter()
                            .map(move |replacement| Recommendation {
                                color: "#FF0000".to_string(),
                                range: Range {
                                    start: x.offset,
                                    length: x.length,
                                },
                                value: replacement.value.to_string(),
                            })
                    })
                    .collect::<Vec<_>>();
            }
        }

        results
    }
}
