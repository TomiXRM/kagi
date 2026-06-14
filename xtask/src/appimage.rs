//! Linux AppImage packaging (ADR-0047 追補, W21-APPIMAGE).
//!
//! `bundle-appimage --bin <path> [--arch x86_64|aarch64]` assembles a
//! `Kagi.AppDir/` (AppRun + kagi.desktop + kagi.png + usr/bin/kagi). When an
//! `appimagetool` is available — on `$APPIMAGETOOL` or `PATH` — it is invoked to
//! produce `target/dist/Kagi-<arch>.AppImage`; otherwise we stop after the
//! AppDir with a clear message (the local macOS case, where appimagetool is
//! absent). Finally a `target/dist/kagi_Linux-AppImage_<arch>.zip` is produced
//! containing the AppImage (when built) plus `kagi.png` and the install script.
//!
//! Stdlib-only: external tools (`appimagetool`, `zip`) are shelled out to.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const BIN_NAME: &str = "kagi";

/// Validate `--arch` (defaults to the AppImage convention names).
fn normalize_arch(arch: &str) -> Result<&'static str, String> {
    match arch {
        "x86_64" => Ok("x86_64"),
        "aarch64" | "arm64" => Ok("aarch64"),
        other => Err(format!(
            "unsupported --arch: {other} (expected x86_64 or aarch64)"
        )),
    }
}

fn desktop_entry() -> String {
    // `appimagetool` requires a top-level .desktop in the AppDir with a matching
    // Icon= key (no extension) and a Categories entry.
    "\
[Desktop Entry]
Type=Application
Name=Kagi
GenericName=Git Client
Comment=Safety-first Git GUI client
Exec=kagi %F
Icon=kagi
Terminal=false
Categories=Development;
StartupWMClass=Kagi
"
    .to_string()
}

fn apprun_script() -> String {
    // Minimal AppRun: resolve the AppDir and exec the bundled binary.
    "\
#!/bin/sh
HERE=\"$(dirname \"$(readlink -f \"${0}\")\")\"
exec \"${HERE}/usr/bin/kagi\" \"$@\"
"
    .to_string()
}

/// Resolve the binary to package (mirrors bundle-linux behaviour).
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

