use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

use cxx_qt_lib_extras::QApplication;
use lazy_static::lazy_static;

use std::env;

use rhesis::interop::bridge;
use rhesis::interop::language::{find_translations_dir, persisted_language};

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
