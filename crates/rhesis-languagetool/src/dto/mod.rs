use serde::Deserialize;

mod language;
mod matches;

pub use language::Language;
pub use matches::{Category, Match, Replacement, Rule};

#[derive(Debug, Deserialize)]
pub struct LanguageToolDto {
    pub language: Language,
    pub matches: Vec<Match>,
}
