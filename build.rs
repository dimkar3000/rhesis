fn main() {
    // Shell-visible identity lives in the exe itself: embed version info and
    // the icon so Task manager's Startup list shows "Rhesis" with logo and
    // publisher instead of the bare file name with the generic one.
    let target_is_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if target_is_windows {
        embed_windows_resources();
    }
}

// Adding metadata for windows
fn embed_windows_resources() {
    let mut resources = winresource::WindowsResource::new();
    resources
        .set("FileDescription", "Rhesis")
        .set("ProductName", "Rhesis")
        .set("CompanyName", "dimkar3000")
        .set("LegalCopyright", "MIT License")
        .set("OriginalFilename", "rhesis.exe")
        .set_icon("assets/rhesis.ico");
    resources
        .compile()
        .expect("failed to embed the Windows resources into the executable");
}
