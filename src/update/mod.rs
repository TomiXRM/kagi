//! Auto-update I/O (ADR-0082, T-AUTOUPDATE-001).
//!
//! The **only** layer that does network + filesystem for updates. The pure model
//! (version compare, release-JSON parse, asset/plan selection, checksum lookup)
//! lives in [`kagi_domain::update`]; the view calls *this* module, never `ureq`
//! or `std::fs` directly.
//!
//! Flow: [`check_latest`] (GitHub API) → `kagi_domain::update::plan_update` →
//! (on user confirm) [`install`] (download → SHA-256 verify → extract → atomic
//! swap → relaunch). Checking is best-effort/silent; installing is always
//! user-confirmed and checksum-verified, writes atomically, and runs no
//! destructive command (ADR-0082).

use std::path::Path;

use kagi_domain::update::{self, Asset, ReleaseInfo, UpdatePlan, Version};

const REPO: &str = "TomiXRM/kagi";
const USER_AGENT: &str = concat!("kagi/", env!("CARGO_PKG_VERSION"), " (auto-update)");

/// How to relaunch after a successful in-place update.
#[derive(Debug, Clone)]
pub struct Relaunch {
    pub program: String,
    pub args: Vec<String>,
}

impl Relaunch {
    /// Spawn the new process (detached) and exit this one. Never returns.
    pub fn spawn_and_exit(&self) -> ! {
        let _ = std::process::Command::new(&self.program)
            .args(&self.args)
            .spawn();
        std::process::exit(0);
    }
}

/// The running version, honoring the `KAGI_UPDATE_FORCE_CURRENT` test override
/// (set it to e.g. `0.0.1` to make any real release look like an update).
pub fn current_version() -> Version {
    if let Ok(forced) = std::env::var("KAGI_UPDATE_FORCE_CURRENT") {
        if let Some(v) = Version::parse(&forced) {
            return v;
        }
    }
    Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is valid semver")
}

/// Fetch the latest GitHub release. Best-effort: any network/parse error is a
/// `String` the caller logs and ignores (no update offered).
pub fn check_latest() -> Result<ReleaseInfo, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = http_text(&url)?;
    update::parse_release_json(&body).ok_or_else(|| "could not parse release JSON".to_string())
}

/// Convenience used by the UI startup check: fetch + decide. Returns the plan and
/// the release it came from (the release carries the asset list needed to fetch
/// checksums at install time). `skipped` is the user's "skip this version" tag.
pub fn check_for_update(
    skipped: Option<&str>,
) -> Result<Option<(UpdatePlan, ReleaseInfo)>, String> {
    let release = check_latest()?;
    let plan = update::plan_update(
        &current_version(),
        &release,
        std::env::consts::OS,
        std::env::consts::ARCH,
        skipped,
    );
    Ok(plan.map(|p| (p, release)))
}

// ────────────────────────────────────────────────────────────
// HTTP (ureq, blocking, rustls — same client family as avatar_fetch)
// ────────────────────────────────────────────────────────────

fn http_text(url: &str) -> Result<String, String> {
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("read {url}: {e}"))
}

fn http_bytes(url: &str) -> Result<Vec<u8>, String> {
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .body_mut()
        // Release assets are tens of MB; lift the default read cap generously.
        .with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("download {url}: {e}"))
}

// ────────────────────────────────────────────────────────────
// Install (download → verify → extract → swap → relaunch)
// ────────────────────────────────────────────────────────────

