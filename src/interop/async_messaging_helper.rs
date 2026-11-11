//! Client-side mirror of the daemon's state (Refactor.md §5).
//!
//! The helper no longer owns a LanguageTool worker — the daemon does. This
//! module holds the mirror object's pure information side: the qproperty
//! fields, the snapshot parse (`variant_to_settings`, the `[QVariant]`
//! map the settings page hands over), the color-map conversion and the one
//! teardown rule (drop = ask the daemon to quit iff background hosting is
//! off). The invokables that drive the connection live next to the cxx-qt
//! bridge (bridge.rs); the connection module is the only socket-aware code.

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

use crate::connection::Connection;
use crate::constants::CLIENT_STATUS_DISCONNECTED;
use rhesis_settings::constants::DEFAULT_PORT;
use rhesis_settings::settings::Settings;

/// The mirror object behind QML's `messagingHelper`. Field set = the
/// `#[qproperty]` declarations in bridge.rs plus the connection handle.
pub struct AsyncMessagingHelperRust {
    /// What invokedables enqueue onto; `None` while dead (a dead loop is
    /// logged by [`Connection::send`], the UI just stays stale).
    pub connection: Option<Connection>,

    // Mirrors of the daemon-reported status. Between sessions the UI shows
    // the client-legal "Disconnected" (code 6); the daemon replay follows.
    pub server_status: i32,
    pub server_status_reason: QString,

    // Snapshot mirrors; written wherever Qt-thread code applies a snapshot
    // (`apply_settings` in bridge.rs, via the generated setters).
    pub embedded: bool,
    pub port: i32,
    pub default_port: i32,
    pub external_address: QString,
    pub background: bool,
    pub show_tooltips: bool,
    pub show_debug_tooltips: bool,
    pub language: QString,
    pub color_settings: QVariant,
}

impl Default for AsyncMessagingHelperRust {
    /// Boot-defensive defaults: no session yet (Disconnected), the same
    /// snapshot the daemon would store fresh (`Settings::default`), and the
    /// factory port duplicated in `default_port` for the UI hint.
    fn default() -> Self {
        let defaults = Settings::default();
        Self {
            connection: None,
            server_status: CLIENT_STATUS_DISCONNECTED,
            server_status_reason: QString::default(),
            embedded: defaults.embedded,
            port: defaults.port as i32,
            default_port: DEFAULT_PORT as i32,
            external_address: QString::from(defaults.external_address.as_str()),
            background: defaults.background,
            show_tooltips: defaults.show_tooltips,
            show_debug_tooltips: defaults.show_debug_tooltips,
            language: QString::from(defaults.language.as_str()),
            color_settings: color_settings_to_variant(&defaults.color_settings),
        }
    }
}

impl Drop for AsyncMessagingHelperRust {
    /// The UI is the daemon's reference counter when background hosting is
    /// off: whoever quits last kills the daemon. With background on the
    /// daemon keeps serving (idle grace skipped), so no shutdown here.
    fn drop(&mut self) {
        if self.background {
            return;
        }
        if let Some(connection) = &self.connection {
            log::info!("UI is quitting without background hosting; asking the daemon to quit");
            connection.send(crate::connection::ClientMsg::Shutdown);
        }
    }
}

/// Build the QVariant map the QML mirror property holds
/// (`helper.color_settings`, `"CATEGORY:GRAMMAR"` → `"#ecc224"`).
pub(crate) fn color_settings_to_variant(
    colors: &std::collections::BTreeMap<String, String>,
) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in colors {
        map.insert(QString::from(key), QVariant::from(&QString::from(value)));
    }
    QVariant::from(&map)
}

