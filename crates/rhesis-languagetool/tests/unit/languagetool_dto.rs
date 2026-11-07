//! serde behavior of the trimmed `/v2/check` DTOs: a full real-server
//! answer parses into exactly the fields the worker consumes — the
//! unmodeled sections are skipped like any other unknown key — and so does
//! an answer carrying only the modeled keys.

use rhesis_languagetool::dto::LanguageToolDto;

/// A realistic LanguageTool `/v2/check` response, the same recorded shape
/// the fixtures carry: every section the server sends, modeled or not.
const SAMPLE_JSON: &str = r#"{
  "software": {
    "name": "LanguageTool",
    "version": "6.9-SNAPSHOT",
    "buildDate": "2026-08-06",
    "apiVersion": 1,
    "premium": false,
    "premiumHint": "",
    "status": ""
  },
  "warnings": { "incompleteResults": false },
  "language": {
    "name": "English (US)",
    "code": "en-US",
    "detectedLanguage": {
      "name": "English (US)",
      "code": "en-US",
      "confidence": 0.95,
      "source": "fasttext"
    }
  },
  "matches": [
    {
      "message": "Possible spelling mistake found.",
      "shortMessage": "Spelling mistake",
      "replacements": [
        { "value": "the" },
        { "value": "teh" }
      ],
      "offset": 17,
      "length": 3,
      "context": {
        "text": "word teh test.",
        "offset": 5,
        "length": 3
      },
      "sentence": "This is a test with the word teh test.",
      "type": { "typeName": "misspelling" },
      "rule": {
        "id": "MORFOLOGIK_RULE_EN_US",
        "description": "Possible spelling mistake",
        "issueType": "misspelling",
        "category": { "id": "TYPOS", "name": "Possible Typo" }
      },
      "ignoreForIncompleteSentence": false,
      "contextForSureMatch": 0
    },
    {
      "message": "Possible typo: you may have forgot to leave a space.",
      "shortMessage": "Possible typo",
      "replacements": [{ "value": ", " }],
      "offset": 5,
      "length": 1,
      "context": {
        "text": "test,word",
        "offset": 4,
        "length": 2
      },
      "sentence": "This is a test,word",
      "type": { "typeName": "other" },
      "rule": {
        "id": "COMMA_PARENTHESIS_WHITESPACE",
        "description": "Comma is not followed by whitespace",
        "issueType": "typographical",
        "category": { "id": "PUNCTUATION", "name": "Punctuation" }
      },
      "ignoreForIncompleteSentence": false,
      "contextForSureMatch": 0
    }
  ],
  "sentenceRanges": [[0, 36]],
  "extendedSentenceRanges": [
    {
      "from": 0,
      "to": 36,
      "detectedLanguages": [{ "language": "en-US", "rate": 0.95 }]
    }
  ]
}"#;

/// The same answer stripped to the consumed keys only.
const MINIMAL_JSON: &str = r#"{
  "language": { "code": "en-US" },
  "matches": [{
    "message": "Possible spelling mistake found.",
    "replacements": [{ "value": "the" }],
    "offset": 17,
    "length": 3,
    "rule": {
      "id": "MORFOLOGIK_RULE_EN_US",
      "category": { "id": "TYPOS" }
    }
  }]
}"#;

#[test]
fn parses_full_response_into_consumed_fields() {
    let dto: LanguageToolDto = serde_json::from_str(SAMPLE_JSON).expect("sample must parse");

    // The one field the converter lifts out from `language`.
    assert_eq!(dto.language.code, "en-US");

    assert_eq!(dto.matches.len(), 2);

    let first = &dto.matches[0];
    assert_eq!(first.message, "Possible spelling mistake found.");
    assert_eq!(first.offset, 17);
    assert_eq!(first.length, 3);
    assert_eq!(first.replacements.len(), 2);
    assert_eq!(first.replacements[0].value, "the");
    assert_eq!(first.rule.id, "MORFOLOGIK_RULE_EN_US");
    assert_eq!(first.rule.category.id, "TYPOS");

    assert_eq!(dto.matches[1].rule.id, "COMMA_PARENTHESIS_WHITESPACE");
    assert_eq!(dto.matches[1].rule.category.id, "PUNCTUATION");
}

#[test]
fn unmodeled_fields_are_ignored() {
    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();

    // Top-level sections the server sends but the models stopped carrying:
    // removing them must not change what parses.
    for section in [
        "software",
        "warnings",
        "sentenceRanges",
        "extendedSentenceRanges",
    ] {
        value.as_object_mut().unwrap().remove(section);
        let dto: LanguageToolDto = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(dto.matches.len(), 2);
        assert_eq!(dto.language.code, "en-US");
    }

    // The same ignore rule holds at any nesting depth: per-match extras,
    // per-rule extras and language extras are all unmodeled.
    for field in [
        "shortMessage",
        "context",
        "sentence",
        "type",
        "ignoreForIncompleteSentence",
        "contextForSureMatch",
    ] {
        value["matches"][0].as_object_mut().unwrap().remove(field);
    }
    value["matches"][0]["rule"]
        .as_object_mut()
        .unwrap()
        .remove("description");
    value["matches"][0]["rule"]
        .as_object_mut()
        .unwrap()
        .remove("issueType");
    value["matches"][0]["rule"]["category"]
        .as_object_mut()
        .unwrap()
        .remove("name");
    value["language"].as_object_mut().unwrap().remove("name");
    value["language"]
        .as_object_mut()
        .unwrap()
        .remove("detectedLanguage");

    let dto: LanguageToolDto = serde_json::from_value(value).unwrap();
    assert_eq!(dto.matches.len(), 2);
    // The consumed neighbors survived the stripping.
    assert_eq!(dto.matches[0].offset, 17);
    assert_eq!(dto.matches[0].rule.id, "MORFOLOGIK_RULE_EN_US");
    assert_eq!(dto.matches[0].rule.category.id, "TYPOS");
}

#[test]
fn parses_minimal_answer() {
    // Deserialization works without the fields we dropped: an answer
    // carrying only the consumed keys is enough.
    let dto: LanguageToolDto = serde_json::from_str(MINIMAL_JSON).expect("minimal must parse");
    assert_eq!(dto.language.code, "en-US");
    assert_eq!(dto.matches.len(), 1);
    assert_eq!(dto.matches[0].offset, 17);
    assert_eq!(dto.matches[0].replacements[0].value, "the");
    assert_eq!(dto.matches[0].rule.category.id, "TYPOS");
}

#[test]
fn missing_consumed_field_fails() {
    // The consumed fields stay required: a language-less or match-less
    // answer is a broken payload, not an empty suggestion list.
    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
    value.as_object_mut().unwrap().remove("language");
    assert!(serde_json::from_str::<LanguageToolDto>(&value.to_string()).is_err());

    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
    value.as_object_mut().unwrap().remove("matches");
    assert!(serde_json::from_str::<LanguageToolDto>(&value.to_string()).is_err());

    let mut value: serde_json::Value = serde_json::from_str(MINIMAL_JSON).unwrap();
    value["matches"][0]
        .as_object_mut()
        .unwrap()
        .remove("offset");
    assert!(serde_json::from_str::<LanguageToolDto>(&value.to_string()).is_err());
}

#[test]
fn empty_matches_parse() {
    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
    value["matches"] = serde_json::json!([]);

    let dto: LanguageToolDto = serde_json::from_value(value).unwrap();
    assert!(dto.matches.is_empty());
}
