use rhesis::languagetool::models::LanguageToolDto;

/// A realistic (minimal) LanguageTool /v2/check response
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

#[test]
fn parses_full_response() {
    let dto: LanguageToolDto = serde_json::from_str(SAMPLE_JSON).expect("sample must parse");

    assert_eq!(dto.software.name, "LanguageTool");
    assert_eq!(dto.software.api_version, 1);
    assert!(!dto.software.premium);
    assert!(!dto.warnings.incomplete_results);

    assert_eq!(dto.language.code, "en-US");
    assert_eq!(dto.language.detected_language.source, "fasttext");
    assert_eq!(dto.language.detected_language.confidence, 0.95);

    assert_eq!(dto.matches.len(), 2);

    let first = &dto.matches[0];
    assert_eq!(first.offset, 17);
    assert_eq!(first.length, 3);
    assert_eq!(first.rule.id, "MORFOLOGIK_RULE_EN_US");
    assert_eq!(first.rule.category.id, "TYPOS");
    assert_eq!(first.match_type.type_name, "misspelling");
    assert_eq!(first.replacements.len(), 2);
    assert_eq!(first.replacements[0].value, "the");
    assert_eq!(first.context.text, "word teh test.");

    let second = &dto.matches[1];
    assert_eq!(second.offset, 5);
    assert_eq!(second.length, 1);
    assert_eq!(second.rule.id, "COMMA_PARENTHESIS_WHITESPACE");
    assert_eq!(second.rule.category.id, "PUNCTUATION");

    assert_eq!(dto.sentence_ranges, vec![vec![0, 36]]);
    assert_eq!(dto.extended_sentence_ranges.len(), 1);
    assert_eq!(dto.extended_sentence_ranges[0].to, 36);
    assert_eq!(
        dto.extended_sentence_ranges[0].detected_languages[0].language,
        "en-US"
    );
}

#[test]
fn round_trips_through_json() {
    let dto: LanguageToolDto = serde_json::from_str(SAMPLE_JSON).unwrap();
    let serialized = serde_json::to_string(&dto).unwrap();
    let reparsed: LanguageToolDto = serde_json::from_str(&serialized).unwrap();

    assert_eq!(dto, reparsed);
}

#[test]
fn missing_required_field_fails() {
    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
    value.as_object_mut().unwrap().remove("warnings");

    assert!(serde_json::from_str::<LanguageToolDto>(&value.to_string()).is_err());
}

#[test]
fn snake_case_keys_are_rejected() {
    // rename_all = "camelCase" is set, so snake_case keys must not parse
    let snake = SAMPLE_JSON.replace("\"incompleteResults\"", "\"incomplete_results\"");
    assert!(serde_json::from_str::<LanguageToolDto>(&snake).is_err());
}

#[test]
fn empty_matches_parse() {
    let mut value: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
    value["matches"] = serde_json::json!([]);

    let dto: LanguageToolDto = serde_json::from_value(value).unwrap();
    assert!(dto.matches.is_empty());
}
