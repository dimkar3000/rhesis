import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Dialogs

RowLayout {
    id: root

    spacing: 8

    property string ruleType: "CATEGORY"
    property string ruleText: ""
    property string ruleColor: "#000000"
    property int modelIndex: -1
    readonly property color errorColor: Application.styleHints.colorScheme === Qt.ColorScheme.Dark ? "#ff99a4" : "#c42b1c"

    signal typeChanged(string newType)
    signal textChanged(string newText)
    signal colorChanged(string newColor)
    signal removeRequested

    // The model carries values only; labels use qsTr() directly inside the
    // bindings below so they follow language switches. (retranslate() only
    // re-evaluates bindings that textually contain qsTr(): not
    // Component.onCompleted, not JS helper functions.)
    ListModel {
        id: ruleTypesModel
        Component.onCompleted: {
            ruleTypesModel.append({ value: "CATEGORY" });
            ruleTypesModel.append({ value: "RULE" });
        }
    }

    Controls.ComboBox {
        id: ruleTypeCombo
        Layout.preferredWidth: 140
        model: ruleTypesModel
        textRole: "value"
        displayText: currentValue === "CATEGORY" ? qsTr("rule_type_category") : qsTr("rule_type_rule")
        delegate: Controls.ItemDelegate {
            required property string value
            required property int index
            width: ruleTypeCombo.width
            text: value === "CATEGORY" ? qsTr("rule_type_category") : qsTr("rule_type_rule")
            highlighted: ruleTypeCombo.highlightedIndex === index
        }
        currentIndex: {
            for (let i = 0; i < ruleTypesModel.count; i++) {
                if (ruleTypesModel.get(i).value === root.ruleType)
                    return i;
            }
            return 0;
        }
        onActivated: function (activatedIndex) {
            root.typeChanged(ruleTypesModel.get(activatedIndex).value);
        }
    }

    Controls.TextField {
        id: ruleTextField
        // Named for tests: every rule row carries one.
        objectName: "ruleTextField"
        Layout.fillWidth: true
        Layout.preferredHeight: ruleTypeCombo.height
        placeholderText: qsTr("filter_text_placeholder")
        // An empty, already-visited field is invalid: flag it through the
        // placeholder, the only text visible while it is empty.
        placeholderTextColor: wasFocused && !activeFocus && text === "" ? root.errorColor : palette.placeholderText
        text: root.ruleText
        validator: RegularExpressionValidator {
            regularExpression: /^[A-Za-z0-9]*$/
        }
        inputMethodHints: Qt.ImhNoPredictiveText
        property bool wasFocused: false
        onActiveFocusChanged: {
            if (activeFocus)
                wasFocused = true;
        }
        onTextChanged: {
            let cleaned = text.replace(/\s/g, "").toUpperCase();
            if (text !== cleaned) {
                text = cleaned;
                return;
            }
            root.textChanged(text);
        }
    }

    Rectangle {
        id: colorSwatch
        Layout.preferredWidth: 32
        Layout.preferredHeight: 32
        color: root.ruleColor
        border.color: Qt.rgba(0.5, 0.5, 0.5, 0.8)
        border.width: 1
        radius: 4

        MouseArea {
            id: swatchMouseArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
                _colorDialog.selectedColor = root.ruleColor;
                _colorDialog.open();
            }

            Controls.ToolTip {
                text: qsTr("click_pick_color_tooltip")
                visible: swatchMouseArea.containsMouse
            }
        }
    }

    Controls.Button {
        objectName: "removeRuleButton"
        icon.source: "qrc:/icons/windows/remove.svg"
        Layout.preferredWidth: 32
        Layout.preferredHeight: 32
        onClicked: root.removeRequested()
        Controls.ToolTip {
            text: qsTr("remove_filter_tooltip")
            visible: parent.hovered
        }
    }

    ColorDialog {
        id: _colorDialog
        modality: Qt.WindowModal
        onAccepted: {
            root.colorChanged(selectedColor.toString());
        }
    }
}
