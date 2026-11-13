// Release builds on Windows are GUI-subsystem apps: no console window next to
// the UI. Debug builds keep the console so `cargo run` still shows the logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    rhesis_qt_client::run();
}
