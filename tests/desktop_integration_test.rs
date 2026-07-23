//! Linux desktop-integration contract: the window's runtime `app_id`
//! (`kagi::APP_ID`) must match every shipped `.desktop` launcher so GNOME/Mutter
//! (Wayland) and X11 window managers bind the window to the Kagi launcher
//! instead of spawning a separate generic ("unknown"/gear) taskbar entry.
//!
//! This locks the four historically-divergent desktop sources to one id:
//!   - `assets/linux/com.tomixrm.kagi.desktop`   (the `.deb`)
//!   - `scripts/install_linux_desktop.sh`        (AppImage install)
//!   - `xtask/src/linux.rs` / `appimage.rs`      (tar.gz / AppImage embed) —
//!     covered by unit tests in the `xtask` crate.

use std::path::PathBuf;

/// The reverse-DNS id gpui hands to Wayland `app_id` / X11 `WM_CLASS`.
const EXPECTED_APP_ID: &str = "com.tomixrm.kagi";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Value of a `Key=Value` line in a freedesktop entry (first match wins).
fn desktop_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .map(str::trim)
}

#[test]
fn app_id_is_the_reverse_dns_id() {
    // Guards against a well-meaning rename to a bare "kagi" that would silently
    // stop matching the installed `com.tomixrm.kagi.desktop` launcher.
    assert_eq!(kagi::APP_ID, EXPECTED_APP_ID);
    assert!(
        kagi::APP_ID.contains('.'),
        "app_id must stay reverse-DNS so it maps to <app_id>.desktop"
    );
}

#[test]
fn deb_desktop_file_id_and_wmclass_match_app_id() {
    // On Wayland GNOME matches the window to a launcher by desktop-file *id*
    // (basename) and by `StartupWMClass`; both must equal the runtime app_id.
    let rel = format!("assets/linux/{}.desktop", kagi::APP_ID);
    let contents = read(&rel);
    assert_eq!(
        desktop_value(&contents, "StartupWMClass"),
        Some(kagi::APP_ID),
        "{rel}: StartupWMClass must equal the runtime app_id",
    );
}

#[test]
fn install_script_desktop_id_and_wmclass_match_app_id() {
    // The AppImage installer writes `${APP_ID}.desktop` with
    // `StartupWMClass=${APP_ID}` — the path the user actually installs from.
    let script = read("scripts/install_linux_desktop.sh");
    assert!(
        script.contains(&format!("APP_ID=\"{}\"", kagi::APP_ID)),
        "install script APP_ID must equal the runtime app_id",
    );
    assert!(
        script.contains("${APP_ID}.desktop"),
        "install script must name the launcher ${{APP_ID}}.desktop",
    );
    assert!(
        script.contains("StartupWMClass=${APP_ID}"),
        "install script StartupWMClass must equal ${{APP_ID}}",
    );
}
