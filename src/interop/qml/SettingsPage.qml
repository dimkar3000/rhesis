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
    property bool localEmbedded: false
    property bool localTooltipSwitch: false
    property bool localDebugTooltipSwitch: false
    property string localPort: ""
    property bool shouldRestartServer: localEmbedded !== settings.embedded || localPort !== settings.port

    ListModel {
        id: colorRulesModel
    }

    Component.onCompleted: {
        localEmbedded = settings.embedded;
        localPort = settings.port;
        localTooltipSwitch = settings.showTooltips;
        localDebugTooltipSwitch = settings.showDebugTooltips;
        loadColorRules();
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
        console.log("Collected rules: ", JSON.stringify(rules));
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

    property bool localPortValid: isValidLocalPort(portField.text)

    function isFormSubmitable() {
        for (var i = 0; i < colorRulesModel.count; i++) {
            if (colorRulesModel.get(i).ruleText === "") {
                return false;
            }
        }

        // Embeded switch is changed
        if (localEmbedded !== settings.embedded) {
            // switch off
            if (!localEmbedded) {
                return true;
            }
            if (localEmbedded && isValidLocalPort(localPort)) {
                return true;
            }

            return false;
        }

        if (localPort !== settings.port && isValidLocalPort(localPort)) {
            return true;
        }

        if (localTooltipSwitch !== settings.showTooltips) {
            return true;
        }

        if (localDebugTooltipSwitch !== settings.showDebugTooltips) {
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
                    helper.restart(localEmbedded, localPort);
                }

                settings.embedded = localEmbedded;
                if (!localEmbedded) {
                    settings.port = "";
                } else {
                    settings.port = localPort;
                }

                settings.showTooltips = localTooltipSwitch;

                settings.showDebugTooltips = localDebugTooltipSwitch && localTooltipSwitch;

                settings.colorSettings = collectColorRules();
                helper.update_colors(settings.colorSettings);

                applicationWindow().pageStack.layers.pop();
            }
        }
    ]

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
                Kirigami.FormData.label: qsTr("embedded_lt_settings_section")
            }

            Controls.Switch {
                id: embedSwitch
                Kirigami.FormData.label: qsTr("enable_embed_label")
                checked: localEmbedded
                onToggled: localEmbedded = checked
            }

            RowLayout {
                Kirigami.FormData.label: qsTr("port_label")

                Controls.TextField {
                    id: portField
                    Layout.fillWidth: true
                    enabled: localEmbedded
                    placeholderText: "(2689)"
                    text: localPort
                    onTextChanged: localPort = text

                    background: Rectangle {
                        color: Kirigami.Theme.backgroundColor
                        border.color: localPortValid || !localEmbedded ? ((!localPort && portField.focused) ? Kirigami.Theme.activeTextColor : Kirigami.Theme.textColor) : Kirigami.Theme.negativeTextColor
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
                    enabled: localEmbedded && localPort !== settings.defaultPort
                    onClicked: localPort = settings.defaultPort
                    Controls.ToolTip {
                        text: qsTr("reset_port_tooltip")
                        visible: parent.hovered
                    }
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
