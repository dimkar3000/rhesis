import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis

Kirigami.Page {
    id: secondPage

    title: "Settings"

    property var settings
    property AsyncMessagingHelper helper
    property bool localEmbedded: false
    property string localPort: ""
    property bool hasChanges: localEmbedded !== settings.embedded || localPort !== settings.port

    Component.onCompleted: {
        localEmbedded = settings.embedded
        localPort = settings.port
    }

    function isValidLocalPort(port) {
        try {
            let url = new URL(`http://localhost:${port}`)
            
            // For local server we want a valid port and for it not to be empty 
            return port === url.port && port !== "";
        } catch(err) {
            return false;
        }
    }

    property bool localPortValid: isValidLocalPort(portField.text)

    actions: [
        Kirigami.Action {
            icon.name: "dialog-ok-apply"
            text: "Apply"
            enabled: hasChanges && ((localEmbedded && localPortValid) || !localEmbedded)
            onTriggered: {
                if(hasChanges) {
                    helper.restart(localEmbedded, localPort)
                }

                settings.embedded = localEmbedded
                if(!localEmbedded) 
                {
                    settings.port = ""
                } else {
                    settings.port = localPort
                }

                applicationWindow().pageStack.layers.pop()
            }
        }
    ]

    Item {
        id: formBox
        width: formLayout.width + Kirigami.Units.smallSpacing * 2
        height: formLayout.height + Kirigami.Units.smallSpacing * 2
        x: (parent.width - width) / 2
        y: Kirigami.Units.smallSpacing

        Kirigami.FormLayout {
            id: formLayout
            x: Kirigami.Units.smallSpacing
            y: Kirigami.Units.smallSpacing

            Kirigami.Separator {
                    Kirigami.FormData.isSection: true
                    Kirigami.FormData.label: "Embedded LanguageTool Settings"
            }

            Controls.Switch {
                id: embedSwitch
                Kirigami.FormData.label: "Enable"
                checked: localEmbedded
                onToggled: localEmbedded = checked
            }

            RowLayout {
                Kirigami.FormData.label: "Port"

                Controls.TextField {
                    id: portField
                    Layout.fillWidth: true
                    enabled: localEmbedded
                    placeholderText: "(2689)"
                    text: localPort
                    onTextChanged: localPort = text

                    background: Rectangle {
                        color: Kirigami.Theme.backgroundColor
                        border.color: localPortValid || !localEmbedded
                            ? ((!localPort && portField.focused) ? Kirigami.Theme.activeTextColor : Kirigami.Theme.textColor)
                            : Kirigami.Theme.negativeTextColor
                        border.width: 1
                        radius: 2
                    }
                    
                    validator: IntValidator{ 
                        bottom: 1
                        top: 65534
                    }
                    
                    Controls.ToolTip {
                        text: "Available options: [1-65534]"
                        visible: parent.hovered
                    }
                }

                Controls.Button {
                    id: resetBtn
                    icon.name: "document-revert"
                    enabled: localEmbedded && localPort !== settings.defaultPort
                    onClicked: localPort = settings.defaultPort
                    Controls.ToolTip {
                        text: "Reset to default port"
                        visible: parent.hovered
                    }
                }
            }
        }
    }
}
