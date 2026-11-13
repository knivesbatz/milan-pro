// Windows: the app icon (resource ID 1, what Explorer, the taskbar and shortcuts show) and the version info.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/brand/milan-pro.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // FileVersion and ProductVersion come from the Cargo.toml version.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/brand/milan-pro.ico")
        .set("ProductName", "Milan Pro")
        .set("FileDescription", "Milan Pro video editor")
        .set("CompanyName", "Milan Pro Contributors")
        .set("LegalCopyright", "Copyright (c) Milan Pro Contributors. MIT License.")
        .set("OriginalFilename", "MilanPro.exe")
        .set("InternalName", "MilanPro");
    res.compile().expect("embedding the icon and version resource (needs rc.exe from the Windows SDK)");
}
