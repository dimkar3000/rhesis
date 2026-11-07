use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use rhesis_languagetool::client::LanguageToolClient;
use rhesis_protocol::{Range, Recommendation};

/// Builds a full LanguageTool `/v2/check` response body in exactly the
/// JSON shape a real server sends — unmodeled sections (`software`,
/// `warnings`, the sentence ranges) and per-match extras (`shortMessage`,
/// `context`, `sentence`, `type`, ...) included — so the tier parses live
/// shapes, not echoes of our own structs. One match, two replacements:
/// the converter must turn it into two recommendations.
fn response_body(rule_id: &str, category_id: &str, offset: i32, length: i32) -> Vec<u8> {
    let response = serde_json::json!({
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
                "confidence": 0.99,
                "source": "fasttext"
            }
        },
        "matches": [{
            "message": "Possible spelling mistake found.",
            "shortMessage": "Spelling mistake",
            "replacements": [{ "value": "the" }, { "value": "teh" }],
            "offset": offset,
            "length": length,
            "context": {
                "text": "some context",
                "offset": offset,
                "length": length
            },
            "sentence": "A sentence.",
            "type": { "typeName": "misspelling" },
            "rule": {
                "id": rule_id,
                "description": "Possible spelling mistake",
                "issueType": "misspelling",
                "category": { "id": category_id, "name": "Possible Typo" }
            },
            "ignoreForIncompleteSentence": false,
            "contextForSureMatch": 0
        }],
        "sentenceRanges": [[0, 20]],
        "extendedSentenceRanges": []
    });
    serde_json::to_vec(&response).unwrap()
}

/// The zero-findings answer: full envelope, no matches.
fn empty_response_body() -> Vec<u8> {
    let response = serde_json::json!({
        "software": { "name": "LanguageTool", "version": "6.9-SNAPSHOT" },
        "warnings": { "incompleteResults": false },
        "language": { "name": "English (US)", "code": "en-US" },
        "matches": [],
        "sentenceRanges": [],
        "extendedSentenceRanges": []
    });
    serde_json::to_vec(&response).unwrap()
}

/// Spawns a mock LanguageTool HTTP server replying to every request with `response`.
/// Returns the port it listens on.
fn mock_server(response: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => break,
            };

            // Read the request headers so the client never sees a broken pipe
            let mut data = Vec::new();
            while data.len() < 8192 && !data.windows(4).any(|w| w == b"\r\n\r\n") {
                let mut chunk = [0u8; 1024];
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => data.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }

            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });
    port
}

fn recommendation_ranges(recommendations: &[Recommendation]) -> Vec<Range> {
    recommendations.iter().map(|r| r.range).collect()
}

#[tokio::test]
async fn returns_recommendations_from_server() {
    let port = mock_server(response_body("MORFOLOGIK_RULE_EN_US", "TYPOS", 17, 3));

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let recommendations = client
        .get_recommendation("some misspelled text")
        .await
        .unwrap();

    assert_eq!(recommendations.len(), 2, "one match with two replacements");
    let first = &recommendations[0];
    assert_eq!(
        first.range,
        Range {
            start: 17,
            length: 3
        }
    );
    assert_eq!(first.value, "the");
    assert_eq!(first.rule_id, "MORFOLOGIK_RULE_EN_US");
    assert_eq!(first.category_id, "TYPOS");
    assert_eq!(first.tooltip, "Possible spelling mistake found.");
    assert_eq!(first.language, "EN-US", "language code is uppercased");
    assert_eq!(recommendations[1].value, "teh");
}

#[tokio::test]
async fn returns_empty_when_no_matches() {
    let port = mock_server(empty_response_body());

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let recommendations = client.get_recommendation("a perfect text").await.unwrap();

    assert!(recommendations.is_empty());
}

