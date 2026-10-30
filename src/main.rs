// Release builds on Windows are GUI-subsystem apps: no console window next to
// the UI. Debug builds keep the console so `cargo run` still shows the logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

use cxx_qt_lib_extras::QApplication;
use lazy_static::lazy_static;

use std::env;

use rhesis::interop::bridge;
use rhesis::interop::language::{find_translations_dir, persisted_language};

// Windows has its own Kirigami-free QML (see build.rs).
#[cfg(windows)]
const ROOT_QML_SOURCE: &str = "src/interop/qml/windows/Root.qml";
#[cfg(not(windows))]
const ROOT_QML_SOURCE: &str = "src/interop/qml/Root.qml";

#[cfg(windows)]
const DEFAULT_QUICK_STYLE: &str = "FluentWinUI3";
#[cfg(not(windows))]
const DEFAULT_QUICK_STYLE: &str = "org.kde.desktop";

lazy_static! {
    static ref NAMESPACE: QString = QString::from("io.github.dimkar3000.rhesis");
    static ref ROOT_QML_FILE_PATH: QUrl = QUrl::from(&format!(
        "qrc:/qt/qml/{}/{}",
        NAMESPACE.to_string().replace(".", "/"),
        ROOT_QML_SOURCE
    ));
    static ref LOGO_PATH: QString = QString::from(":/icons/hicolor/22x22/apps/logo.png");
}

#[tokio::main()]
async fn main() {
    run_ui();
}

fn run_ui() {
    env_logger::init();

    let mut app = QApplication::new();
    if let Some(mut app) = app.as_mut() {
        use std::pin::Pin;
        Pin::as_mut(&mut app).set_organization_name(&QString::from("rhesis"));
        Pin::as_mut(&mut app).set_organization_domain(&QString::from("dimkar.org"));
        Pin::as_mut(&mut app).set_application_name(&QString::from("Rhesis"));
    }

    #[cfg(windows)]
    bridge::ffi::useIniSettingsFormat();

    // Apply the persisted "keep running in the background" preference before
    // anything creates windows: with it enabled, hiding the last window must
    // not quit the whole app (the LanguageTool server stays up for other apps).
    log::debug!("syncing background behavior from the persisted settings");
    bridge::ffi::syncBackgroundBehavior();

    // Single-instance guard: a second launch asks the running app to show
    // its window and exits without creating any UI of its own. Deliberately
    // runs before QApplication exists, so a secondary instance is cheap.
    if !rhesis::single_instance::acquire() {
        log::info!("another Rhesis instance is running; asked it to show its window");
        return;
    }

    // A relocated AppImage (moved, renamed for a version bump, re-homed by
    // Gear Lever…) left the autostart entry pointing at the old path, so the
    // login launch dies with systemd 203/EXEC — the file no longer exists.
    // The running image is the source of truth for where it lives now, so
    // every manual launch repoints the entry at itself; the next login works
    // again. No-op for every other flavor (self_heal only acts under
    // $APPIMAGE, never a mount path) and for entries that are already
    // correct. A stale entry still cannot heal at login — the exec fails
    // before any of our code runs — so the first launch after moving the
    // file must be a manual one (toggling autostart on off→on would also
    // rewrite it, but the heal makes that unnecessary).
    rhesis::autostart::self_heal();

    bridge::ffi::setupIconTheme();

    let translations_dir = find_translations_dir();
    log::info!("using translations directory: {:?}", translations_dir);

    if let Some(dir) = translations_dir {
        let dir = dir.to_string_lossy().to_string();
        let preferred = persisted_language();
        if !preferred.is_empty() {
            log::info!("using persisted language override: {preferred}");
        }
        if !bridge::ffi::applyLanguage(&QString::from(&dir), &QString::from(&preferred)) {
            log::warn!("failed to install translations from {dir}");
        }
    }

    let mut engine = QQmlApplicationEngine::new();

    // To associate the executable to the installed desktop file
    QGuiApplication::set_desktop_file_name(&NAMESPACE);

    // To ensure the style is set correctly
    let style = env::var("QT_QUICK_CONTROLS_STYLE");
    match style {
        Ok(style) => log::debug!("using QT_QUICK_CONTROLS_STYLE={style}"),
        Err(_) => QQuickStyle::set_style(&QString::from(DEFAULT_QUICK_STYLE)),
    }

    if let Some(engine) = engine.as_mut() {
        engine.load(&ROOT_QML_FILE_PATH);
    }

    // Autostart CLI flag: keep the app from flashing its window at login.
    // Root.qml gates the root window's own visibility (hiddenLaunch &&
    // tray availability), so for a hidden launch with a system tray the
    // window is never mapped at all; this call then finds nothing to hide.
    // It stays as the guard for other paths and for top-levels that end
    // up visible anyway (helper.h refuses to hide when no tray exists —
    // a hidden window without a reopen path must stay visible instead).
    if env::args().any(|arg| arg == "--hidden") {
        bridge::ffi::appHideRootWindow();
    }

    log::info!("Initialized");
    if let Some(mut app) = app.as_mut() {
        bridge::ffi::appSetWindowIcon(app.as_mut(), &LOGO_PATH);
        app.exec();
    }
}
