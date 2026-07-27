fn main() {
    slint_build::compile("ui/app-window.slint").expect("failed to compile Slint UI");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("icons/icon.ico")
            .set("ProductName", "OVC")
            .set("FileDescription", "OVC Desktop")
            .set("InternalName", "ovc_desktop.exe")
            .set("OriginalFilename", "ovc_desktop.exe")
            .compile()
            .expect("failed to compile Windows application resources");
    }
}