/// Parse the settings snapshot a QML page hands over (`set_settings`).
/// Missing keys fall back to [`Settings::default`], so a partial map can
/// never amputate the daemon's snapshot; the keys are the store's
/// camelCase names (Refactor.md §6), the same names SettingsPage builds.
pub(crate) fn variant_to_settings(map: &QMap<QMapPair_QString_QVariant>) -> Settings {
    let mut settings = Settings::default();

    let get_bool = |key: &str| {
        map.get(&QString::from(key))
            .and_then(|value| value.value::<bool>())
    };
    let get_string = |key: &str| {
        map.get(&QString::from(key))
            .and_then(|value| value.value::<QString>())
            .map(|value| value.to_string())
    };

    if let Some(embedded) = get_bool("embedded") {
        settings.embedded = embedded;
    }

    // The port always crosses as a string (the page's text field), so no
    // QVariant number guessing happens here; junk keeps the fallback.
    if let Some(text) = get_string("port") {
        match text.trim().parse::<u16>() {
            Ok(port) => settings.port = port,
            Err(_) => log::warn!("invalid port {text:?}, keeping {}", settings.port),
        }
    }

    if let Some(address) = get_string("externalAddress") {
        settings.external_address = address.trim().to_string();
    }
    if let Some(background) = get_bool("background") {
        settings.background = background;
    }
    if let Some(tooltips) = get_bool("showTooltips") {
        settings.show_tooltips = tooltips;
    }
    if let Some(debug) = get_bool("showDebugTooltips") {
        settings.show_debug_tooltips = debug;
    }
    if let Some(language) = get_string("language") {
        settings.language = language;
    }

    if let Some(colors) = map
        .get(&QString::from("colorSettings"))
        .and_then(|value| value.value::<QMap<QMapPair_QString_QVariant>>())
    {
        for (key, value) in colors.iter() {
            if let Some(color) = value.value::<QString>() {
                settings
                    .color_settings
                    .insert(key.to_string(), color.to_string());
            }
        }
    }

    settings
}

// ---------------------------------------------------------------------------
// In-module tests: pure data plumbing only (map parse + mirror defaults).
// The Qt-thread glue (generated setters, setBackgroundBehavior) is not
// unit-testable here and lives in the E2E tiers.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn map_from(pairs: &[(&str, QVariant)]) -> QMap<QMapPair_QString_QVariant> {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in pairs {
            map.insert(QString::from(*key), value.clone());
        }
        map
    }

    #[test]
    fn defaults_match_the_protocol_snapshot() {
        let helper = AsyncMessagingHelperRust::default();
        let defaults = Settings::default();
        assert_eq!(helper.server_status, CLIENT_STATUS_DISCONNECTED);
        assert_eq!(helper.server_status_reason.to_string(), "");
        assert_eq!(helper.embedded, defaults.embedded);
        assert_eq!(helper.port, defaults.port as i32);
        assert_eq!(helper.default_port, DEFAULT_PORT as i32);
        assert_eq!(helper.external_address.to_string(), "");
        assert!(!helper.background);
        assert!(helper.show_tooltips);
        assert!(helper.show_debug_tooltips);
        assert_eq!(helper.language.to_string(), "");
        assert!(helper.connection.is_none());
    }

    #[test]
    fn a_missing_map_falls_back_to_full_defaults() {
        let empty = map_from(&[]);
        let settings = variant_to_settings(&empty);
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn every_store_key_is_parsed() {
        let colors = {
            let mut map = QMap::<QMapPair_QString_QVariant>::default();
            map.insert(
                QString::from("CATEGORY:TYPOS"),
                QVariant::from(&QString::from("#1ae918")),
            );
            map.insert(
                QString::from("RULE:MY_RULE"),
                QVariant::from(&QString::from("#abcdef")),
            );
            QVariant::from(&map)
        };
        let snapshot = map_from(&[
            ("embedded", QVariant::from(&false)),
            ("port", QVariant::from(&QString::from("2790"))),
            (
                "externalAddress",
                QVariant::from(&QString::from(" http://127.0.0.1:8081/v2 ")),
            ),
            ("background", QVariant::from(&true)),
            ("showTooltips", QVariant::from(&false)),
            ("showDebugTooltips", QVariant::from(&false)),
            ("language", QVariant::from(&QString::from("el_GR"))),
            ("colorSettings", colors),
        ]);

        let settings = variant_to_settings(&snapshot);
        assert!(!settings.embedded);
        assert_eq!(settings.port, 2790);
        assert_eq!(settings.external_address, "http://127.0.0.1:8081/v2");
        assert!(settings.background);
        assert!(!settings.show_tooltips);
        assert!(!settings.show_debug_tooltips);
        assert_eq!(settings.language, "el_GR");
        assert_eq!(
            settings
                .color_settings
                .get("RULE:MY_RULE")
                .map(String::as_str),
            Some("#abcdef")
        );
        // Untouched entry from the defaults base survives the merge.
        assert_eq!(
            settings
                .color_settings
                .get("CATEGORY:GRAMMAR")
                .map(String::as_str),
            Some("#ecc224")
        );
    }

    #[test]
    fn an_integer_port_in_a_string_still_works_and_junk_is_kept_out() {
        let text_port = map_from(&[("port", QVariant::from(&QString::from("3012")))]);
        assert_eq!(variant_to_settings(&text_port).port, 3012);

        let junk = map_from(&[("port", QVariant::from(&QString::from("not-a-port")))]);
        assert_eq!(variant_to_settings(&junk).port, Settings::default().port);
    }
}