/// Download the planned asset, verify its SHA-256 against the release's
/// `SHA256SUMS-*.txt`, extract it, swap it into the running install, and return
/// how to relaunch. `log` receives human-readable progress lines.
///
/// On any failure the running install is left untouched (verification happens
/// before any swap; the swap writes a staging copy first).
pub fn install(
    plan: &UpdatePlan,
    release: &ReleaseInfo,
    log: &dyn Fn(&str),
) -> Result<Relaunch, String> {
    log(&format!(
        "Downloading {} ({:.1} MB)…",
        plan.asset.name,
        plan.asset.size as f64 / 1_048_576.0
    ));
    let bytes = http_bytes(&plan.asset.url)?;

    log("Verifying checksum…");
    let expected = expected_checksum(release, &plan.asset.name)?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {} (expected {}…, got {}…) — install aborted, current version untouched",
            plan.asset.name,
            &expected[..8.min(expected.len())],
            &actual[..8.min(actual.len())],
        ));
    }

    // Stage the download in a temp dir that lives until extraction is done.
    let staging = tempfile::Builder::new()
        .prefix("kagi-update-")
        .tempdir()
        .map_err(|e| format!("tempdir: {e}"))?;
    let archive = staging.path().join(&plan.asset.name);
    std::fs::write(&archive, &bytes).map_err(|e| format!("write archive: {e}"))?;

    log("Installing…");
    let target = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    install_platform(&archive, staging.path(), &target, log)
}

/// Find the expected SHA-256 for `asset_name` by downloading the release's
/// `SHA256SUMS-*.txt` asset(s) and looking up the line.
fn expected_checksum(release: &ReleaseInfo, asset_name: &str) -> Result<String, String> {
    let sums: Vec<&Asset> = release
        .assets
        .iter()
        .filter(|a| a.name.starts_with("SHA256SUMS"))
        .collect();
    if sums.is_empty() {
        return Err("release has no SHA256SUMS file to verify against".to_string());
    }
    for a in sums {
        if let Ok(text) = http_text(&a.url) {
            if let Some(hex) = update::find_checksum(&text, asset_name) {
                return Ok(hex);
            }
        }
    }
    Err(format!("no checksum found for {asset_name}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

// ── Platform-specific extract + atomic swap ──────────────────

#[cfg(target_os = "linux")]
fn install_platform(
    archive: &Path,
    staging: &Path,
    target: &Path,
    log: &dyn Fn(&str),
) -> Result<Relaunch, String> {
    // Running from an AppImage mounts read-only — we can't swap in place.
    if std::env::var_os("APPIMAGE").is_some() {
        return Err(
            "running as an AppImage — please download the new AppImage manually".to_string(),
        );
    }
    run(std::process::Command::new("tar").args([
        "-xzf",
        archive.to_str().ok_or("non-utf8 archive path")?,
        "-C",
        staging.to_str().ok_or("non-utf8 staging path")?,
    ]))?;
    // The tarball lays out kagi-<v>-<arch>/bin/kagi.
    let new_bin = find_file(staging, "kagi").ok_or("kagi binary not found in tarball")?;
    swap_file(&new_bin, target, log)?;
    Ok(Relaunch {
        program: target.to_string_lossy().into_owned(),
        args: vec![],
    })
}

#[cfg(target_os = "macos")]
fn install_platform(
    archive: &Path,
    staging: &Path,
    target: &Path,
    log: &dyn Fn(&str),
) -> Result<Relaunch, String> {
    // target = …/Kagi.app/Contents/MacOS/kagi  → app root is 3 levels up.
    let app_root = target
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        .ok_or("not running from an installed Kagi.app — download the .dmg manually")?
        .to_path_buf();

    let mnt = staging.join("mnt");
    std::fs::create_dir_all(&mnt).map_err(|e| format!("mkdir mnt: {e}"))?;
    run(std::process::Command::new("hdiutil").args([
        "attach",
        "-nobrowse",
        "-quiet",
        "-mountpoint",
        mnt.to_str().ok_or("non-utf8 mnt")?,
        archive.to_str().ok_or("non-utf8 archive")?,
    ]))?;
    let detach = || {
        let _ = std::process::Command::new("hdiutil")
            .args(["detach", "-quiet", &mnt.to_string_lossy()])
            .status();
    };
    let new_app = mnt.join("Kagi.app");
    if !new_app.exists() {
        detach();
        return Err("Kagi.app not found in the .dmg".to_string());
    }
    // Copy the new app out of the read-only DMG into staging, then swap.
    let staged_app = staging.join("Kagi.app");
    let copy = run(std::process::Command::new("cp").args([
        "-R",
        new_app.to_str().ok_or("non-utf8 new app")?,
        staged_app.to_str().ok_or("non-utf8 staged app")?,
    ]));
    detach();
    copy?;
    swap_dir(&staged_app, &app_root, log)?;
    Ok(Relaunch {
        program: "open".to_string(),
        args: vec![app_root.to_string_lossy().into_owned()],
    })
}

#[cfg(target_os = "windows")]
fn install_platform(
    archive: &Path,
    staging: &Path,
    target: &Path,
    log: &dyn Fn(&str),
) -> Result<Relaunch, String> {
    let out = staging.join("extract");
    std::fs::create_dir_all(&out).map_err(|e| format!("mkdir extract: {e}"))?;
    run(std::process::Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            archive.display(),
            out.display()
        ),
    ]))?;
    let new_exe = find_file(&out, "kagi.exe").ok_or("kagi.exe not found in zip")?;
    // A running .exe can't be overwritten, but it can be renamed aside first.
    let old = target.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).map_err(|e| format!("rename running exe: {e}"))?;
    if let Err(e) = std::fs::copy(&new_exe, target) {
        let _ = std::fs::rename(&old, target); // roll back
        return Err(format!("install new exe: {e}"));
    }
    log("Installed (the previous .exe is cleaned on next launch).");
    Ok(Relaunch {
        program: target.to_string_lossy().into_owned(),
        args: vec![],
    })
}

