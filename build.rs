use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let module = QmlModule::new("io.github.dimkar3000.rhesis");

    // The Windows UI lives in qml/windows/ and avoids Kirigami; the other
    // platforms use the Kirigami files next to it. Only one set may be
    // registered, otherwise the types clash and qmlcachegen has to resolve
    // `org.kde.kirigami` on Windows.
    let target_is_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");

    let paths = glob::glob("src/interop/qml/**/*.qml")
        .expect("invalid glob pattern for QML sources (src/interop/qml/**/*.qml)");

    let paths: Vec<String> = paths
        .map(|x| {
            x.expect("failed to read a QML source entry matched by the glob")
                .to_str()
                .expect("QML source path is not valid UTF-8")
                .replace('\\', "/")
        })
        .filter(|path| path.starts_with("src/interop/qml/windows/") == target_is_windows)
        .collect();

    unsafe {
        let mut builder = CxxQtBuilder::new_qml_module(module.qml_files(paths))
            .qt_module("Gui")
            .qt_module("Quick")
            .crate_include_root(Some("src/interop/cpp".to_string()))
            .files(["src/interop/bridge.rs"]);

        // The QtQuickTest module and C++ probes are always compiled so that
        // test targets (ui-boot, rhesis-ui-runner) can link them without a
        // feature gate. The shipped binary never references these symbols,
        // so they add nothing to the production artifact.
        builder = builder.qt_module("QuickTest").cc_builder(|a| {
            // Warnings from the Qt code base are not ours to fix.
            a.flag_if_supported("-w");
            // Skip -g for generated C++, which we cannot influence otherwise.
            a.debug(false);
            // quick_test_main is a plain exported Qt function that is only
            // reachable through its C++-mangled symbol; the wrapper keeps the
            // mangling off the Rust side by exposing a stable extern "C"
            // symbol instead.
            a.file("src/ui_tests/wrapper.cpp");
            // Probes for the full-boot E2E tier (see the file's own header):
            // plain extern "C" helpers around QQmlApplicationEngine,
            // QQmlExpression and QQuickWindow::grabWindow, so
            // tests/e2e/ui_boot.rs can drive the real UI without any
            // production bridge changes.
            a.file("src/ui_tests/harness.cpp");
        });

        builder.qrc("resources.qrc").build();
    }
}
