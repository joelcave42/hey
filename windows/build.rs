// Embeds Hey's icon and version details in hey.exe, so Explorer, the taskbar
// and the installer show them.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("hey.ico");
        res.set("FileDescription", "Hey");
        res.set("ProductName", "Hey");
        res.compile().expect("couldn't embed the icon");
    }
    println!("cargo:rerun-if-changed=hey.ico");
}
