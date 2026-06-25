//! Per-target "bare binary" archives for package managers (ADR-0120).
//!
//! Produces `kagi-<version>-<target-triple>.{tar.gz|zip}` with the `kagi`
//! binary (and `LICENSE`) at the archive root. These are the artifacts consumed
//! by `cargo binstall`, mise's `ubi` backend, and the Homebrew formula —
//! deliberately named with the full Rust target triple so those tools can
//! auto-resolve the right asset from its name alone.
//!
//! The richer platform bundles (`.dmg`, AppImage `.zip`, the `bin/ + .desktop`
//! tarball) are unchanged and remain what the in-app updater consumes.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

/// Resolve the binary to package. Prefer an explicit `--bin <path>` override
/// (CI passes the freshly built release binary).
fn resolve_bin(
    root: &Path,
    override_path: Option<&str>,
    bin_name: &str,
) -> Result<PathBuf, String> {
    if let Some(p) = override_path {
        let pb = PathBuf::from(p);
        if !pb.exists() {
            return Err(format!("--bin path does not exist: {p}"));
        }
        return Ok(pb);
    }
    let p = root.join("target/release").join(bin_name);
    if !p.exists() {
        return Err(format!(
            "{} not found — build first (`cargo build --release`) or pass --bin <path>",
            p.display()
        ));
    }
    Ok(p)
}

/// `bundle-binarchive --target <triple> [--bin <path>]`: stage the bare binary
/// (+ LICENSE) and archive it as `kagi-<version>-<triple>.{tar.gz|zip}`.
pub fn bundle(root: &Path, override_bin: Option<&str>, target: &str) -> Result<(), String> {
    let version = util::kagi_version(root)?;
    let is_windows = target.contains("windows");
    let bin_name = if is_windows { "kagi.exe" } else { "kagi" };
    let bin = resolve_bin(root, override_bin, bin_name)?;

    let dist = root.join("target").join("dist");
    let stem = format!("kagi-{version}-{target}");
    let stage = dist.join(format!("binarchive-{target}"));
    util::clean_dir(&stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| format!("mkdir {}: {e}", stage.display()))?;

    let staged_bin = stage.join(bin_name);
    std::fs::copy(&bin, &staged_bin).map_err(|e| format!("copy bin: {e}"))?;
    let license = root.join("LICENSE");
    if license.exists() {
        std::fs::copy(&license, stage.join("LICENSE")).map_err(|e| format!("copy license: {e}"))?;
    }

    // macOS arm64 kills binaries without at least an ad-hoc signature. The
    // cargo linker usually applies one, but re-sign defensively on a macOS host.
    if target.contains("apple") && cfg!(target_os = "macos") {
        println!(
            "bundle-binarchive: codesign --force -s - {}",
            staged_bin.display()
        );
        util::run(Command::new("codesign").args([
            "--force",
            "-s",
            "-",
            staged_bin.to_str().ok_or("non-utf8 bin path")?,
        ]))?;
    }

    let ext = if is_windows { "zip" } else { "tar.gz" };
    let archive = dist.join(format!("{stem}.{ext}"));
    if archive.exists() {
        std::fs::remove_file(&archive).map_err(|e| format!("rm old archive: {e}"))?;
    }

    if is_windows {
        // PowerShell ships on the windows-latest runner; the `\*` glob lands the
        // staged files at the zip root rather than under the stage directory.
        let glob = stage.join("*");
        println!(
            "bundle-binarchive: Compress-Archive {} -> {}",
            glob.display(),
            archive.display()
        );
        util::run(Command::new("powershell").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
                glob.display(),
                archive.display()
            ),
        ]))?;
    } else {
        // Tar the staged files (binary, plus LICENSE when present) at the root.
        let mut cmd = Command::new("tar");
        cmd.args([
            "-czf",
            archive.to_str().ok_or("non-utf8 archive path")?,
            "-C",
            stage.to_str().ok_or("non-utf8 stage path")?,
            bin_name,
        ]);
        if license.exists() {
            cmd.arg("LICENSE");
        }
        println!("bundle-binarchive: tar czf {}", archive.display());
        util::run(&mut cmd)?;
    }

    util::clean_dir(&stage)?;
    println!("bundle-binarchive: wrote {}", archive.display());
    Ok(())
}
