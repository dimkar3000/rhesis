//! Build script for the headless daemon: on Windows it embeds the same
//! identity block the client gets (see the root `build.rs`), so both exes
//! show proper version metadata in Explorer and Task manager. The daemon
//! is a published service process, not a background task spawned from the
//! client, so its list entries read "Rhesis Service" with the shared
//! product name, publisher and icon.

fn main() {
    let target_is_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");

    if target_is_windows {
        embed_windows_resources();
    }
}

/// Version information and icon for the Windows executable; see the root
/// `build.rs` for why the shell needs these. FileDescription and
/// OriginalFilename distinguish the daemon from the client, everything
/// else (product name, publisher, icon) is shared. Relative icon path:
/// build scripts run with the crate manifest directory as cwd.
fn embed_windows_resources() {
    let mut resources = winresource::WindowsResource::new();
    resources
        .set("FileDescription", "Rhesis Service")
        .set("ProductName", "Rhesis")
        .set("CompanyName", "dimkar3000")
        .set("LegalCopyright", "MIT License")
        .set("OriginalFilename", "rhesis-service.exe")
        .set_icon("../../assets/rhesis.ico");
    resources
        .compile()
        .expect("failed to embed the Windows resources into rhesis-service.exe");
}
