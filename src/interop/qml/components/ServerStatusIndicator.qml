import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    property var helper
    // Worker status codes, see WorkerStatus::code in languagetool/service.rs:
    // 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed.
    readonly property int statusStopped: 0
    readonly property int statusStarting: 1
    readonly property int statusStarted: 2
    readonly property int statusFailed: 3
    property int status: helper ? helper.server_status : statusStopped
    property string reason: helper ? helper.server_status_reason : ""

    implicitWidth: row.implicitWidth
    implicitHeight: row.implicitHeight

    function dotColor(): string {
        switch (status) {
        case statusStarted:
            return Kirigami.Theme.positiveTextColor;
        case statusStarting:
            return Kirigami.Theme.neutralTextColor;
        case statusFailed:
            return Kirigami.Theme.negativeTextColor;
        default:
            return Kirigami.Theme.disabledTextColor;
        }
    }

    function statusText(): string {
        switch (status) {
        case statusStarted:
            return qsTr("lt_started");
        case statusStarting:
            return qsTr("lt_starting");
        case statusFailed:
            return qsTr("lt_failed");
        default:
            return qsTr("lt_stopped");
        }
    }

    RowLayout {
        id: row
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing

        Rectangle {
            id: dot
            Layout.preferredWidth: 10
            Layout.preferredHeight: 10
            Layout.alignment: Qt.AlignVCenter
            radius: 5
            color: root.dotColor()
        }

        Controls.Label {
            text: root.statusText()
            color: Kirigami.Theme.textColor
            elide: Text.ElideRight
            Layout.alignment: Qt.AlignVCenter
        }

        Controls.ToolButton {
            visible: root.status === root.statusFailed
            icon.name: "view-refresh"
            text: qsTr("lt_retry")
            display: Controls.AbstractButton.IconOnly
            Layout.alignment: Qt.AlignVCenter
            onClicked: {
                if (root.helper)
                    root.helper.retry_server();
            }
            Controls.ToolTip {
                text: root.reason !== "" ? root.reason : qsTr("lt_retry_tooltip")
                visible: parent.hovered
            }
        }
    }

    MouseArea {
        id: hoverMouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.NoButton
    }

    Controls.ToolTip {
        text: root.status === root.statusFailed && root.reason !== "" ? root.reason : root.statusText()
        visible: hoverMouse.containsMouse
    }
}
