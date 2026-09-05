use std::{
    borrow::Cow,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use cxx_qt_lib::QString;
use rhesis::{
    interop::recommendation::Range,
    languagetool::{
        client::LanguageToolClient,
        models::{
            Category, Context, Language, LanguageDetectedLanguage, LanguageToolDto, Match,
            Replacement, Rule, Software, Type, Warnings,
        },
    },
};

/// Builds a minimal LanguageTool response with a single match
fn make_dto(
    rule_id: &str,
    category_id: &str,
    offset: i32,
    length: i32,
) -> LanguageToolDto<'static> {
    LanguageToolDto {
        software: Software {
            name: Cow::Borrowed("LanguageTool"),
            version: Cow::Borrowed("test"),
            build_date: Cow::Borrowed(""),
            api_version: 1,
            premium: false,
            premium_hint: Cow::Borrowed(""),
            status: Cow::Borrowed(""),
        },
        warnings: Warnings {
            incomplete_results: false,
        },
        language: Language {
            name: Cow::Borrowed("English (US)"),
            code: Cow::Borrowed("en-US"),
            detected_language: LanguageDetectedLanguage {
                name: Cow::Borrowed("English (US)"),
                code: Cow::Borrowed("en-US"),
                confidence: 0.99,
                source: Cow::Borrowed("fasttext"),
            },
        },
        matches: vec![Match {
            message: Cow::Borrowed("Possible spelling mistake found."),
            short_message: Cow::Borrowed("Spelling mistake"),
            replacements: vec![
                Replacement {
                    value: Cow::Borrowed("the"),
                },
                Replacement {
                    value: Cow::Borrowed("teh"),
                },
            ],
            offset,
            length,
            context: Context {
                text: Cow::Borrowed("some context"),
                offset,
                length,
            },
            sentence: Cow::Borrowed("A sentence."),
            match_type: Type {
                type_name: Cow::Borrowed("misspelling"),
            },
            rule: Rule {
                id: Cow::Owned(rule_id.to_string()),
                description: Cow::Borrowed("Possible spelling mistake"),
                issue_type: Cow::Borrowed("misspelling"),
                category: Category {
                    id: Cow::Owned(category_id.to_string()),
                    name: Cow::Borrowed("Possible Typo"),
                },
            },
            ignore_for_incomplete_sentence: false,
            context_for_sure_match: 0,
        }],
        sentence_ranges: vec![vec![0, 20]],
        extended_sentence_ranges: vec![],
    }
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

fn json_response(dto: &LanguageToolDto) -> Vec<u8> {
    serde_json::to_vec(dto).unwrap()
}

fn recommendation_ranges(
    recommendations: &[rhesis::interop::recommendation::Recommendation],
) -> Vec<Range> {
    recommendations.iter().map(|r| r.range).collect()
}

#[tokio::test]
async fn returns_recommendations_from_server() {
    let port = mock_server(json_response(&make_dto(
        "MORFOLOGIK_RULE_EN_US",
        "TYPOS",
        17,
        3,
    )));

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("some misspelled text").await;

    assert_eq!(recommendations.len(), 2, "one match with two replacements");
    let first = &recommendations[0];
    assert_eq!(
        first.range,
        Range {
            start: 17,
            length: 3
        }
    );
    assert_eq!(first.value.to_string(), "the");
    assert_eq!(first.rule_id.to_string(), "MORFOLOGIK_RULE_EN_US");
    assert_eq!(first.category_id.to_string(), "TYPOS");
    assert_eq!(
        first.tooltip.to_string(),
        "Possible spelling mistake found."
    );
    assert_eq!(
        first.language.to_string(),
        "EN-US",
        "language code is uppercased"
    );
    assert_eq!(recommendations[1].value.to_string(), "teh");
}

#[tokio::test]
async fn returns_empty_when_no_matches() {
    let mut dto = make_dto("MORFOLOGIK_RULE_EN_US", "TYPOS", 0, 3);
    dto.matches.clear();
    let port = mock_server(json_response(&dto));

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("a perfect text").await;

    assert!(recommendations.is_empty());
}

#[tokio::test]
async fn applies_configured_rule_colors() {
    let dto = make_dto("MORFOLOGIK_RULE_EN_US", "TYPOS", 3, 5);
    let port = mock_server(json_response(&dto));

    let mut client = LanguageToolClient::new("127.0.0.1", port);
    client.set_colors(vec![
        (QString::from("CATEGORY:TYPOS"), QString::from("#33cc33")),
        (
            QString::from("RULE:MORFOLOGIK_RULE_EN_US"),
            QString::from("#11bb22"),
        ),
    ]);

    let recommendations = client.get_recommendation("misspelled text").await;
    assert_eq!(
        recommendations[0].color.to_string(),
        "#11bb22",
        "rule color overrides category color"
    );
}

#[tokio::test]
async fn falls_back_to_category_then_default_color() {
    let dto = make_dto("MORFOLOGIK_RULE_EN_US", "TYPOS", 3, 5);
    let port = mock_server(json_response(&dto));

    let mut client = LanguageToolClient::new("127.0.0.1", port);
    client.set_colors(vec![(
        QString::from("CATEGORY:TYPOS"),
        QString::from("#abcdab"),
    )]);

    let recommendations = client.get_recommendation("misspelled text").await;
    assert_eq!(recommendations[0].color.to_string(), "#abcdab");

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("misspelled text").await;
    assert_eq!(
        recommendations[0].color.to_string(),
        "#FF0000",
        "unconfigured rules fall back to the default red"
    );
}

#[tokio::test]
async fn returns_empty_on_error_status() {
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

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("text").await;
    assert!(recommendations.is_empty());
}

#[tokio::test]
async fn returns_empty_on_garbage_response() {
    let port = mock_server(b"this is not json".to_vec());

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("text").await;
    assert!(recommendations.is_empty());
}

#[tokio::test]
async fn returns_empty_when_server_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("text").await;
    assert!(recommendations.is_empty());
}

