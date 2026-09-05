use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let module = QmlModule::new("io.github.dimkar3000.rhesis");

    let paths = glob::glob("src/interop/qml/**/*.qml")
        .expect("invalid glob pattern for QML sources (src/interop/qml/**/*.qml)");

    unsafe {
        CxxQtBuilder::new_qml_module(module.qml_files(paths.map(|x| {
            x.expect("failed to read a QML source entry matched by the glob")
                .to_str()
                .expect("QML source path is not valid UTF-8")
                .to_string()
        })))
        .qt_module("Gui")
        .qt_module("Quick")
        .crate_include_root(Some("src/interop/cpp".to_string()))
        .files(["src/interop/bridge.rs"])
        .qrc("resources.qrc")
        .cc_builder(|a| {
            a.flag_if_supported("-w"); // Disabled warning from qt code base. We cannot fix those.
            a.debug(false); // Skip -g for generated C++, not match we can do with this either.
        })
        .build();
    }
}
