fn main() {
    // Embeds the app icon in the .exe (Explorer, taskbar, shortcuts, installer).
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/blackhole.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/blackhole.ico");
        if let Err(err) = res.compile() {
            println!("cargo:warning=cannot embed the Windows icon: {err}");
        }
    }
}
