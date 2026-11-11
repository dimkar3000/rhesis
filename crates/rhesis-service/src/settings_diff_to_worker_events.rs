use std::collections::BTreeMap;

use rhesis_languagetool::service::LanguageToolWorkerEvent;
use rhesis_settings::settings::Settings;

/// Create a list of the event that convert the the current applied state to the stored state.
/// If the applied is not provided then cretes a list of event that applied the stored state from scratch
pub fn settings_diff_to_worker_events(
    current_settings: Option<&Settings>,
    target_settings: &Settings,
) -> Vec<LanguageToolWorkerEvent> {
    let mut events = Vec::new();

    match current_settings {
        None => {
            if target_settings.embedded {
                events.push(LanguageToolWorkerEvent::RestartLocalServer(
                    target_settings.port,
                ));
            } else if target_settings.external_address.is_empty() {
                events.push(LanguageToolWorkerEvent::Stop);
            } else {
                events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                    target_settings.external_address.clone(),
                ));
            }
        }
        Some(applied) => {
            match (applied.embedded, target_settings.embedded) {
                // Send a restart event to make sure the server is up and running. Used on first start
                (true, true) => {
                    if applied.port != target_settings.port {
                        events.push(LanguageToolWorkerEvent::RestartLocalServer(
                            target_settings.port,
                        ));
                    }
                }
                (false, false) => {
                    if applied.external_address != target_settings.external_address {
                        if target_settings.external_address.is_empty() {
                            // Cleared address: leave no server running.
                            events.push(LanguageToolWorkerEvent::Stop);
                        } else {
                            events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                                target_settings.external_address.clone(),
                            ));
                        }
                    }
                }
                // Mode switches always produce the full server
                // configuration, same translation as the settings page
                // always used.
                (true, false) | (false, true) => {
                    if target_settings.embedded {
                        events.push(LanguageToolWorkerEvent::RestartLocalServer(
                            target_settings.port,
                        ));
                    } else if target_settings.external_address.is_empty() {
                        events.push(LanguageToolWorkerEvent::Stop);
                    } else {
                        events.push(LanguageToolWorkerEvent::SwitchToRemoteServer(
                            target_settings.external_address.clone(),
                        ));
                    }
                }
            }
        }
    }

    // Colors ride along whenever they differ from the worker's current
    // table (a fresh worker still needs its first batch).
    let worker_colors =
        current_settings.map_or_else(BTreeMap::default, |applied| applied.color_settings.clone());
    if worker_colors != target_settings.color_settings {
        events.push(LanguageToolWorkerEvent::UpdateColors(
            target_settings
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
        let events = events_of(settings_diff_to_worker_events(None, &stored));
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
            events_of(settings_diff_to_worker_events(None, &stored)),
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
            events_of(settings_diff_to_worker_events(None, &stored)),
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
            events_of(settings_diff_to_worker_events(None, &stored)),
            vec!["stop", "colors:3 rules"],
            "'remote with no address' leaves no server running"
        );
    }

    #[test]
    fn reconnect_with_equal_settings_sends_nothing() {
        let stored = base();
        assert!(
            settings_diff_to_worker_events(Some(&stored), &stored).is_empty(),
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

        let events = settings_diff_to_worker_events(Some(&applied), &stored);
        let tagged = events_of(events);
        assert_eq!(
            tagged,
            vec!["colors:4 rules"],
            "one color event, nothing else"
        );

        // And unchanged colors produce nothing at all.
        applied.color_settings = stored.color_settings.clone();
        assert!(settings_diff_to_worker_events(Some(&applied), &stored).is_empty());
    }

    #[test]
    fn port_change_restarts_on_the_new_port_only() {
        let applied = base();
        let stored = Settings {
            port: 7890,
            ..base()
        };
        assert_eq!(
            events_of(settings_diff_to_worker_events(Some(&applied), &stored)),
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
            events_of(settings_diff_to_worker_events(Some(&applied), &stored)),
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
            events_of(settings_diff_to_worker_events(Some(&applied), &stored)),
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
            events_of(settings_diff_to_worker_events(Some(&applied), &stored)),
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
            events_of(settings_diff_to_worker_events(Some(&applied), &stored)),
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
        assert!(settings_diff_to_worker_events(Some(&applied), &stored).is_empty());
    }
}
