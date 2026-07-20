use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

use cxx_qt_lib_extras::QApplication;
use lazy_static::lazy_static;

use std::{env, path::PathBuf};

use crate::interop::bridge;

mod interop;
mod languagetool;

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
    log::info!("Starting LanguageTool");

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
    if translations_dir.is_none() {
        log::error!("Failed to detect translation dir. The translation is probably broken.");
    }
    else {
        let translations_dir = translations_dir.unwrap().to_string_lossy().to_string();
        log::info!("using translations directory: {}", translations_dir);

        if let Some(mut app) = app.as_mut() {
            bridge::ffi::installTranslation(app.as_mut(), &QString::from(&translations_dir));
        }
    }

    let mut engine = QQmlApplicationEngine::new();

    // To associate the executable to the installed desktop file
    QGuiApplication::set_desktop_file_name(&NAMESPACE);

    // To ensure the style is set correctly
    let style = env::var("QT_QUICK_CONTROLS_STYLE");
    if style.is_err() {
        QQuickStyle::set_style(&QString::from("org.kde.desktop"));
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

fn find_translations_dir() -> Option<PathBuf> {
    use std::path::Path;

    // Search in the current dir
    let path = Path::new("./translations");
    if path.is_dir() {
        log::info!("Found translations in cwd");
        return Some(path.to_path_buf())
    }

    // Search next to the executable 
    let exe_location = match std::env::current_exe() {
        Ok(x) => {
            log::info!("Executable: {x:?}");
            x
        },
        Err(e) => {
            panic!("Failed to find the location of the executable: {e:?}");
        },
    };

    let path = exe_location.parent()?.join("translations");
    log::info!("PATH: {path:?}");
    if path.is_dir()  {
        return Some(path)
    }

    None
}
