// Windows-only: embeds assets/icon/icon.ico as the .exe's resource icon (Explorer,
// taskbar, Alt-Tab) and basic version info. No-op on other targets — the
// `winresource` build-dependency itself is gated to `cfg(windows)` in Cargo.toml.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/icon.ico");
        if let Err(e) = res.compile() {
            eprintln!("build.rs: failed to embed Windows icon resource: {e}");
        }
    }
}
