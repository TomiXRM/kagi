//! Linux `tar.gz` packaging (ADR-0047).
//!
//! Layout (relative to the tarball root `kagi-<version>-<arch>/`):
//!   bin/kagi
//!   share/applications/com.tomixrm.kagi.desktop
//!   share/icons/hicolor/512x512/apps/kagi.png
//!
//! On the CI ubuntu runner the binary is a real Linux build. On macOS (local
//! verification) there is no Linux binary, so we substitute the macOS release
//! binary purely to exercise the layout-generation logic — the resulting
//! tarball is for layout verification only, not for distribution.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const BIN_NAME: &str = "kagi";

fn desktop_entry() -> String {
    // `StartupWMClass` must equal the window's runtime app_id (`ui::APP_ID`,
    // "com.tomixrm.kagi") so the desktop environment binds the window to this
    // launcher instead of spawning a separate generic ("unknown") taskbar entry.
    "\
[Desktop Entry]
Type=Application
Name=Kagi
GenericName=Git Client
Comment=A Git GUI built with gpui
Exec=kagi
Icon=kagi
Terminal=false
Categories=Development;RevisionControl;
StartupWMClass=com.tomixrm.kagi
"
    .to_string()
}

/// Resolve the binary to package. Prefer an explicit `--bin <path>` override
/// (CI passes the Linux build); otherwise fall back to `target/release/kagi`.
fn resolve_bin(root: &Path, override_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(p) = override_path {
        let pb = PathBuf::from(p);
        if !pb.exists() {
            return Err(format!("--bin path does not exist: {p}"));
        }
        return Ok(pb);
    }
    let p = root.join("target/release").join(BIN_NAME);
    if !p.exists() {
        return Err(format!(
            "{} not found — build first (`cargo build --release`) or pass --bin <path>",
            p.display()
        ));
    }
    Ok(p)
}

/// `bundle-linux [--bin <path>]`: assemble the tar.gz layout and create the tarball.
pub fn bundle(root: &Path, override_bin: Option<&str>) -> Result<(), String> {
    let version = util::kagi_version(root)?;
    let bin = resolve_bin(root, override_bin)?;
    let icon = root.join("assets/icon/icon_512x512.png");
    if !icon.exists() {
        return Err(format!(
            "{} not found — run `xtask icon` first",
            icon.display()
        ));
    }

    let dist = root.join("target").join("dist");
    // AppImage-style arch names (x86_64 / aarch64); was hardcoded x86_64,
    // which made the two Linux CI legs emit colliding artifact names.
    let arch = util::host_arch_appimage();
    let stem = format!("{BIN_NAME}-{version}-{arch}");
    let stage = dist.join(&stem);
    util::clean_dir(&stage)?;

    let bin_dir = stage.join("bin");
    let apps_dir = stage.join("share/applications");
    let icon_dir = stage.join("share/icons/hicolor/512x512/apps");
    for d in [&bin_dir, &apps_dir, &icon_dir] {
        std::fs::create_dir_all(d).map_err(|e| format!("mkdir {}: {e}", d.display()))?;
    }

    std::fs::copy(&bin, bin_dir.join(BIN_NAME)).map_err(|e| format!("copy bin: {e}"))?;
    std::fs::write(apps_dir.join("com.tomixrm.kagi.desktop"), desktop_entry())
        .map_err(|e| format!("write desktop: {e}"))?;
    std::fs::copy(&icon, icon_dir.join("kagi.png")).map_err(|e| format!("copy icon: {e}"))?;

    let tarball = dist.join(format!("{stem}.tar.gz"));
    if tarball.exists() {
        std::fs::remove_file(&tarball).map_err(|e| format!("rm old tarball: {e}"))?;
    }

    println!("bundle-linux: tar czf {}", tarball.display());
    util::run(Command::new("tar").args([
        "-czf",
        tarball.to_str().unwrap(),
        "-C",
        dist.to_str().unwrap(),
        &stem,
    ]))?;

    util::clean_dir(&stage)?;
    println!("bundle-linux: wrote {}", tarball.display());
    if override_bin.is_none() && cfg!(target_os = "macos") {
        println!(
            "bundle-linux: NOTE — packaged the macOS binary for layout verification only; \
             the CI ubuntu runner produces the real Linux artifact."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Must equal the window's runtime `app_id` (`kagi::APP_ID`) so the desktop
    // environment binds the window to this launcher; see
    // `tests/desktop_integration_test.rs`.
    #[test]
    fn desktop_entry_wmclass_is_reverse_dns_app_id() {
        assert!(
            desktop_entry().contains("StartupWMClass=com.tomixrm.kagi"),
            "tar.gz desktop entry must set StartupWMClass to the app_id",
        );
    }
}
