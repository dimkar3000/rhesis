import QtCore
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis
import org.kde.kirigamiaddons.formcard as FormCard

Kirigami.ApplicationWindow {
    id: root

    width: 1280
    height: 720
    maximumWidth: 1280
    maximumHeight: 720

    title: "Hello Linux"

    onClosing: Qt.quit()

    property alias settingsPage: settingsPage

    Settings {
        id: appSettings
        property bool embedded: true
        property string port: "2689"
        property string defaultPort: "2689"
    }

    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.NoNavigationButtons
    pageStack.columnView.scrollDuration: 0

    Component {
        id: settingsPage
        SettingsPage {
            settings: appSettings
            helper: messagingHelper
        }
    }

    CustomHighlighter {
        id: highlighter
        Component.onCompleted: highlighter.startMessageThread(messagingHelper)
    }

    AsyncMessagingHelper {
        id: messagingHelper
        Component.onCompleted: { 
            messagingHelper.restart(appSettings.embedded, appSettings.port)
        }
    }

    pageStack.initialPage: MainPage {
        id: mainPage
        highlighter: highlighter
        helper: messagingHelper
    }
}