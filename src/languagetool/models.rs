use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageToolDto<'a> {
    pub software: Software<'a>,
    pub warnings: Warnings,
    pub language: Language<'a>,
    pub matches: Vec<Match<'a>>,
    pub sentence_ranges: Vec<Vec<i32>>,
    pub extended_sentence_ranges: Vec<ExtendedSentenceRange<'a>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedSentenceRange<'a> {
    pub from: i32,
    pub to: i32,
    pub detected_languages: Vec<DetectedLanguageElement<'a>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedLanguageElement<'a> {
    pub language: Cow<'a, str>,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Language<'a> {
    pub name: Cow<'a, str>,
    pub code: Cow<'a, str>,
    pub detected_language: LanguageDetectedLanguage<'a>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageDetectedLanguage<'a> {
    pub name: Cow<'a, str>,
    pub code: Cow<'a, str>,
    pub confidence: f64,
    pub source: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match<'a> {
    pub message: Cow<'a, str>,
    pub short_message: Cow<'a, str>,
    pub replacements: Vec<Replacement<'a>>,
    pub offset: i32,
    pub length: i32,
    pub context: Context<'a>,
    pub sentence: Cow<'a, str>,
    #[serde(rename = "type")]
    pub match_type: Type<'a>,
    pub rule: Rule<'a>,
    pub ignore_for_incomplete_sentence: bool,
    pub context_for_sure_match: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context<'a> {
    pub text: Cow<'a, str>,
    pub offset: i32,
    pub length: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Type<'a> {
    pub type_name: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replacement<'a> {
    pub value: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule<'a> {
    pub id: Cow<'a, str>,
    pub description: Cow<'a, str>,
    pub issue_type: Cow<'a, str>,
    pub category: Category<'a>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category<'a> {
    pub id: Cow<'a, str>,
    pub name: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Software<'a> {
    pub name: Cow<'a, str>,
    pub version: Cow<'a, str>,
    pub build_date: Cow<'a, str>,
    pub api_version: i32,
    pub premium: bool,
    pub premium_hint: Cow<'a, str>,
    pub status: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warnings {
    pub incomplete_results: bool,
}
