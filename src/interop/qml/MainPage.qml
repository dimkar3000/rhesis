import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis

Kirigami.Page {
    id: mainPage
    padding: 0

    property var settings
    property int wordStart: 0
    property int wordEnd: 0
    property CustomHighlighter highlighter
    property AsyncMessagingHelper helper

    actions: [
        Kirigami.Action {
            icon.name: "configure"
            text: qsTr("settings_action")
            onTriggered: applicationWindow().pageStack.layers.push(applicationWindow().settingsPage)
        }
    ]

    // holds the list of suggestions
    ListModel {
        id: menuModel
    }

    Controls.Menu {
        id: contextMenu

        Repeater {
            id: menu_repeater
            model: menuModel

            Controls.MenuItem {
                text: suggestion
                Controls.ToolTip {
                    text: tooltip
                    visible: parent.hovered && settings.showTooltips
                    delay: 500
                    timeout: 2500
                    x: parent.width + 10
                    y: 0
                }
                onTriggered: {
                    let suggestion = text;
                    highlighter.replaceWord(mainPage.wordStart, mainPage.wordEnd, suggestion);
                }
            }
        }

        function rebuild(suggestions) {
            menuModel.clear();

            for (let i = 0; i < suggestions.length; i++) {
                let suggestion = suggestions[i].value;

                let tooltip = suggestions[i].tooltip;
                if (settings.showDebugTooltips) {
                    tooltip = `[${suggestions[i].language}] ${suggestions[i].tooltip}\n\n${qsTr("more_info_tooltip")} ${qsTr("rule_id_label")} ${suggestions[i].rule_id}\n${qsTr("category_id_label")} ${suggestions[i].category_id}`;
                }
                menuModel.append({
                    suggestion: suggestion,
                    tooltip: tooltip
                });
            }
        }
    }

    Controls.TextArea {
        id: sourceArea
        anchors.fill: mainPage.contentItem
        padding: 5
        background: null
        wrapMode: Controls.TextArea.Wrap
        placeholderText: qsTr("enter_text_placeholder")
        Component.onCompleted: highlighter.setTextDocument(sourceArea.textDocument)
        onTextChanged: t => {
            helper.text_area_changed(sourceArea.text);
        }
    }

    MouseArea {
        anchors.fill: mainPage.contentItem
        acceptedButtons: Qt.RightButton
        onClicked: mouse => {
            let pos = sourceArea.positionAt(mouse.x, mouse.y);
            if (pos < 0)
                return;
            let bounds = highlighter.findRecommendation(pos);
            if (bounds.length === 0) {
                contextMenu.rebuild([]);
                return;
            }

            mainPage.wordStart = bounds[0];
            mainPage.wordEnd = bounds[1];

            sourceArea.cursorPosition = mainPage.wordStart;
            sourceArea.moveCursorSelection(mainPage.wordEnd, TextEdit.SelectCharacters);

            let items = highlighter.getSuggestions(mainPage.wordStart, mainPage.wordEnd);
            if (items.length > 0) {
                contextMenu.rebuild(items);
                contextMenu.popup(mouse.x, mouse.y + 10);
            }
        }
    }
}