/// Locate an `appimagetool`: prefer `$APPIMAGETOOL`, else look it up on `PATH`.
fn find_appimagetool() -> Option<String> {
    if let Ok(p) = std::env::var("APPIMAGETOOL") {
        if !p.is_empty() && Path::new(&p).exists() {
            return Some(p);
        }
    }
    // `which`-style lookup over PATH (stdlib only).
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("appimagetool");
            if cand.is_file() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)
        .map_err(|e| format!("stat {}: {e}", p.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(p, perms).map_err(|e| format!("chmod {}: {e}", p.display()))
}

#[cfg(not(unix))]
fn make_executable(_p: &Path) -> Result<(), String> {
    Ok(())
}

/// `bundle-appimage --bin <path> [--arch ...]`.
pub fn bundle(root: &Path, override_bin: Option<&str>, arch: &str) -> Result<(), String> {
    let arch = normalize_arch(arch)?;
    let bin = resolve_bin(root, override_bin)?;
    let icon = root.join("assets/icon/icon_512x512.png");
    if !icon.exists() {
        return Err(format!(
            "{} not found — run `xtask icon` first",
            icon.display()
        ));
    }
    let install_script = root.join("scripts/install_linux_desktop.sh");
    if !install_script.exists() {
        return Err(format!("{} not found", install_script.display()));
    }

    let dist = root.join("target").join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("mkdir {}: {e}", dist.display()))?;

    // ── Assemble Kagi.AppDir ──────────────────────────────────────────────
    let appdir = dist.join("Kagi.AppDir");
    util::clean_dir(&appdir)?;
    let usr_bin = appdir.join("usr/bin");
    std::fs::create_dir_all(&usr_bin).map_err(|e| format!("mkdir {}: {e}", usr_bin.display()))?;

    std::fs::copy(&bin, usr_bin.join(BIN_NAME)).map_err(|e| format!("copy bin: {e}"))?;
    make_executable(&usr_bin.join(BIN_NAME))?;

    let apprun = appdir.join("AppRun");
    std::fs::write(&apprun, apprun_script()).map_err(|e| format!("write AppRun: {e}"))?;
    make_executable(&apprun)?;

    std::fs::write(appdir.join("kagi.desktop"), desktop_entry())
        .map_err(|e| format!("write desktop: {e}"))?;
    std::fs::copy(&icon, appdir.join("kagi.png")).map_err(|e| format!("copy icon: {e}"))?;

    println!("bundle-appimage: assembled {}", appdir.display());

    // ── Run appimagetool when present ─────────────────────────────────────
    let appimage = dist.join(format!("Kagi-{arch}.AppImage"));
    let built = match find_appimagetool() {
        Some(tool) => {
            if appimage.exists() {
                std::fs::remove_file(&appimage).map_err(|e| format!("rm old AppImage: {e}"))?;
            }
            println!(
                "bundle-appimage: {tool} {} {}",
                appdir.display(),
                appimage.display()
            );
            // CI invokes the downloaded tool with --appimage-extract-and-run so
            // FUSE is not required; pass ARCH so appimagetool labels correctly.
            let mut cmd = Command::new(&tool);
            cmd.env("ARCH", arch)
                .arg("--appimage-extract-and-run")
                .arg(&appdir)
                .arg(&appimage);
            util::run(&mut cmd)?;
            println!("bundle-appimage: wrote {}", appimage.display());
            true
        }
        None => {
            println!(
                "bundle-appimage: appimagetool not found (set $APPIMAGETOOL or add to PATH) — \
                 stopping after AppDir; no .AppImage produced (expected on local macOS)."
            );
            false
        }
    };

    // ── zip: AppImage (if built) + kagi.png + install script ──────────────
    let zip_path = dist.join(format!("kagi_Linux-AppImage_{arch}.zip"));
    if zip_path.exists() {
        std::fs::remove_file(&zip_path).map_err(|e| format!("rm old zip: {e}"))?;
    }

    // `zip` stores paths relative to its cwd, so stage the payload in a temp dir
    // and zip from there to get a flat archive (AppImage + kagi.png + script).
    let stage = dist.join("appimage-zip-stage");
    util::clean_dir(&stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| format!("mkdir {}: {e}", stage.display()))?;
    let scripts_dir = stage.join("scripts");
    std::fs::create_dir_all(&scripts_dir)
        .map_err(|e| format!("mkdir {}: {e}", scripts_dir.display()))?;

    std::fs::copy(appdir.join("kagi.png"), stage.join("kagi.png"))
        .map_err(|e| format!("stage icon: {e}"))?;
    std::fs::copy(
        &install_script,
        scripts_dir.join("install_linux_desktop.sh"),
    )
    .map_err(|e| format!("stage install script: {e}"))?;
    make_executable(&scripts_dir.join("install_linux_desktop.sh"))?;

    let mut zip_entries: Vec<String> =
        vec!["kagi.png".into(), "scripts/install_linux_desktop.sh".into()];
    if built {
        let name = appimage
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("AppImage filename is not valid UTF-8")?
            .to_string();
        std::fs::copy(&appimage, stage.join(&name)).map_err(|e| format!("stage AppImage: {e}"))?;
        make_executable(&stage.join(&name))?;
        zip_entries.insert(0, name);
    }

    println!("bundle-appimage: zip {}", zip_path.display());
    let mut cmd = Command::new("zip");
    cmd.current_dir(&stage).arg("-q").arg("-X").arg(&zip_path);
    for e in &zip_entries {
        cmd.arg(e);
    }
    util::run(&mut cmd)?;

    util::clean_dir(&stage)?;
    println!("bundle-appimage: wrote {}", zip_path.display());
    if !built {
        println!(
            "bundle-appimage: NOTE — zip contains the install script + icon only; \
             the AppImage is produced on the Linux CI runner where appimagetool runs."
        );
    }
    Ok(())
}
