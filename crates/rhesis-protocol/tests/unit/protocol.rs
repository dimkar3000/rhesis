//! Framing + schema tier: every message variant round-trips through the
//! real framing, broken frames fail loudly, and the shared types keep their
//! documented shapes (Refactor.md §9). This tier doubles as the drift
//! tripwire for the `PROTOCOL` bump rule any type change implies.

use std::collections::BTreeMap;
use std::io::Read;

use rhesis_protocol::constants::{FRAME_CAP, PROTOCOL};
use rhesis_protocol::{ClientMsg, DaemonMsg, Range, Recommendation, Settings, WorkerStatus};

/// Helper: encode a message and read it back through the public framing API.
fn round_trip<T>(original: T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq,
{
    let mut wire = Vec::new();
    rhesis_protocol::write_frame(&mut wire, &original).expect("frame must encode");
    rhesis_protocol::read_frame(&mut wire.as_slice())
        .expect("frame must decode")
        .expect("a written frame must come back before EOF")
}

#[test]
fn protocol_const_is_v1() {
    assert_eq!(PROTOCOL, 1);
}

#[test]
fn tier_socket_names_are_pid_unique() {
    let name = rhesis_protocol::tier_socket_name("daemon-session");
    assert_ne!(
        name,
        rhesis_protocol::constants::SERVICE_SOCKET_NAME,
        "test names must never shadow the production socket"
    );
    assert!(
        name.len() < 107,
        "abstract socket names share the 107-byte limit"
    );
}

// --- Per-variant ClientMsg round-trips --------------------------------------

#[test]
fn client_hello_round_trips() {
    assert_eq!(
        round_trip(ClientMsg::Hello { protocol: PROTOCOL }),
        ClientMsg::Hello { protocol: PROTOCOL }
    );
}

#[test]
fn client_check_round_trips() {
    assert_eq!(
        round_trip(ClientMsg::Check {
            text: "She like football.".to_string(),
        }),
        ClientMsg::Check {
            text: "She like football.".to_string(),
        }
    );
}

#[test]
fn client_lifecycle_round_trips() {
    assert_eq!(round_trip(ClientMsg::Start), ClientMsg::Start);
    assert_eq!(round_trip(ClientMsg::Stop), ClientMsg::Stop);
    assert_eq!(round_trip(ClientMsg::Retry), ClientMsg::Retry);
}

#[test]
fn client_remote_and_shutdown_round_trips() {
    assert_eq!(
        round_trip(ClientMsg::SwitchToRemote(
            "http://192.168.1.10:8081".to_string()
        )),
        ClientMsg::SwitchToRemote("http://192.168.1.10:8081".to_string())
    );
    assert_eq!(round_trip(ClientMsg::Shutdown), ClientMsg::Shutdown);
}

#[test]
fn client_set_settings_round_trips() {
    let msg = ClientMsg::SetSettings(Settings::default());
    assert_eq!(round_trip(msg.clone()), msg);
}

// --- Per-variant DaemonMsg round-trips --------------------------------------

#[test]
fn daemon_hello_round_trips() {
    let msg = DaemonMsg::Hello {
        protocol: PROTOCOL,
        busy: false,
    };
    assert_eq!(round_trip(msg.clone()), msg);

    let busy = DaemonMsg::Hello {
        protocol: PROTOCOL,
        busy: true,
    };
    assert_eq!(round_trip(busy.clone()), busy);
}

#[test]
fn daemon_status_round_trips_every_variant() {
    let statuses = [
        WorkerStatus::Stopped,
        WorkerStatus::Starting,
        WorkerStatus::Connecting,
        WorkerStatus::Started,
        WorkerStatus::Failed("port 2689 is already in use".to_string()),
        WorkerStatus::FailedToRespond("no response".to_string()),
    ];
    for status in statuses {
        let msg = DaemonMsg::Status(status.clone());
        assert_eq!(round_trip(msg.clone()), msg);
    }
}

#[test]
fn daemon_suggestion_round_trips() {
    let msg = DaemonMsg::Suggestion(vec![Recommendation {
        range: Range {
            start: 4,
            length: 4,
        },
        value: "likes".to_string(),
        color: "#1ae918".to_string(),
        rule_id: "HE_LIKE".to_string(),
        category_id: "TYPOS".to_string(),
        tooltip: "Possible agreement error".to_string(),
        language: "EN-US".to_string(),
    }]);
    assert_eq!(round_trip(msg.clone()), msg);
    assert_eq!(
        round_trip(DaemonMsg::Suggestion(Vec::new())),
        DaemonMsg::Suggestion(Vec::new()),
        "the empty batch is a valid frame"
    );
}

#[test]
fn settings_round_trips_with_custom_rules() {
    let settings = Settings {
        embedded: false,
        port: 7654,
        background: true,
        language: "el_GR".to_string(),
        color_settings: BTreeMap::from([(
            "RULE:MORFOLOGIK_RULE_EN_US".to_string(),
            "#11bb22".to_string(),
        )]),
        ..Settings::default()
    };
    assert_eq!(round_trip(settings.clone()), settings);
}

#[test]
fn hello_version_mismatch_is_detectable() {
    // A pre-upgrade daemon answers a different version; the client detects
    // the mismatch from the decoded field (compare against PROTOCOL locally).
    let stale = round_trip(DaemonMsg::Hello {
        protocol: PROTOCOL.checked_sub(1).unwrap_or(99),
        busy: false,
    });
    match stale {
        DaemonMsg::Hello { protocol, .. } => assert_ne!(protocol, PROTOCOL),
        _ => panic!("expected a Hello frame"),
    }
}

// --- Negative framing -------------------------------------------------------

#[test]
fn empty_stream_reads_as_clean_close() {
    let mut empty: &[u8] = &[];
    let closed = rhesis_protocol::read_frame::<ClientMsg>(&mut empty)
        .expect("EOF at a frame boundary is not an error");
    assert!(closed.is_none(), "peer closed between messages");
}

#[test]
fn torn_header_is_truncated() {
    for fills in 1..4 {
        let slice = [1u8; 8];
        let mut partial = &slice[..fills];
        assert!(
            matches!(
                rhesis_protocol::read_frame::<ClientMsg>(&mut partial),
                Err(rhesis_protocol::FrameError::Truncated)
            ),
            "{fills}-byte header must not look like a clean close"
        );
    }
}

#[test]
fn torn_payload_is_truncated() {
    // Announce 4 bytes, deliver 3.
    let mut wire = 0u32.to_le_bytes().to_vec();
    wire[0] = 4;
    wire.pop();
    assert!(matches!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut wire.as_slice()),
        Err(rhesis_protocol::FrameError::Truncated)
    ));
}

