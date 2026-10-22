fn main() {
    // Embed the app icon and version info into the Windows executable.
    #[cfg(windows)]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into());
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Отпуска");
        res.set("FileDescription", "Отпуска — учёт отпусков");
        res.set("ProductVersion", &version);
        res.set("FileVersion", &version);
        res.set("OriginalFilename", "Otpuska.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=cannot embed Windows resources: {e}");
        }
    }
}
