import QtCore
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as Controls
import Qt.labs.platform as Platform
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

    // Hand the event to ours: closing hides instead of quitting when
    // background hosting is enabled, so the LanguageTool server stays up.
    // Hiding requires a working tray icon — without one a hidden app would
    // be stranded with no way back in, so it quits the traditional way.
    onClosing: close => {
        if (appSettings.background && appSettings.embedded && trayIcon.available) {
            close.accepted = false;
            root.visible = false;
        } else {
            close.accepted = true;
            Qt.quit();
        }
    }

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
        // Keep Rhesis alive when the window is closed so the embedded
        // LanguageTool server stays reachable for other applications
        // (http://127.0.0.1:<port>/v2/check) and the tray icon lets the
        // user reopen or quit.
        property bool background: false
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

    // Reopen the window from the tray (menu, click, ...).
    function showWindow() {
        root.visible = true;
        root.requestActivate();
    }

    // Tray-hosted background running: while the embedded LanguageTool server
    // keeps running behind a closed window, the tray icon is the way back
    // in (and the only place to shut the hosted server down gracefully).
    Platform.SystemTrayIcon {
        id: trayIcon

        // Worker status codes, see WorkerStatus::code in languagetool/service.rs:
        // 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed,
        // 4 = Connecting, 5 = FailedToRespond.
        readonly property int statusStopped: 0
        readonly property int statusStarting: 1
        readonly property int statusStarted: 2
        readonly property int statusFailed: 3
        readonly property int statusConnecting: 4
        readonly property int statusFailedToRespond: 5
        readonly property int workerStatus: messagingHelper ? messagingHelper.server_status : statusStopped
        readonly property bool serverBusy: workerStatus === statusStarting
            || workerStatus === statusStarted
            || workerStatus === statusConnecting
            || workerStatus === statusFailedToRespond

        // An embedded server is required for anything meaningful to sit in
        // the background; without it the tray would strand a hidden window
        // (external mode shares its own, remote server instead).
        visible: appSettings.background && appSettings.embedded

        // Must be a real "qrc:" URL — the ":/..." resource shorthand is not
        // resolved by Qt.labs.platform's icon loader, and the tray ends up
        // publishing an item with an empty IconName AND an empty IconPixmap,
        // which Plasma renders as a blank slot.
        icon.source: "qrc:/icons/hicolor/22x22/apps/logo.png"
        tooltip: {
            const address = "http://127.0.0.1:" + (appSettings.port || appSettings.defaultPort);
            switch (workerStatus) {
            case statusStarted:
            case statusFailedToRespond:
                return qsTr("tray_tooltip_running").arg(address);
            case statusFailed:
                return qsTr("tray_tooltip_failed");
            default:
                return qsTr("tray_tooltip_stopped").arg(address);
            }
        }

        onActivated: reason => {
            if (reason === Platform.SystemTrayIcon.Trigger
                || reason === Platform.SystemTrayIcon.DoubleClick) {
                root.showWindow();
            }
        }

        menu: Platform.Menu {
            Platform.MenuItem {
                text: qsTr("tray_show")
                onTriggered: root.showWindow()
            }
            Platform.MenuItem {
                text: trayIcon.serverBusy
                    ? qsTr("tray_stop_server")
                    : qsTr("tray_start_server")
                // While the worker latched into Failed only the in-window
                // retry action can recover it (see MAX_START_FAILURES);
                // starting from the tray would silently do nothing.
                enabled: appSettings.embedded && trayIcon.workerStatus !== trayIcon.statusFailed
                onTriggered: {
                    if (trayIcon.serverBusy)
                        messagingHelper.stop_server();
                    else
                        messagingHelper.start_server();
                }
            }
            Platform.MenuSeparator {}
            Platform.MenuItem {
                text: qsTr("tray_quit")
                // Qt.quit() tears the QML engine down, which drops
                // AsyncMessagingHelperRust and sends Kill to the worker:
                // the LanguageTool child is stopped before the app exits.
                onTriggered: Qt.quit()
            }
        }
    }

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