#[tokio::test]
async fn update_address_switches_server() {
    let port_a = mock_server(json_response(&make_dto("RULE_A", "TYPOS", 1, 2)));
    let port_b = mock_server(json_response(&make_dto("RULE_B", "GRAMMAR", 3, 4)));

    let mut client = LanguageToolClient::new("127.0.0.1", port_a);
    let first = client.get_recommendation("text").await;
    assert_eq!(first[0].rule_id.to_string(), "RULE_A");

    client.update_address("127.0.0.1", port_b);
    let second = client.get_recommendation("text").await;
    assert_eq!(second[0].rule_id.to_string(), "RULE_B");
}

#[tokio::test]
async fn bare_hosts_get_http_scheme() {
    // A bare address (no scheme) must still produce a working request;
    // this guards against regressions of the missing http:// bug
    let dto = make_dto("MORFOLOGIK_RULE_EN_US", "TYPOS", 0, 1);
    let port = mock_server(json_response(&dto));

    let client = LanguageToolClient::new("127.0.0.1", port);
    let recommendations = client.get_recommendation("text").await;

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
    let dto = make_dto("RULE_X", "TYPOS", 2, 4);
    let port = mock_server(json_response(&dto));

    let mut client = LanguageToolClient::new("127.0.0.1", port);
    client.set_colors(vec![(
        QString::from("RULE:RULE_X"),
        QString::from("#00ff00"),
    )]);

    let recommendations = client.get_recommendation("text").await;
    assert_eq!(recommendations[0].color.to_string(), "#00ff00");

    client.set_colors(vec![(
        QString::from("RULE:RULE_X"),
        QString::from("#ff0001"),
    )]);
    let recommendations = client.get_recommendation("text").await;
    assert_eq!(recommendations[0].color.to_string(), "#ff0001");
}