/// Atomically replace the file at `target` with `new` (copy to a sibling staging
/// path, set +x on unix, then rename over). The running binary's open inode keeps
/// the old code mapped, so this is safe while running on Linux/macOS.
#[allow(dead_code)] // only called from the Linux install path; tested on any unix
fn swap_file(new: &Path, target: &Path, _log: &dyn Fn(&str)) -> Result<(), String> {
    let staged = target.with_extension("new");
    std::fs::copy(new, &staged).map_err(|e| format!("stage new binary: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&staged)
            .map_err(|e| format!("stat staged: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&staged, perms).map_err(|e| format!("chmod staged: {e}"))?;
    }
    std::fs::rename(&staged, target).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("swap into place: {e}")
    })
}

/// Replace the directory at `target` with `new`: move the old aside, move the new
/// in, then best-effort delete the old. Used for the macOS `.app` bundle swap.
#[cfg(target_os = "macos")]
fn swap_dir(new: &Path, target: &Path, _log: &dyn Fn(&str)) -> Result<(), String> {
    let backup = target.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(target, &backup).map_err(|e| format!("move old app aside: {e}"))?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&backup, target); // roll back
        return Err(format!("move new app into place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

/// Recursively find the first file named `name` under `root`.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn find_file(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(unix)]
fn run(cmd: &mut std::process::Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("spawn: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {status}"))
    }
}

#[cfg(windows)]
fn run(cmd: &mut std::process::Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("spawn: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[cfg(unix)]
    #[test]
    fn swap_file_replaces_target_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("kagi");
        let new = dir.path().join("new-kagi");
        std::fs::write(&target, b"OLD").unwrap();
        std::fs::write(&new, b"NEWCONTENT").unwrap();
        swap_file(&new, &target, &|_| {}).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"NEWCONTENT");
        // staging sibling cleaned up
        assert!(!target.with_extension("new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "target is executable");
        }
    }

    #[test]
    fn current_version_honors_override() {
        // (Process-global env; this test just exercises the parse path.)
        std::env::set_var("KAGI_UPDATE_FORCE_CURRENT", "0.0.1");
        assert_eq!(current_version(), Version::parse("0.0.1").unwrap());
        std::env::remove_var("KAGI_UPDATE_FORCE_CURRENT");
    }
}
