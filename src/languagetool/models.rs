use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageToolDto {
    pub software: Software,
    pub warnings: Warnings,
    pub language: Language,
    pub matches: Vec<Match>,
    pub sentence_ranges: Vec<Vec<i32>>,
    pub extended_sentence_ranges: Vec<ExtendedSentenceRange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedSentenceRange {
    pub from: i32,
    pub to: i32,
    pub detected_languages: Vec<DetectedLanguageElement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedLanguageElement {
    pub language: String,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub name: String,
    pub code: String,
    pub detected_language: LanguageDetectedLanguage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageDetectedLanguage {
    pub name: String,
    pub code: String,
    pub confidence: f64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub message: String,
    pub short_message: String,
    pub replacements: Vec<Replacement>,
    pub offset: i32,
    pub length: i32,
    pub context: Context,
    pub sentence: String,
    #[serde(rename = "type")]
    pub match_type: Type,
    pub rule: Rule,
    pub ignore_for_incomplete_sentence: bool,
    pub context_for_sure_match: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub text: String,
    pub offset: i32,
    pub length: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Type {
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replacement {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub issue_type: String,
    pub category: Category,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Software {
    pub name: String,
    pub version: String,
    pub build_date: String,
    pub api_version: i32,
    pub premium: bool,
    pub premium_hint: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warnings {
    pub incomplete_results: bool,
}
