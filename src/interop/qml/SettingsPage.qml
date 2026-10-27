import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis

Kirigami.Page {
    id: secondPage

    title: qsTr("settings_title")

    property var settings
    property AsyncMessagingHelper helper
    property LanguageManager langManager
    property bool localEmbedded: false
    property bool localBackgroundSwitch: false
    property bool localAutostartSwitch: false
    // Read from the autostart files themselves (not the persisted settings):
    // the source of truth can change outside the app, so the baseline is
    // re-established on every page open.
    property bool savedAutostart: helper.is_autostart()
    property bool localTooltipSwitch: false
    property bool localDebugTooltipSwitch: false
    property string localPort: ""
    property string localAddress: ""
    property string localLanguage: ""
    // The configuration of the selected server type, old vs. new.
    property bool serverConfigChanged: {
        var saved = localEmbedded ? settings.port : settings.externalAddress;
        var current = localEmbedded ? localPort : localAddress;
        return current !== saved;
    }
    property bool shouldRestartServer: localEmbedded !== settings.embedded || serverConfigChanged

    ListModel {
        id: colorRulesModel
    }

    Component.onCompleted: {
        localEmbedded = settings.embedded;
        localPort = settings.port;
        localAddress = settings.externalAddress;
        localBackgroundSwitch = settings.background;
        localTooltipSwitch = settings.showTooltips;
        localDebugTooltipSwitch = settings.showDebugTooltips;
        localLanguage = settings.language;
        loadColorRules();
    }

    // Display name in the language itself ("Ελληνικά (Ελλάδα)"). Stable
    // across UI languages, so it is safe to call from a binding. The
    // "system default" entry uses qsTr() directly in the model instead.
    function localeLabel(code) {
        var loc = Qt.locale(code);
        var name = loc.nativeLanguageName;
        if (name === "") {
            return code;
        }
        var territory = loc.nativeTerritoryName;
        return territory === "" ? name : `${name} (${territory})`;
    }

    function loadColorRules() {
        colorRulesModel.clear();
        var map = settings.colorSettings;
        var keys = Object.keys(map);
        if (keys && keys.length) {
            for (var i = 0; i < keys.length; i++) {
                let key = keys[i];
                if (key.startsWith("CATEGORY:")) {
                    colorRulesModel.append({
                        ruleType: "CATEGORY",
                        ruleText: key.slice("CATEGORY:".length),
                        ruleColor: map[key]
                    });
                } else if (key.startsWith("RULE:")) {
                    colorRulesModel.append({
                        ruleType: "RULE",
                        ruleText: key.slice("RULE:".length),
                        ruleColor: map[key]
                    });
                } else {
                    console.log(`found weird rule: key: ${key} value: ${map[key]}`);
                }
            }
        } else {
            // TODO: something?
        }
    }

    function collectColorRules() {
        var rules = {};
        for (var i = 0; i < colorRulesModel.count; i++) {
            var item = colorRulesModel.get(i);
            rules[`${item.ruleType}:${item.ruleText}`] = item.ruleColor;
        }
        return rules;
    }

    function isValidLocalPort(port) {
        try {
            let url = new URL(`http://localhost:${port}`);

            // For local server we want a valid port and for it not to be empty
            return port === url.port && port !== "";
        } catch (err) {
            return false;
        }
    }

    // Accepts "http://host:port", "host:port" (http is assumed) and
    // scheme-less reverse-proxy URLs like "https://host/path".
    function isValidExternalAddress(address) {
        try {
            let trimmed = address.trim();
            if (trimmed === "") {
                return false;
            }
            let url = new URL(trimmed.includes("://") ? trimmed : `http://${trimmed}`);

            // The scheme-less default hides the port, so only a hostname is
            // mandatory.
            return url.hostname !== "";
        } catch (err) {
            return false;
        }
    }

    property bool localPortValid: isValidLocalPort(portField.text)
    property bool localAddressValid: isValidExternalAddress(addressField.text)

    function isFormSubmitable() {
        for (var i = 0; i < colorRulesModel.count; i++) {
            if (colorRulesModel.get(i).ruleText === "") {
                return false;
            }
        }

        // Server type is changed
        if (localEmbedded !== settings.embedded) {
            if (localEmbedded && isValidLocalPort(localPort)) {
                return true;
            }
            if (!localEmbedded && isValidExternalAddress(localAddress)) {
                return true;
            }

            return false;
        }

        if (localEmbedded && localPort !== settings.port && isValidLocalPort(localPort)) {
            return true;
        }

        if (!localEmbedded && localAddress !== settings.externalAddress
                && isValidExternalAddress(localAddress)) {
            return true;
        }

        if (localBackgroundSwitch !== settings.background) {
            return true;
        }

        if (localAutostartSwitch !== savedAutostart) {
            return true;
        }

        if (localTooltipSwitch !== settings.showTooltips) {
            return true;
        }

        if (localDebugTooltipSwitch !== settings.showDebugTooltips) {
            return true;
        }

        if (localLanguage !== settings.language) {
            return true;
        }

        if (colorRulesChanged()) {
            return true;
        }

        return false;
    }

    function colorRulesChanged() {
        let current = collectColorRules();
        let saved = settings.colorSettings;

        let currentKeys = Object.keys(current);
        let savedKeys = saved ? Object.keys(saved).filter(k => k.startsWith("CATEGORY:") || k.startsWith("RULE:")) : [];

        if (currentKeys.length !== savedKeys.length) {
            return true;
        }

        for (let i = 0; i < currentKeys.length; i++) {
            if (saved[currentKeys[i]] !== current[currentKeys[i]]) {
                return true;
            }
        }

        return false;
    }

    actions: [
        Kirigami.Action {
            icon.name: "dialog-ok-apply"
            text: qsTr("apply_action")
            enabled: isFormSubmitable()
            onTriggered: {
                if (shouldRestartServer) {
                    helper.restart(localEmbedded, localEmbedded ? localPort : localAddress);
                }

                settings.embedded = localEmbedded;
                settings.port = localPort;
                settings.externalAddress = localAddress;

                // Persisted QSettings writes may lag behind (QSettings syncs
                // lazily), so the C++ side receives the new state explicitly
                // instead of re-reading the store and racing it.
                settings.background = localBackgroundSwitch;
                helper.set_background(localBackgroundSwitch);

                // The autostart toggle touches real files, so it applies
                // immediately; on failure the UI state reverts so the next
                // Apply no longer claims a change that never happened.
                if (localAutostartSwitch !== savedAutostart) {
                    if (helper.set_autostart(localAutostartSwitch)) {
                        savedAutostart = localAutostartSwitch;
                    } else {
                        console.warn(`failed to apply autostart state, keeping "${savedAutostart}"`);
                        localAutostartSwitch = savedAutostart;
                    }
                }

                // Hidden-in-tray sessions must never be stranded: when the
                // tray icon is about to disappear (background hosting off,
                // or an external server now selected), bring the window
                // back so the user can still see and close the app.
                if (!(settings.background && settings.embedded)) {
                    applicationWindow().visible = true;
                }

                settings.showTooltips = localTooltipSwitch;

                settings.showDebugTooltips = localDebugTooltipSwitch && localTooltipSwitch;

                if (localLanguage !== settings.language) {
                    if (langManager.apply_language(localLanguage)) {
                        settings.language = localLanguage;
                    } else {
                        console.warn(`failed to switch language to "${localLanguage}", keeping "${settings.language}"`);
                        localLanguage = settings.language;
                    }
                }

                settings.colorSettings = collectColorRules();
                helper.update_colors(settings.colorSettings);

                applicationWindow().pageStack.layers.pop();
            }
        }
    ]

    // The settings can exceed the window height (720px cap): a Flickable
    // keeps every option reachable. The children below keep their anchors,
    // which resolve in content coordinates; contentWidth matches the
    // viewport width so only vertical scrolling happens.
    Flickable {
        id: pageFlickable
        anchors.fill: parent
        contentWidth: width
        contentHeight: colorsCol.y + colorsCol.height + Kirigami.Units.smallSpacing
        clip: true
        Controls.ScrollBar.vertical: Controls.ScrollBar {
            policy: Controls.ScrollBar.AsNeeded
        }

    Item {
        id: formBox
        width: Math.max(formLayout.implicitWidth + Kirigami.Units.smallSpacing * 16, Kirigami.Units.gridUnit * 20)
        height: formLayout.height + Kirigami.Units.smallSpacing * 2
        anchors.horizontalCenter: parent.horizontalCenter
        y: Kirigami.Units.smallSpacing

        Kirigami.FormLayout {
            id: formLayout
            anchors.fill: parent
            anchors.margins: Kirigami.Units.smallSpacing

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: qsTr("language_section")
            }

            Controls.ComboBox {
                id: languageCombo
                Kirigami.FormData.label: qsTr("language_label")
                textRole: "label"
                valueRole: "code"
                model: [
                    { code: "", label: qsTr("system_default_label") },
                    { code: "en_US", label: localeLabel("en_US") },
                    { code: "el_GR", label: localeLabel("el_GR") }
                ]
                currentIndex: {
                    const i = indexOfValue(localLanguage);
                    return i >= 0 ? i : 0;
                }
                onActivated: localLanguage = currentValue
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: qsTr("lt_server_section")
            }

            ColumnLayout {
                Kirigami.FormData.label: qsTr("server_type_label")
                spacing: 0

                Controls.RadioButton {
                    text: qsTr("embedded_server_label")
                    checked: localEmbedded
                    onToggled: if (checked) localEmbedded = true
                }

                Controls.RadioButton {
                    text: qsTr("external_server_label")
                    checked: !localEmbedded
                    onToggled: if (checked) localEmbedded = false
                }
            }

            RowLayout {
                // Configuration for the embedded server: it runs inside
                // Rhesis, listening on this port.
                visible: localEmbedded
                Kirigami.FormData.label: qsTr("port_label")

                Controls.TextField {
                    id: portField
                    Layout.fillWidth: true
                    placeholderText: "(2689)"
                    text: localPort
                    onTextChanged: localPort = text

                    background: Rectangle {
                        color: Kirigami.Theme.backgroundColor
                        border.color: localPortValid ? ((!localPort && portField.focused) ? Kirigami.Theme.activeTextColor : Kirigami.Theme.textColor) : Kirigami.Theme.negativeTextColor
                        border.width: 1
                        radius: 2
                    }

                    validator: IntValidator {
                        bottom: 1
                        top: 65534
                    }

                    Controls.ToolTip {
                        text: qsTr("port_tooltip")
                        visible: parent.hovered
                    }
                }

                Controls.Button {
                    id: resetBtn
                    icon.name: "document-revert"
                    enabled: localPort !== settings.defaultPort
                    onClicked: localPort = settings.defaultPort
                    Controls.ToolTip {
                        text: qsTr("reset_port_tooltip")
                        visible: parent.hovered
                    }
                }
            }

            RowLayout {
                // Configuration for the external server: Rhesis only talks
                // to it over HTTP, nothing is spawned locally.
                visible: !localEmbedded
                Kirigami.FormData.label: qsTr("address_label")

                Controls.TextField {
                    id: addressField
                    Layout.fillWidth: true
                    inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                    placeholderText: qsTr("address_placeholder")
                    text: localAddress
                    onTextChanged: localAddress = text

                    background: Rectangle {
                        color: Kirigami.Theme.backgroundColor
                        border.color: localAddressValid ? ((!localAddress && addressField.focused) ? Kirigami.Theme.activeTextColor : Kirigami.Theme.textColor) : Kirigami.Theme.negativeTextColor
                        border.width: 1
                        radius: 2
                    }

                    Controls.ToolTip {
                        text: qsTr("address_tooltip")
                        visible: parent.hovered
                    }
                }
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: qsTr("background_section")
            }

            Controls.Switch {
                id: backgroundSwitch
                Kirigami.FormData.label: qsTr("background_switch_label")
                // Only the embedded server can be hosted in the background;
                // external servers are shared machines of their own.
                enabled: localEmbedded
                checked: localBackgroundSwitch
                onToggled: localBackgroundSwitch = checked

                Controls.ToolTip {
                    text: qsTr("background_tooltip")
                    visible: parent.hovered
                }
            }

            Controls.Switch {
                id: autostartSwitch
                Kirigami.FormData.label: qsTr("autostart_switch_label")
                checked: localAutostartSwitch
                onToggled: localAutostartSwitch = checked

                Controls.ToolTip {
                    text: qsTr("autostart_tooltip")
                    visible: parent.hovered
                }
            }

            Kirigami.Separator {
                Kirigami.FormData.isSection: true
                Kirigami.FormData.label: qsTr("tooltip_settings_section")
            }

            Controls.Switch {
                id: tooltipSwitch
                Kirigami.FormData.label: qsTr("enable_tooltip_label")
                checked: localTooltipSwitch
                onToggled: localTooltipSwitch = checked
            }

            Controls.Switch {
                id: debugTooltips
                Kirigami.FormData.label: qsTr("more_info_label")
                enabled: tooltipSwitch.checked
                checked: localDebugTooltipSwitch
                onToggled: localDebugTooltipSwitch = checked
            }
        }
    }

    ColumnLayout {
        id: colorsCol
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: formBox.bottom
        anchors.topMargin: Kirigami.Units.largeSpacing
        width: formBox.width

        Kirigami.Heading {
            Layout.fillWidth: true
            text: qsTr("suggestion_colors_heading")
            level: 3
            type: Kirigami.Heading.Type.Primary
            horizontalAlignment: Text.AlignHCenter
        }

        Kirigami.Separator {
            Layout.fillWidth: true
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 250
            Layout.maximumHeight: 300
            color: "transparent"
            border.color: Kirigami.Theme.textColor
            border.width: 1
            radius: 4

            Controls.ScrollView {
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                clip: true
                contentWidth: availableWidth

                ColumnLayout {
                    width: parent.width
                    spacing: Kirigami.Units.smallSpacing

                    Repeater {
                        id: rulesRepeater
                        model: colorRulesModel

                        RuleListItem {
                            ruleType: model.ruleType
                            ruleText: model.ruleText
                            ruleColor: model.ruleColor
                            modelIndex: index
                            width: parent.width

                            onTypeChanged: function (newType) {
                                colorRulesModel.setProperty(index, "ruleType", newType);
                            }
                            onTextChanged: function (newText) {
                                colorRulesModel.setProperty(index, "ruleText", newText);
                            }
                            onColorChanged: function (newColor) {
                                colorRulesModel.setProperty(index, "ruleColor", newColor);
                            }
                            onRemoveRequested: {
                                colorRulesModel.remove(index);
                            }
                        }
                    }
                }
            }
        }

        Controls.Button {
            icon.name: "list-add"
            text: qsTr("add_filter_button")
            onClicked: {
                colorRulesModel.append({
                    ruleType: "CATEGORY",
                    ruleText: "",
                    ruleColor: "#000000"
                });
            }
        }
    }
    }
}
