import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Item {
    id: root

    property var helper
    // Worker status codes, see WorkerStatus::code in languagetool/service.rs:
    // 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed,
    // 4 = Connecting, 5 = FailedToRespond.
    readonly property int statusStopped: 0
    readonly property int statusStarting: 1
    readonly property int statusStarted: 2
    readonly property int statusFailed: 3
    readonly property int statusConnecting: 4
    readonly property int statusFailedToRespond: 5
    property int status: helper ? helper.server_status : statusStopped
    property string reason: helper ? helper.server_status_reason : ""
    // Manual retry only makes sense when nothing retries on its own: a
    // `FailedToRespond` outage already re-probes on the next typed request
    // (and recovers automatically), so it gets no button.
    readonly property bool canRetry: status === statusFailed
    readonly property bool dark: Application.styleHints.colorScheme === Qt.ColorScheme.Dark

    implicitWidth: row.implicitWidth
    implicitHeight: row.implicitHeight

    // Fluent status colors, one pair per color scheme (no Kirigami.Theme here).
    function dotColor(): string {
        switch (status) {
        case statusStarted:
            return dark ? "#6ccb5f" : "#0f7b0f";
        case statusStarting:
        case statusConnecting:
            return dark ? "#fce100" : "#9d5d00";
        case statusStopped:
            return dark ? "#8a8a8a" : "#767676";
        default:
            return dark ? "#ff99a4" : "#c42b1c";
        }
    }

    function statusText(): string {
        switch (status) {
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

    RowLayout {
        id: row
        anchors.fill: parent
        spacing: 8

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
            elide: Text.ElideRight
            Layout.alignment: Qt.AlignVCenter
        }

        Controls.ToolButton {
            visible: root.canRetry
            icon.source: "qrc:/icons/windows/refresh.svg"
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
