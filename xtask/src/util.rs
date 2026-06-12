//! Small stdlib-only helpers shared by the xtask subcommands.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Workspace root = parent of the `xtask` crate directory (this file lives at
/// `<root>/xtask/src/util.rs`).
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // <root>/xtask
    manifest
        .parent()
        .expect("xtask crate dir has a parent")
        .to_path_buf()
}

/// Parse the `kagi` package version from the root `Cargo.toml`.
///
/// Hand-rolled (no toml crate) so xtask stays dependency-free. We scan for the
/// `[package]` table and return its first `version = "..."`.
pub fn kagi_version(root: &Path) -> Result<String, String> {
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    let mut in_package = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("version") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let v = rest.trim().trim_matches('"');
                    if !v.is_empty() {
                        return Ok(v.to_string());
                    }
                }
            }
        }
    }
    Err("could not find [package] version in root Cargo.toml".into())
}

/// Host architecture string used in artifact names: `arm64` or `x86_64`.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// Run a command, streaming its stdio, and fail on non-zero exit.
pub fn run(cmd: &mut Command) -> Result<(), String> {
    let rendered = render(cmd);
    let status = cmd
        .status()
        .map_err(|e| format!("spawn `{rendered}`: {e}"))?;
    if !status.success() {
        return Err(format!("`{rendered}` failed: {status}"));
    }
    Ok(())
}

fn render(cmd: &Command) -> String {
    let mut s = cmd.get_program().to_string_lossy().to_string();
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

/// Remove a directory tree if it exists (idempotent).
pub fn clean_dir(p: &Path) -> Result<(), String> {
    if p.exists() {
        std::fs::remove_dir_all(p).map_err(|e| format!("rm -rf {}: {e}", p.display()))?;
    }
    Ok(())
}
