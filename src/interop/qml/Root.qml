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

    title: qsTr("app_title")

    onClosing: Qt.quit()

    property alias settingsPage: settingsPage

    Settings {
        id: appSettings
        // Server selection: true uses the bundled LanguageTool, false
        // connects to an external LanguageTool instance.
        property bool embedded: true
        // Port for the embedded server.
        property string port: "2689"
        property string defaultPort: "2689"
        // Base URL of the external server (e.g. "http://192.168.1.10:8081").
        property string externalAddress: ""
        property bool showTooltips: true
        property bool showDebugTooltips: true
        // Locale code (e.g. "el_GR"); empty means system locale.
        property string language: ""
        property var colorSettings: ({
                "CATEGORY:GRAMMAR": "#ecc224",
                "CATEGORY:TYPOGRAPHY": "#f63ef9",
                "CATEGORY:TYPOS": "#1ae918"
            })
    }

    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.NoNavigationButtons
    pageStack.columnView.scrollDuration: 0

    Component {
        id: settingsPage
        SettingsPage {
            settings: appSettings
            helper: messagingHelper
            langManager: languageManager
        }
    }

    LanguageManager {
        id: languageManager
    }

    CustomHighlighter {
        id: highlighter
        Component.onCompleted: {
            highlighter.startMessageThread(messagingHelper);
        }
    }

    AsyncMessagingHelper {
        id: messagingHelper
        Component.onCompleted: {
            messagingHelper.restart(appSettings.embedded, appSettings.embedded ? appSettings.port : appSettings.externalAddress);
            messagingHelper.update_colors(appSettings.colorSettings);
        }
    }

    pageStack.initialPage: MainPage {
        id: mainPage
        highlighter: highlighter
        helper: messagingHelper
        settings: appSettings
    }
}
