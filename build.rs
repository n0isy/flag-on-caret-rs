// Embed the application icon (and version metadata from Cargo.toml) into the
// Windows exe, so it shows up in Explorer and on shortcuts. The same icon is
// used for the tray (via include_bytes! in main.rs) and the installer.
fn main() {
    println!("cargo:rerun-if-changed=icon/FlagOnCaret.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon/FlagOnCaret.ico");
        res.compile().expect("embed app icon resource");
    }
}
