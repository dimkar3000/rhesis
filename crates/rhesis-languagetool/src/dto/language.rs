use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Language {
    pub code: String,
}
