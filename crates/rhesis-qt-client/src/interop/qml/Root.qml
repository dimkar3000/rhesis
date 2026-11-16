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

    // A hidden launch must not flash the window at login: the mapping is
    // gated here, before Qt ever shows the window. The alternative — letting
    // it open and calling appHideRootWindow() right after engine load —
    // arrives too late on X11: the map has already hit the screen, so the
    // late hide shows as a one-frame flicker. With no system tray the
    // window stays visible on purpose: the same stranding rule as the
    // close handler below (a hidden app with no way back must not
    // disappear into nowhere); appHideRootWindow enforces it identically.
    visible: !(hiddenLaunch && trayIcon.available)

    // Hand the event to ours: closing hides instead of quitting when
    // background hosting is enabled, so the LanguageTool server stays up.
    // Hiding requires a working tray icon — without one a hidden app would
    // be stranded with no way back in, so it quits the traditional way.
    onClosing: close => {
        if (messagingHelper.background && messagingHelper.embedded && trayIcon.available) {
            close.accepted = false;
            root.visible = false;
        } else {
            close.accepted = true;
            Qt.quit();
        }
    }

    property alias settingsPage: settingsPage

    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.NoNavigationButtons
    pageStack.columnView.scrollDuration: 0

    // Reopen the window from the tray (menu, click, ...).
    function showWindow() {
        root.visible = true;
        root.requestActivate();
    }

    // A launch with --hidden (the autostart entry's opener, or the CLI
    // flag) lands in the tray: the tray icon is shown for hidden launches
    // even when background hosting is off, because a hidden window with
    // nowhere to reopen from would strand the app. The root window is
    // never mapped for hidden launches when a system tray exists (the
    // `visible` gate above) — mapping it and hiding it afterwards would
    // flash on X11.
    readonly property bool hiddenLaunch:
        Qt.application.arguments.indexOf("--hidden") !== -1

    // Background hosting offers the tray always; a hidden launch does
    // too — the tray is the only way back in for a hidden window.
    readonly property bool _shouldShowTray: (messagingHelper.background && messagingHelper.embedded) || hiddenLaunch

    // Tray-hosted background running: while the embedded LanguageTool server
    // keeps running behind a closed window, the tray icon is the way back
    // in (and the only place to shut the hosted server down gracefully).
    Platform.SystemTrayIcon {
        id: trayIcon

        // Worker status codes, see WorkerStatus::code in rhesis-protocol:
        // 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed,
        // 4 = Connecting, 5 = FailedToRespond; 6 = client-local
        // Disconnected (no daemon session).
        readonly property int statusStopped: 0
        readonly property int statusStarting: 1
        readonly property int statusStarted: 2
        readonly property int statusFailed: 3
        readonly property int statusConnecting: 4
        readonly property int statusFailedToRespond: 5
        readonly property int statusDisconnected: 6
        readonly property int workerStatus: messagingHelper ? messagingHelper.server_status : statusStopped
        readonly property bool serverBusy: workerStatus === statusStarting
            || workerStatus === statusStarted
            || workerStatus === statusConnecting
            || workerStatus === statusFailedToRespond

        // Background hosting offers the tray always; a hidden launch does
        // too — the tray is the only way back in for a hidden window.
        visible: _shouldShowTray

        // Must be a real "qrc:" URL — the ":/..." resource shorthand is not
        // resolved by Qt.labs.platform's icon loader, and the tray ends up
        // publishing an item with an empty IconName AND an empty IconPixmap,
        // which Plasma renders as a blank slot.
        icon.source: "qrc:/icons/hicolor/256x256/apps/logo.png"
        tooltip: {
            const address = "http://127.0.0.1:" + (messagingHelper.port || messagingHelper.default_port);
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
            // Qt.labs.platform Menu.visible defaults to true; the KDE tray
            // integration applies that default when the item registers,
            // popping the menu up as a stray window and stealing the
            // window's focus. Only Plasma may open the menu, on demand.
            visible: false
            Platform.MenuItem {
                text: qsTr("tray_show")
                onTriggered: root.showWindow()
            }
            Platform.MenuItem {
                text: trayIcon.serverBusy
                    ? qsTr("tray_stop_server")
                    : qsTr("tray_start_server")
                // While the daemon's failure budget latched into Failed only
                // the in-window retry action can recover it; starting from
                // the tray would silently do nothing.
                enabled: messagingHelper.embedded && trayIcon.workerStatus !== trayIcon.statusFailed
                onTriggered: {
                    if (trayIcon.serverBusy)
                        messagingHelper.stop_server();
                    else
                        messagingHelper.start_server();
                }
            }
            Platform.MenuSeparator {}
            Platform.MenuItem {
                text: qsTr("tray_quit_service")
                // The daemon is the background process: quitting it takes the
                // LanguageTool server down for every client, which is why
                // this is an explicit menu item instead of the implicit app
                // exit. The UI follows (a disconnected UI is of no use; the
                // connection loop stops on the same Shutdown command).
                onTriggered: {
                    messagingHelper.quit_service();
                    Qt.quit();
                }
            }
            Platform.MenuItem {
                text: qsTr("tray_quit")
                // Qt.quit() tears the QML engine down, which drops the
                // AsyncMessagingHelper: with background hosting off its Drop
                // asks the daemon to quit, so the LanguageTool child is
                // stopped before the app exits. With background on the
                // daemon keeps running and the tray comes back at login.
                onTriggered: Qt.quit()
            }
        }
    }

    Component {
        id: settingsPage
        SettingsPage {
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
    }

    pageStack.initialPage: MainPage {
        id: mainPage
        highlighter: highlighter
        helper: messagingHelper
    }
}
