import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis

Kirigami.Page {
    id: mainPage
    padding: 0

    property int wordStart: 0
    property int wordEnd: 0
    property CustomHighlighter highlighter
    property AsyncMessagingHelper helper

    actions: [
        Kirigami.Action {
            icon.name: "configure"
            text: "Settings"
            onTriggered: applicationWindow().pageStack.layers.push(applicationWindow().settingsPage)
        }
    ]

    Controls.Menu {
        id: contextMenu
        
        function rebuild(suggestions) {
            while (contextMenu.count > 0) {
                let item = contextMenu.itemAt(0)
                contextMenu.removeItem(item)
                item.destroy()
            }

            for (let i = 0; i < suggestions.length; i++) {
                let item = menuItemComponent.createObject(null)
                item.text = suggestions[i]
                if(suggestions[i].trim().length === 0) {
                    item.text = ("(clear empty text)")
                }
                contextMenu.addItem(item)
            }
        }

        Component {
            id: menuItemComponent
            Controls.MenuItem {
                onTriggered: {
                    let suggestion = text
                    if(suggestion === "(clear empty text)") {
                        suggestion = "";
                    }   
                    highlighter.replaceWord(mainPage.wordStart, mainPage.wordEnd, suggestion)
                }
            }
        }
    }

    Controls.TextArea {
        id: sourceArea
        anchors.fill: mainPage.contentItem
        padding: 5
        background: null
        wrapMode: Controls.TextArea.Wrap
        placeholderText: "Enter text..."
        Component.onCompleted: highlighter.setTextDocument(sourceArea.textDocument)
        onTextChanged: t => {
            helper.text_area_changed(sourceArea.text)
        }
    }

    MouseArea {
        anchors.fill: mainPage.contentItem
        acceptedButtons: Qt.RightButton
        onClicked: mouse => {
            let pos = sourceArea.positionAt(mouse.x, mouse.y)
            if (pos < 0) return

            let bounds = highlighter.findRecommendation(pos)
            if (bounds.length === 0) {
                contextMenu.rebuild([])
                return
            }

            mainPage.wordStart = bounds[0]
            mainPage.wordEnd = bounds[1]

            sourceArea.cursorPosition = mainPage.wordStart
            sourceArea.moveCursorSelection(mainPage.wordEnd, TextEdit.SelectCharacters)

            let items = highlighter.getSuggestions(mainPage.wordStart, mainPage.wordEnd)
            if (items.length > 0) {
                contextMenu.rebuild(items)
                contextMenu.popup(mouse.x, mouse.y + 10)
            }
        }
    }
}

