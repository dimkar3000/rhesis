import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import io.github.dimkar3000.rhesis

Controls.Page {
    id: secondPage

    title: qsTr("settings_title")

    property var settings
    property AsyncMessagingHelper helper
    property LanguageManager langManager
    // The root window (Root.qml): Apply may need to bring it back, and the
    // hidden-launch state lives there.
    property var appWindow
    property bool localEmbedded: false
    property bool localBackgroundSwitch: false
    property bool localAutostartSwitch: false
    // Read from the autostart entry itself (not the persisted settings): the
    // source of truth can change outside the app, so the baseline is
    // re-established on every page open.
    property bool savedAutostart: helper.is_autostart()
    // Seeded in Component.onCompleted. Guards the draft switches:
    // programmatic checked changes still emit toggled(), so the seeding
    // itself must not look like a user edit and re-write the state just read.
    property bool pageInitDone: false
    property bool localTooltipSwitch: false
    property bool localDebugTooltipSwitch: false
    property string localPort: ""
    property string localAddress: ""
    property string localLanguage: ""
    // Bumped whenever a color rule is edited: ListModel.get() reads are not
    // tracked by bindings, so isFormSubmitable() depends on this instead.
    property int rulesRevision: 0
    readonly property color errorColor: Application.styleHints.colorScheme === Qt.ColorScheme.Dark ? "#ff99a4" : "#c42b1c"
    // The configuration of the selected server type, old vs. new.
    property bool serverConfigChanged: {
        var saved = localEmbedded ? settings.port : settings.externalAddress;
        var current = localEmbedded ? localPort : localAddress;
        return current !== saved;
    }
    property bool shouldRestartServer: localEmbedded !== settings.embedded || serverConfigChanged

    signal closeRequested

    ListModel {
        id: colorRulesModel
    }

    Connections {
        target: colorRulesModel
        function onDataChanged() {
            secondPage.rulesRevision++;
        }
    }

    Component.onCompleted: {
        localEmbedded = settings.embedded;
        localPort = settings.port;
        localAddress = settings.externalAddress;
        localBackgroundSwitch = settings.background;
        localTooltipSwitch = settings.showTooltips;
        localDebugTooltipSwitch = settings.showDebugTooltips;
        localLanguage = settings.language;
        // The autostart switch must show the real state on open: its draft
        // property starts false, so without this the toggle reads "not set"
        // even when an autostart entry exists.
        localAutostartSwitch = savedAutostart;
        // Autostart implies background hosting: while the autostart state is
        // on, the background switch is forced on and locked (the pairing is
        // committed by Apply). This also heals the legacy mismatch of an old
        // build having written autostart without background.
        if (localAutostartSwitch) {
            localBackgroundSwitch = true;
        }
        loadColorRules();
        // From here on, switch changes are user edits that join the draft and
        // wait for the Apply button (the switches' onToggled handlers only
        // write drafts: see isFormSubmitable and apply()).
        pageInitDone = true;
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
        rulesRevision; // dependency, see above
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

        // The background and autostart switches join the draft contract like
        // everything else on this page: their draft (and, for autostart, the
        // state read from the entry on open) differs while a change waits
        // for the Apply button to commit it.
        if (localBackgroundSwitch !== settings.background
                || localAutostartSwitch !== savedAutostart) {
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

    function apply() {
        if (shouldRestartServer) {
            helper.restart(localEmbedded, localEmbedded ? localPort : localAddress);
        }

        settings.embedded = localEmbedded;
        settings.port = localPort;
        settings.externalAddress = localAddress;

        // Background hosting follows the draft. Hidden-in-tray sessions must
        // never be stranded: when the tray icon is about to disappear, bring
        // the window back so the app stays visible and closable. A hidden
        // launch keeps its tray icon regardless.
        settings.background = localBackgroundSwitch;
        helper.set_background(settings.background);
        if (!(settings.background && settings.embedded) && !appWindow.hiddenLaunch) {
            appWindow.visible = true;
        }

        // The autostart draft only reaches the registry here; on failure the
        // draft reverts so the switch no longer claims a change that never
        // happened. The saved-state check makes the commit a no-op when the
        // toggle round-trips back. The checked write mirrors the revert: the
        // user's click may have broken the checked binding.
        if (localAutostartSwitch !== savedAutostart) {
            if (helper.set_autostart(localAutostartSwitch)) {
                savedAutostart = localAutostartSwitch;
            } else {
                console.warn(`failed to apply autostart state, keeping "${savedAutostart}"`);
                localAutostartSwitch = savedAutostart;
                autostartSwitch.checked = savedAutostart;
            }
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

        closeRequested();
    }

    header: Controls.ToolBar {
        leftPadding: 8
        rightPadding: 8

        // See MainPage: use contentItem, not a layout anchored to the pane.
        contentItem: RowLayout {
            spacing: 8

            Controls.ToolButton {
                icon.source: "qrc:/icons/windows/back.svg"
                onClicked: secondPage.closeRequested()
            }

            Controls.Label {
                Layout.fillWidth: true
                text: secondPage.title
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }

            Controls.Button {
                icon.source: "qrc:/icons/windows/apply.svg"
                text: qsTr("apply_action")
                highlighted: true
                enabled: isFormSubmitable()
                onClicked: secondPage.apply()
            }
        }
    }

    // Section title used by the three groups below.
    component SectionHeader: ColumnLayout {
        property alias text: sectionLabel.text
        Layout.fillWidth: true
        Layout.topMargin: 8
        spacing: 4

        Controls.Label {
            id: sectionLabel
            font.pixelSize: 16
            font.weight: Font.DemiBold
        }
        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: secondPage.palette.mid
        }
    }

    Controls.ScrollView {
        id: scroll
        objectName: "settingsFlickable"
        anchors.fill: parent
        contentWidth: availableWidth
        clip: true

        Item {
            width: scroll.availableWidth
            implicitHeight: content.implicitHeight + 32

            ColumnLayout {
                id: content
                y: 16
                width: Math.min(parent.width - 48, 640)
                x: (parent.width - width) / 2
                spacing: 12

                SectionHeader {
                    text: qsTr("language_section")
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("language_label")
                    }

                    Controls.ComboBox {
                        id: languageCombo
                        Layout.fillWidth: true
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
                }

                SectionHeader {
                    text: qsTr("lt_server_section")
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 10
                        text: qsTr("server_type_label")
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
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
                }

                RowLayout {
                    // Configuration for the embedded server: it runs inside
                    // Rhesis, listening on this port.
                    Layout.fillWidth: true
                    visible: localEmbedded
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("port_label")
                    }

                    Controls.TextField {
                        id: portField
                        Layout.fillWidth: true
                        placeholderText: "(2689)"
                        // Invalid (or required and left empty) shows through the
                        // text and placeholder color.
                        color: !localPortValid && localPort !== "" ? secondPage.errorColor : palette.text
                        placeholderTextColor: !localPortValid && !activeFocus ? secondPage.errorColor : palette.placeholderText
                        text: localPort
                        onTextChanged: localPort = text

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
                        icon.source: "qrc:/icons/windows/reset.svg"
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
                    Layout.fillWidth: true
                    visible: !localEmbedded
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("address_label")
                    }

                    Controls.TextField {
                        id: addressField
                        Layout.fillWidth: true
                        inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
                        placeholderText: qsTr("address_placeholder")
                        color: !localAddressValid && localAddress !== "" ? secondPage.errorColor : palette.text
                        placeholderTextColor: !localAddressValid && !activeFocus ? secondPage.errorColor : palette.placeholderText
                        text: localAddress
                        onTextChanged: localAddress = text

                        Controls.ToolTip {
                            text: qsTr("address_tooltip")
                            visible: parent.hovered
                        }
                    }
                }

                SectionHeader {
                    text: qsTr("background_section")
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("autostart_switch_label")
                    }

                    Controls.Switch {
                        id: autostartSwitch
                        // Named for the page-level suites, like the Linux page.
                        objectName: "autostartSwitch"
                        checked: localAutostartSwitch
                        onToggled: {
                            if (!pageInitDone) {
                                return;
                            }
                            // Draft only: the registry gets written by Apply.
                            // While the autostart draft is enabled, background
                            // hosting is forced on and its switch stays locked:
                            // a hidden-at-login app must keep hosting/holding
                            // the tray state. Unchecking releases the lock,
                            // keeping the forced value for the user to adjust.
                            localAutostartSwitch = checked;
                            if (checked) {
                                localBackgroundSwitch = true;
                            }
                        }

                        Controls.ToolTip {
                            text: qsTr("autostart_tooltip")
                            visible: parent.hovered
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("background_switch_label")
                        enabled: backgroundSwitch.enabled
                    }

                    Controls.Switch {
                        id: backgroundSwitch
                        objectName: "backgroundSwitch"
                        // Only the embedded server can be hosted in the
                        // background; external servers are shared machines of
                        // their own. While autostart is on, the pairing locks
                        // the switch regardless.
                        enabled: localEmbedded && !localAutostartSwitch
                        checked: localBackgroundSwitch
                        onToggled: {
                            if (!pageInitDone) {
                                return;
                            }
                            // Draft only: settings.background and the host
                            // behavior are committed by Apply.
                            localBackgroundSwitch = checked;
                        }

                        Controls.ToolTip {
                            text: qsTr("background_tooltip")
                            visible: parent.hovered
                        }
                    }
                }

                SectionHeader {
                    text: qsTr("tooltip_settings_section")
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("enable_tooltip_label")
                    }

                    Controls.Switch {
                        id: tooltipSwitch
                        objectName: "tooltipSwitch"
                        checked: localTooltipSwitch
                        onToggled: localTooltipSwitch = checked
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16

                    Controls.Label {
                        Layout.preferredWidth: 180
                        text: qsTr("more_info_label")
                        enabled: tooltipSwitch.checked
                    }

                    Controls.Switch {
                        id: debugTooltips
                        enabled: tooltipSwitch.checked
                        checked: localDebugTooltipSwitch
                        onToggled: localDebugTooltipSwitch = checked
                    }
                }

                SectionHeader {
                    text: qsTr("suggestion_colors_heading")
                }

                // Listed in place: the page already scrolls, so no nested
                // scroll area is needed for the rules.
                Repeater {
                    id: rulesRepeater
                    model: colorRulesModel

                    RuleListItem {
                        Layout.fillWidth: true
                        ruleType: model.ruleType
                        ruleText: model.ruleText
                        ruleColor: model.ruleColor
                        modelIndex: index

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

                Controls.Button {
                    objectName: "addRuleButton"
                    icon.source: "qrc:/icons/windows/add.svg"
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
}
