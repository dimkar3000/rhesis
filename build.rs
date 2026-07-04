use cxx_qt_build::{CppFile, CxxQtBuilder, QmlModule};

fn main() {
    let module = QmlModule::new("io.github.dimkar3000.rhesis");

    let paths = glob::glob("src/interop/qml/**/*.qml").unwrap();

    unsafe {
        CxxQtBuilder::new_qml_module(
            module.qml_files(paths.map(|x| x.unwrap().to_str().unwrap().to_string())),
        )
        .qt_module("Gui")
        .qt_module("Quick")
        .cpp_file(CppFile::from("src/interop/cpp/helper.h"))
        .files(["src/interop/bridge.rs"])
        .qrc("resources.qrc")
        .cc_builder(|a| {
            a.flag_if_supported("-w"); // Disabled warning from qt code base. We cannot fix those.
        })
        .build();
    }
}