#[tokio::test]
async fn applies_configured_rule_colors() {
    let port = mock_server(response_body("MORFOLOGIK_RULE_EN_US", "TYPOS", 3, 5));

    let mut client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    client.set_colors(vec![
        ("CATEGORY:TYPOS".to_string(), "#33cc33".to_string()),
        (
            "RULE:MORFOLOGIK_RULE_EN_US".to_string(),
            "#11bb22".to_string(),
        ),
    ]);

    let recommendations = client.get_recommendation("misspelled text").await.unwrap();
    assert_eq!(
        recommendations[0].color, "#11bb22",
        "rule color overrides category color"
    );
}

#[tokio::test]
async fn falls_back_to_category_then_default_color() {
    let port = mock_server(response_body("MORFOLOGIK_RULE_EN_US", "TYPOS", 3, 5));

    let mut client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    client.set_colors(vec![("CATEGORY:TYPOS".to_string(), "#abcdab".to_string())]);

    let recommendations = client.get_recommendation("misspelled text").await.unwrap();
    assert_eq!(recommendations[0].color, "#abcdab");

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let recommendations = client.get_recommendation("misspelled text").await.unwrap();
    assert_eq!(
        recommendations[0].color, "#FF0000",
        "unconfigured rules fall back to the default red"
    );
}

#[tokio::test]
async fn errors_on_error_status() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => break,
            };
            let mut data = [0u8; 4096];
            let _ = stream.read(&mut data);
            let _ = stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let error = client.get_recommendation("text").await.unwrap_err();
    assert!(
        error.contains("HTTP 500"),
        "unexpected error message: {error}"
    );
}

#[tokio::test]
async fn errors_on_garbage_response() {
    let port = mock_server(b"this is not json".to_vec());

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let error = client.get_recommendation("text").await.unwrap_err();
    assert!(
        error.contains("unexpected response"),
        "unexpected error message: {error}"
    );
}

#[tokio::test]
async fn errors_when_server_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let error = client.get_recommendation("text").await.unwrap_err();
    assert!(
        error.contains("no response"),
        "unexpected error message: {error}"
    );
}

#[tokio::test]
async fn update_address_switches_server() {
    let port_a = mock_server(response_body("RULE_A", "TYPOS", 1, 2));
    let port_b = mock_server(response_body("RULE_B", "GRAMMAR", 3, 4));

    let mut client = LanguageToolClient::new(format!("127.0.0.1:{port_a}"));
    let first = client.get_recommendation("text").await.unwrap();
    assert_eq!(first[0].rule_id, "RULE_A");

    client.update_address(format!("127.0.0.1:{port_b}"));
    let second = client.get_recommendation("text").await.unwrap();
    assert_eq!(second[0].rule_id, "RULE_B");
}

#[tokio::test]
async fn bare_hosts_get_http_scheme() {
    // A bare address (no scheme) must still produce a working request;
    // this guards against regressions of the missing http:// bug
    let port = mock_server(response_body("MORFOLOGIK_RULE_EN_US", "TYPOS", 0, 1));

    let client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    let recommendations = client.get_recommendation("text").await.unwrap();

    assert!(!recommendations.is_empty());
    // One match with two replacements -> two recommendations sharing a range
    assert_eq!(
        recommendation_ranges(&recommendations),
        vec![
            Range {
                start: 0,
                length: 1
            };
            2
        ]
    );
}

#[tokio::test]
async fn update_colors_takes_effect_immediately() {
    let port = mock_server(response_body("RULE_X", "TYPOS", 2, 4));

    let mut client = LanguageToolClient::new(format!("127.0.0.1:{port}"));
    client.set_colors(vec![("RULE:RULE_X".to_string(), "#00ff00".to_string())]);

    let recommendations = client.get_recommendation("text").await.unwrap();
    assert_eq!(recommendations[0].color, "#00ff00");

    client.set_colors(vec![("RULE:RULE_X".to_string(), "#ff0001".to_string())]);
    let recommendations = client.get_recommendation("text").await.unwrap();
    assert_eq!(recommendations[0].color, "#ff0001");
}

#[test]
fn client_addresses_normalize_on_construction_and_update() {
    let mut client = LanguageToolClient::new("localhost:2689/");
    assert_eq!(client.address(), "http://localhost:2689");
    client.update_address("https://lt.example.com////");
    assert_eq!(client.address(), "https://lt.example.com");
}
