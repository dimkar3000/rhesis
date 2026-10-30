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
            id: serverStatusAction
            // Worker status codes, see WorkerStatus::code in languagetool/service.rs:
            // 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed,
            // 4 = Connecting, 5 = FailedToRespond.
            readonly property int statusStopped: 0
            readonly property int statusStarting: 1
            readonly property int statusStarted: 2
            readonly property int statusFailed: 3
            readonly property int statusConnecting: 4
            readonly property int statusFailedToRespond: 5
            readonly property bool canRetry: workerStatus === statusFailed
            readonly property int workerStatus: helper ? helper.server_status : statusStopped
            text: {
                switch (workerStatus) {
                case statusStarted:
                    return qsTr("lt_started");
                case statusStarting:
                    return qsTr("lt_starting");
                case statusConnecting:
                    return qsTr("lt_connecting");
                case statusFailedToRespond:
                    return qsTr("lt_failed_to_respond");
                case statusFailed:
                    return qsTr("lt_failed");
                default:
                    return qsTr("lt_stopped");
                }
            }
            icon.name: {
                switch (workerStatus) {
                case statusStarted:
                    return "emblem-ok";
                case statusStarting:
                case statusConnecting:
                    return "emblem-synchronizing";
                default:
                    return "dialog-error";
                }
            }
            tooltip: (canRetry && helper && helper.server_status_reason !== "") ? helper.server_status_reason : text
            enabled: canRetry
            onTriggered: {
                if (canRetry && helper)
                    helper.retry_server();
            }
            displayComponent: ServerStatusIndicator {
                helper: mainPage.helper
            }
        },
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

    Controls.TextArea {
        id: sourceArea
        // Suites locate the input through this name (see tests/ui), keep it stable.
        objectName: "checkerTextArea"
        anchors.fill: mainPage.contentItem
        padding: 5
        background: null
        wrapMode: Controls.TextArea.Wrap
        placeholderText: qsTr("enter_text_placeholder")
        // Rhesis checks text itself via LanguageTool: keep the desktop
        // style's Sonnet highlighter off, it has no backends in the
        // AppImage (and would only duplicate suggestions with warnings).
        Kirigami.SpellCheck.enabled: false
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
