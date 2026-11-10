fn main() {
    println!("cargo:rerun-if-changed=../app.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../app.ico")
        .set("ProductName", "Sirah Mute")
        .set("FileDescription", "Sirah Mute: never talk into a muted mic again")
        .set("CompanyName", "Kurohiko")
        .set("LegalCopyright", "Copyright (c) 2026 Kurohiko")
        .set("ProductVersion", env!("CARGO_PKG_VERSION"))
        .set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.compile()
        .expect("winresource: failed to compile resources");
    slint_build::compile("ui/settings.slint").expect("slint: compile failed");
}
