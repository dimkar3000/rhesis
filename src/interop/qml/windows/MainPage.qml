import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import io.github.dimkar3000.rhesis

Controls.Page {
    id: mainPage
    padding: 0

    property var settings
    property int wordStart: 0
    property int wordEnd: 0
    property CustomHighlighter highlighter
    property AsyncMessagingHelper helper

    signal settingsRequested

    header: Controls.ToolBar {
        leftPadding: 16
        rightPadding: 8

        // A real contentItem: a ToolBar is a Pane, which sizes itself from
        // its children, so a layout anchored to the pane would feed its
        // own height back and balloon.
        contentItem: RowLayout {
            spacing: 8

            Controls.Label {
                Layout.fillWidth: true
                // "app_title" is only translated in Root's context.
                text: Window.window ? Window.window.title : ""
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }

            ServerStatusIndicator {
                helper: mainPage.helper
            }

            Controls.ToolButton {
                icon.source: "qrc:/icons/windows/settings.svg"
                text: qsTr("settings_action")
                display: Controls.AbstractButton.IconOnly
                Controls.ToolTip.text: text
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.delay: 500
                onClicked: mainPage.settingsRequested()
            }
        }
    }

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
                    // Imperative: refreshes on every rebuild (each keystroke),
                    // so a language switch applies here without extra handling.
                    tooltip = `[${suggestions[i].language}] ${suggestions[i].tooltip}\n\n${qsTr("more_info_tooltip")} ${qsTr("rule_id_label")} ${suggestions[i].rule_id}\n${qsTr("category_id_label")} ${suggestions[i].category_id}`;
                }
                menuModel.append({
                    suggestion: suggestion,
                    tooltip: tooltip
                });
            }
        }
    }

    // Unlike the Kirigami page, long text must stay reachable: the text
    // area scrolls inside a ScrollView.
    Controls.ScrollView {
        anchors.fill: parent

        Controls.TextArea {
            id: sourceArea
            // Suites locate the input through this name (see tests/ui), keep it stable.
            objectName: "checkerTextArea"
            padding: 16
            // The Fluent style centers text vertically (single-line
            // heritage); inside a full-height ScrollView that starts the
            // text, caret and placeholder mid-window.
            verticalAlignment: TextEdit.AlignTop
            background: null
            wrapMode: Controls.TextArea.Wrap
            placeholderText: qsTr("enter_text_placeholder")
            Component.onCompleted: highlighter.setTextDocument(sourceArea.textDocument)
            onTextChanged: t => {
                helper.text_area_changed(sourceArea.text);
            }
        }
    }

    MouseArea {
        id: rightClickArea
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: mouse => {
            // The text area scrolls: translate into its own coordinates.
            let local = sourceArea.mapFromItem(rightClickArea, mouse.x, mouse.y);
            let pos = sourceArea.positionAt(local.x, local.y);
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
                contextMenu.popup(rightClickArea, mouse.x, mouse.y + 10);
            }
        }
    }
}
