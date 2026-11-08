fn main() {
    println!("cargo:rerun-if-changed=../app.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../app.ico")
        .set("ProductName", "Kuro Mute & Force Mic")
        .set("FileDescription", "Kuro Mute & Force Mic")
        .set("ProductVersion", env!("CARGO_PKG_VERSION"))
        .set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.compile()
        .expect("winresource: failed to compile resources");
}
