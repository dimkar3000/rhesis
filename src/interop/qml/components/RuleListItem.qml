import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

RowLayout {
    id: root

    spacing: Kirigami.Units.smallSpacing

    property string ruleType: "CATEGORY"
    property string ruleText: ""
    property string ruleColor: "#000000"
    property int modelIndex: -1

    signal typeChanged(string newType)
    signal textChanged(string newText)
    signal colorChanged(string newColor)
    signal removeRequested

    ListModel {
        id: ruleTypesModel
        Component.onCompleted: {
            ruleTypesModel.append({ value: "CATEGORY", text: qsTr("rule_type_category") });
            ruleTypesModel.append({ value: "RULE", text: qsTr("rule_type_rule") });
        }
    }

    Controls.ComboBox {
        id: ruleTypeCombo
        Layout.preferredWidth: 120
        model: ruleTypesModel
        textRole: "text"
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
        Layout.fillWidth: true
        Layout.preferredHeight: ruleTypeCombo.height
        placeholderText: qsTr("filter_text_placeholder")
        text: root.ruleText
        color: ruleTextField.wasFocused && ruleTextField.text === "" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
        validator: RegularExpressionValidator {
            regularExpression: /^[A-Za-z0-9]*$/
        }
        inputMethodHints: Qt.ImhNoPredictiveText
        property bool wasFocused: false
        onActiveFocusChanged: {
            if (activeFocus)
                wasFocused = true;
        }
        background: Rectangle {
            implicitWidth: 100
            implicitHeight: 40
            color: Kirigami.Theme.backgroundColor
            border.color: ruleTextField.wasFocused && ruleTextField.text === "" ? Kirigami.Theme.negativeTextColor : ruleTextField.activeFocus ? Kirigami.Theme.activeTextColor : Kirigami.Theme.textColor
            border.width: 1
            radius: 2
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
        border.color: Kirigami.Theme.textColor
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
        icon.name: "list-remove"
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
