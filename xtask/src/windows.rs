//! Windows `.zip` packaging.
//!
//! The Windows binary embeds all of its assets (the icon allowlist + fonts are
//! `include_bytes!`-compiled in), so the distributable is simply `kagi.exe`
//! (plus `LICENSE`) zipped up — there is no asset directory to ship alongside
//! it. Zipping is delegated to PowerShell's `Compress-Archive` so xtask stays
//! dependency-free; this command is meant to run on the `windows-latest` CI
//! runner (x86_64).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const BIN_NAME: &str = "kagi.exe";

/// Resolve the binary to package. Prefer an explicit `--bin <path>` override
/// (CI passes the Windows build); otherwise fall back to
/// `target/release/kagi.exe`.
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

/// `bundle-windows [--bin <path>]`: stage `kagi.exe` (+ `LICENSE`) and zip it
/// into `target/dist/kagi-<version>-x86_64-windows.zip`.
pub fn bundle(root: &Path, override_bin: Option<&str>) -> Result<(), String> {
    let version = util::kagi_version(root)?;
    let bin = resolve_bin(root, override_bin)?;

    let dist = root.join("target").join("dist");
    let stem = format!("kagi-{version}-x86_64-windows");
    let stage = dist.join(&stem);
    util::clean_dir(&stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| format!("mkdir {}: {e}", stage.display()))?;

    std::fs::copy(&bin, stage.join(BIN_NAME)).map_err(|e| format!("copy bin: {e}"))?;
    let license = root.join("LICENSE");
    if license.exists() {
        std::fs::copy(&license, stage.join("LICENSE")).map_err(|e| format!("copy license: {e}"))?;
    }

    let zip = dist.join(format!("{stem}.zip"));
    if zip.exists() {
        std::fs::remove_file(&zip).map_err(|e| format!("rm old zip: {e}"))?;
    }

    // Compress the staged files (the `\*` glob lands them at the zip root rather
    // than nested under the stem directory). PowerShell ships on every Windows
    // runner, so no extra toolchain is needed.
    let glob = stage.join("*");
    println!(
        "bundle-windows: Compress-Archive {} -> {}",
        glob.display(),
        zip.display()
    );
    util::run(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
            glob.display(),
            zip.display()
        ),
    ]))?;

    util::clean_dir(&stage)?;
    println!("bundle-windows: wrote {}", zip.display());
    Ok(())
}
