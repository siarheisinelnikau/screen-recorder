fn main() {
    println!("cargo:rerun-if-changed=assets/ScreenRecorder.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/ScreenRecorder.ico")
            .set("ProductName", "ScreenRecorder")
            .set("FileDescription", "ScreenRecorder")
            .compile()
            .expect("Compile Windows application icon and version resources");
    }
}
