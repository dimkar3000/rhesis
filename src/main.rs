use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

use cxx_qt_lib_extras::QApplication;
use lazy_static::lazy_static;

use std::{
    env,
    path::{Path, PathBuf},
};

use rhesis::interop::bridge;

lazy_static! {
    static ref NAMESPACE: QString = QString::from("io.github.dimkar3000.rhesis");
    static ref ROOT_QML_FILE_PATH: QUrl = QUrl::from(&format!(
        "qrc:/qt/qml/{}/src/interop/qml/Root.qml",
        NAMESPACE.to_string().replace(".", "/")
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

    bridge::ffi::setupIconTheme();

    let translations_dir = find_translations_dir();
    log::info!("using translations directory: {:?}", translations_dir);

    if let Some(mut app) = app.as_mut() {
        if let Some(dir) = translations_dir {
            let dir = dir.to_string_lossy().to_string();
            if !bridge::ffi::installTranslation(app.as_mut(), &QString::from(&dir)) {
                log::warn!("failed to install translations from {dir}");
            }
        }
    }

    let mut engine = QQmlApplicationEngine::new();

    // To associate the executable to the installed desktop file
    QGuiApplication::set_desktop_file_name(&NAMESPACE);

    // To ensure the style is set correctly
    let style = env::var("QT_QUICK_CONTROLS_STYLE");
    match style {
        Ok(style) => log::debug!("using QT_QUICK_CONTROLS_STYLE={style}"),
        Err(_) => QQuickStyle::set_style(&QString::from("org.kde.desktop")),
    }

    if let Some(engine) = engine.as_mut() {
        engine.load(&ROOT_QML_FILE_PATH);
    }

    log::info!("Initialized");
    if let Some(mut app) = app.as_mut() {
        bridge::ffi::appSetWindowIcon(app.as_mut(), &LOGO_PATH);
        app.exec();
    }
}

fn find_translations_dir_with_prefix(prefix: &str) -> Option<PathBuf> {
    let candidate = Path::new(&format!("{prefix}/rhesis/translations")).to_path_buf();
    log::debug!("Looking for translations in: {candidate:?}");
    if candidate.exists() {
        log::info!("Found translations: {candidate:?}");

        return Some(candidate);
    }

    None
}

fn find_translations_dir() -> Option<PathBuf> {
    use std::path::Path;

    if let Some(path) = find_translations_dir_with_prefix("/usr/share") {
        return Some(path);
    }

    // Per-user install: $HOME/.local/share/rhesis/translations
    if let Ok(home) = env::var("HOME") {
        if let Some(candidate) = find_translations_dir_with_prefix(&format!("{home}/.local/share"))
        {
            return Some(candidate);
        }
    }

    // AppImage: path relative to $APPDIR
    if let Ok(appdir) = env::var("APPDIR") {
        if let Some(candidate) = find_translations_dir_with_prefix(&format!("{appdir}/app/share")) {
            return Some(candidate);
        }
    }

    // Flatpak: the files are installed in /app
    if let Some(path) = find_translations_dir_with_prefix("/app/share") {
        return Some(path);
    }

    // Current working directory (dev fallback)
    let candidate = Path::new("./translations");
    if candidate.is_dir() {
        log::info!("Found translations in CWD");
        return Some(candidate.to_path_buf());
    }

    log::error!("Failed to find translations directory.");

    None
}