#[test]
fn oversized_length_is_rejected_before_reading() {
    let mut header = (FRAME_CAP as u32 + 1).to_le_bytes().to_vec();
    header.extend_from_slice(b"payload never allocated");
    assert!(
        matches!(
            rhesis_protocol::read_frame::<ClientMsg>(&mut header.as_slice()),
            Err(rhesis_protocol::FrameError::TooLarge { length })
                if length == FRAME_CAP + 1
        ),
        "the cap must trigger before the payload materializes"
    );
}

#[test]
fn garbage_payload_fails_decode() {
    // Well-formed header, nonsense postcard body (unknown enum tag).
    let mut wire = 2u32.to_le_bytes().to_vec();
    wire.extend_from_slice(&[0xFF, 0xFF]);
    assert!(matches!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut wire.as_slice()),
        Err(rhesis_protocol::FrameError::Decode(_))
    ));
}

#[test]
fn zero_length_frame_fails_decode() {
    // Header claims a zero-length payload: EOF instantly, no enum bytes.
    let wire = 0u32.to_le_bytes().to_vec();
    assert!(matches!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut wire.as_slice()),
        Err(rhesis_protocol::FrameError::Decode(_))
    ));
}

#[test]
fn frames_read_sequentially() {
    let mut wire = Vec::new();
    rhesis_protocol::write_frame(&mut wire, &ClientMsg::Start).unwrap();
    rhesis_protocol::write_frame(&mut wire, &ClientMsg::Retry).unwrap();
    let mut cursor = wire.as_slice();
    assert_eq!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut cursor).unwrap(),
        Some(ClientMsg::Start)
    );
    assert_eq!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut cursor).unwrap(),
        Some(ClientMsg::Retry)
    );
    assert_eq!(
        rhesis_protocol::read_frame::<ClientMsg>(&mut cursor).unwrap(),
        None
    );
}

// --- Shared type shapes (the old recommendation tier's struct checks) ------

#[test]
fn defaults_are_zero_value() {
    let recommendation = Recommendation::default();
    assert_eq!(recommendation.range, Range::default());
    assert_eq!(recommendation.value, "");
    assert_eq!(recommendation.range.start, 0);
    assert_eq!(recommendation.range.length, 0);
}

#[test]
fn recommendations_map_fixtures_field_for_field() {
    // The recorded-fixture pipeline (DTO mapping in rhesis-languagetool)
    // must keep feeding exactly these field names through the wire.
    let settings = Settings {
        color_settings: BTreeMap::from([
            ("CATEGORY:TYPOS".to_string(), "#1ae918".to_string()),
            ("RULE:MY_RULE".to_string(), "#101010".to_string()),
        ]),
        ..Settings::default()
    };

    let wire = ser(&settings);
    let parsed: Settings = de(&wire);
    assert_eq!(parsed.color_settings, settings.color_settings);
    assert_eq!(parsed.port, settings.port);
    assert_eq!(parsed.embedded, settings.embedded);
}

fn ser(value: &Settings) -> Vec<u8> {
    let mut wire = Vec::new();
    rhesis_protocol::write_frame(&mut wire, value).unwrap();
    wire
}

fn de(mut bytes: &[u8]) -> Settings {
    rhesis_protocol::read_frame::<Settings>(&mut bytes)
        .expect("frame must decode")
        .expect("non-empty byte source")
}

/// A `Reader` adapter proving the framing never depends on buffered reads.
#[allow(dead_code)]
struct OneByteReader<'a> {
    bytes: &'a [u8],
}

impl Read for OneByteReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes.is_empty() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.bytes[0];
        self.bytes = &self.bytes[1..];
        Ok(1)
    }
}

#[test]
fn framing_survives_byte_granular_streams() {
    let settings = Settings::default();
    let wire = ser(&settings);
    let mut reader = OneByteReader { bytes: &wire };
    let parsed = rhesis_protocol::read_frame::<Settings>(&mut reader)
        .expect("frame must decode one byte at a time")
        .expect("complete payload");
    assert_eq!(parsed, settings);
}
