//! Settings-translation layer: turning snapshots into worker events.
//!
//! The daemon owns every setting (Refactor.md §1), but the LanguageTool
//! worker only understands a narrow event vocabulary. [`worker_events`]
//! bridges the two: given what the worker last received (`applied`) and the
//! snapshot now wanted (`stored`), it produces exactly the events whose
//! effect on the running server differs. That is what makes a client
//! reconnecting inside the 5 s idle grace a no-op for the JVM —
//! [`RestartLocalServer`](LanguageToolWorkerEvent::RestartLocalServer)
//! always terminates the child first, so an equal snapshot must produce no
//! events at all (Refactor.md §2) — while a real change does the minimal
//! thing instead of a full replay.
//!
//! Non-worker state (`language`, tooltips, `background`) never produces
//! events here — the daemon applies those to itself.

use std::collections::BTreeMap;

use rhesis_languagetool::service::LanguageToolWorkerEvent;
use rhesis_protocol::Settings;

/// The worker events that turn `applied` (what the worker last received;
/// `None` for a freshly spawned worker) into `stored` (the snapshot now
/// wanted). Order is deterministic: server-target events first, colors
/// last (colors arrive with a live server and are applied immediately).
pub fn worker_events(
    applied: Option<&Settings>,
    stored: &Settings,
) -> Vec<LanguageToolWorkerEvent> {
    let mut events = Vec::new();

    match applied {
        // A freshly spawned worker is Stopped in local mode: the first
        // handshake is the full replay (§2) — start the server on the
        // stored configuration whatever the port happens to be, since
        // nothing is running yet.
        None => {
            if stored.embedded {
                events.push(LanguageToolWorkerEvent::RestartLocalServer(stored.port));
            } else if stored.external_address.is_empty() {
                events.push(LanguageToolWorkerEvent::Stop);
            } else {
                events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                    stored.external_address.clone(),
                ));
            }
        }
        Some(applied) => {
            // Default colors when the worker got nothing yet; colors are
            // handled below, shared with the replay.
            match (applied.embedded, stored.embedded) {
                // Same mode: only a real drift moves the server. Equal
                // settings → nothing, exactly what protects the
                // reconnect-inside-the-grace client's JVM
                // (RestartLocalServer always terminates the child first).
                (true, true) => {
                    if applied.port != stored.port {
                        events.push(LanguageToolWorkerEvent::RestartLocalServer(stored.port));
                    }
                }
                (false, false) => {
                    if applied.external_address != stored.external_address {
                        if stored.external_address.is_empty() {
                            // Cleared address: leave no server running.
                            events.push(LanguageToolWorkerEvent::Stop);
                        } else {
                            events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                                stored.external_address.clone(),
                            ));
                        }
                    }
                }
                // Mode switches always produce the full server
                // configuration, same translation as the settings page
                // always used.
                (true, false) | (false, true) => {
                    if stored.embedded {
                        events.push(LanguageToolWorkerEvent::RestartLocalServer(stored.port));
                    } else if stored.external_address.is_empty() {
                        events.push(LanguageToolWorkerEvent::Stop);
                    } else {
                        events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                            stored.external_address.clone(),
                        ));
                    }
                }
            }
        }
    }

    // Colors ride along whenever they differ from the worker's current
    // table (a fresh worker still needs its first batch).
    let worker_colors =
        applied.map_or_else(BTreeMap::default, |applied| applied.color_settings.clone());
    if worker_colors != stored.color_settings {
        events.push(LanguageToolWorkerEvent::UpdateColors(
            stored
                .color_settings
                .iter()
                .map(|(rule, color)| (rule.clone(), color.clone()))
                .collect(),
        ));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Settings {
        Settings::default()
    }

    fn events_of(events: Vec<LanguageToolWorkerEvent>) -> Vec<String> {
        // Debug-ish tags keep assertions readable without matching payloads.
        events
            .into_iter()
            .map(|event| match event {
                LanguageToolWorkerEvent::RestartLocalServer(port) => format!("restart:{port}"),
                LanguageToolWorkerEvent::SwitchToRemoteServer(address) => {
                    format!("switch:{address}")
                }
                LanguageToolWorkerEvent::Stop => "stop".to_string(),
                LanguageToolWorkerEvent::UpdateColors(pairs) => {
                    format!("colors:{} rules", pairs.len())
                }
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn first_handshake_replays_the_full_embedded_config() {
        let stored = base();
        let events = events_of(worker_events(None, &stored));
        assert_eq!(
            events,
            vec!["restart:2689", "colors:3 rules"],
            "fresh worker: start on the stored port in local mode, then colors"
        );
    }

    #[test]
    fn first_handshake_without_colors_needs_no_color_event() {
        let stored = Settings {
            color_settings: BTreeMap::default(),
            ..Settings::default()
        };
        assert_eq!(
            events_of(worker_events(None, &stored)),
            vec!["restart:2689"],
            "an empty color table matches the fresh worker's empty table"
        );
    }

    #[test]
    fn first_handshake_remote_switches_to_the_external_server() {
        let stored = Settings {
            embedded: false,
            external_address: "http://office:8080".to_string(),
            ..base()
        };
        assert_eq!(
            events_of(worker_events(None, &stored)),
            vec!["switch:http://office:8080", "colors:3 rules"]
        );
    }

    #[test]
    fn first_handshake_remote_cleared_address_stops() {
        let stored = Settings {
            embedded: false,
            external_address: String::new(),
            ..base()
        };
        assert_eq!(
            events_of(worker_events(None, &stored)),
            vec!["stop", "colors:3 rules"],
            "'remote with no address' leaves no server running"
        );
    }

    #[test]
    fn reconnect_with_equal_settings_sends_nothing() {
        let stored = base();
        assert!(
            worker_events(Some(&stored), &stored).is_empty(),
            "the reconnect-within-grace contract: RestartLocalServer would kill the JVM"
        );
    }

    #[test]
    fn colors_only_change_updates_colors() {
        let mut applied = base();
        let mut stored = base();
        stored
            .color_settings
            .insert("RULE:MY_RULE".to_string(), "#101010".to_string());

        let events = worker_events(Some(&applied), &stored);
        let tagged = events_of(events);
        assert_eq!(
            tagged,
            vec!["colors:4 rules"],
            "one color event, nothing else"
        );

        // And unchanged colors produce nothing at all.
        applied.color_settings = stored.color_settings.clone();
        assert!(worker_events(Some(&applied), &stored).is_empty());
    }

    #[test]
    fn port_change_restarts_on_the_new_port_only() {
        let applied = base();
        let stored = Settings {
            port: 7890,
            ..base()
        };
        assert_eq!(
            events_of(worker_events(Some(&applied), &stored)),
            vec!["restart:7890"]
        );
    }

    #[test]
    fn address_change_while_remote_switches() {
        let applied = Settings {
            embedded: false,
            external_address: "http://old:8080".to_string(),
            ..base()
        };
        let stored = Settings {
            embedded: false,
            external_address: "http://new:8081".to_string(),
            ..base()
        };
        assert_eq!(
            events_of(worker_events(Some(&applied), &stored)),
            vec!["switch:http://new:8081"]
        );
    }

    #[test]
    fn cleared_address_while_remote_stops() {
        let applied = Settings {
            embedded: false,
            external_address: "http://old:8080".to_string(),
            ..base()
        };
        let stored = Settings {
            embedded: false,
            external_address: String::new(),
            ..base()
        };
        assert_eq!(
            events_of(worker_events(Some(&applied), &stored)),
            vec!["stop"]
        );
    }

    #[test]
    fn mode_switch_embedded_to_remote() {
        let applied = base();
        let stored = Settings {
            embedded: false,
            external_address: "http://external:9000".to_string(),
            ..base()
        };
        assert_eq!(
            events_of(worker_events(Some(&applied), &stored)),
            vec!["switch:http://external:9000"]
        );
    }

    #[test]
    fn mode_switch_remote_to_embedded() {
        let applied = Settings {
            embedded: false,
            external_address: "http://external:9000".to_string(),
            ..base()
        };
        let stored = Settings {
            port: 2690,
            ..base()
        };
        assert_eq!(
            events_of(worker_events(Some(&applied), &stored)),
            vec!["restart:2690"]
        );
    }

    #[test]
    fn language_and_toggles_never_touch_the_worker() {
        let applied = base();
        let stored = Settings {
            language: "el_GR".to_string(),
            show_tooltips: !Settings::default().show_tooltips,
            show_debug_tooltips: !Settings::default().show_debug_tooltips,
            background: !Settings::default().background,
            ..base()
        };
        assert!(worker_events(Some(&applied), &stored).is_empty());
    }
}
