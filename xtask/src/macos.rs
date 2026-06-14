//! macOS `.app` bundling and `.dmg` packaging (ADR-0047).
//!
//! Hand-rolled bundling (no cargo-bundle) + `hdiutil` (no create-dmg). Ad-hoc
//! code signing only — Developer ID / notarization is Phase 2.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const BUNDLE_NAME: &str = "Kagi.app";
const BIN_NAME: &str = "kagi";
const BUNDLE_ID: &str = "com.tomixrm.kagi";
const DISPLAY_NAME: &str = "Kagi";
const MIN_SYSTEM_VERSION: &str = "13.0";
const ICON_FILE: &str = "AppIcon"; // CFBundleIconFile (sans .icns)

/// `target/dist` under the workspace root.
fn dist_dir(root: &Path) -> PathBuf {
    root.join("target").join("dist")
}

/// Build the Info.plist XML for the bundle.
fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{DISPLAY_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{DISPLAY_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>
    <string>{BIN_NAME}</string>
    <key>CFBundleIconFile</key>
    <string>{ICON_FILE}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>{MIN_SYSTEM_VERSION}</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Kagi</string>
</dict>
</plist>
"#
    )
}

/// `bundle-macos`: release build → assemble `target/dist/Kagi.app` → ad-hoc sign → verify.
pub fn bundle(root: &Path) -> Result<(), String> {
    let version = util::kagi_version(root)?;
    println!("bundle-macos: kagi {version} ({})", util::host_arch());

    // 1) Ensure the icon exists (run the pipeline if missing).
    let icns = root.join("assets/icon/AppIcon.icns");
    if !icns.exists() {
        println!("bundle-macos: AppIcon.icns missing, running scripts/make_icon.sh");
        crate::icon::generate(root)?;
    }

    // 2) Release build of the kagi binary.
    println!("bundle-macos: cargo build --release -p kagi");
    util::run(
        Command::new("cargo")
            .current_dir(root)
            .args(["build", "--release", "-p", "kagi"]),
    )?;
    let built_bin = root.join("target/release").join(BIN_NAME);
    if !built_bin.exists() {
        return Err(format!(
            "expected binary not found: {}",
            built_bin.display()
        ));
    }

    // 3) Assemble the bundle skeleton.
    let dist = dist_dir(root);
    let app = dist.join(BUNDLE_NAME);
    util::clean_dir(&app)?;
    let macos_dir = app.join("Contents/MacOS");
    let resources_dir = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir).map_err(|e| format!("mkdir MacOS: {e}"))?;
    std::fs::create_dir_all(&resources_dir).map_err(|e| format!("mkdir Resources: {e}"))?;

    // Contents/MacOS/kagi
    let dest_bin = macos_dir.join(BIN_NAME);
    std::fs::copy(&built_bin, &dest_bin).map_err(|e| format!("copy binary: {e}"))?;
    // Contents/Info.plist
    std::fs::write(app.join("Contents/Info.plist"), info_plist(&version))
        .map_err(|e| format!("write Info.plist: {e}"))?;
    // Contents/Resources/AppIcon.icns
    std::fs::copy(&icns, resources_dir.join("AppIcon.icns"))
        .map_err(|e| format!("copy icns: {e}"))?;
    // PkgInfo (optional but conventional)
    std::fs::write(app.join("Contents/PkgInfo"), "APPL????")
        .map_err(|e| format!("write PkgInfo: {e}"))?;

    // 4) Ad-hoc code sign (deep) then strict verify.
    println!("bundle-macos: codesign --force -s - --deep");
    util::run(Command::new("codesign").args([
        "--force",
        "-s",
        "-",
        "--deep",
        app.to_str().unwrap(),
    ]))?;
    println!("bundle-macos: codesign --verify --strict");
    util::run(Command::new("codesign").args([
        "--verify",
        "--strict",
        "--verbose=2",
        app.to_str().unwrap(),
    ]))?;

    println!("bundle-macos: wrote {}", app.display());
    Ok(())
}

/// `dmg-macos`: `hdiutil`-built DMG containing `Kagi.app` + an `/Applications` symlink.
pub fn dmg(root: &Path) -> Result<(), String> {
    let version = util::kagi_version(root)?;
    let arch = util::host_arch();
    let dist = dist_dir(root);
    let app = dist.join(BUNDLE_NAME);
    if !app.exists() {
        return Err(format!(
            "{} not found — run `bundle-macos` first",
            app.display()
        ));
    }

    // Stage the DMG contents in a temp dir: Kagi.app + /Applications symlink.
    let stage = dist.join("dmg-stage");
    util::clean_dir(&stage)?;
    std::fs::create_dir_all(&stage).map_err(|e| format!("mkdir stage: {e}"))?;
    // Copy the .app into the stage (cp -R to preserve symlinks/perms/signature).
    util::run(Command::new("cp").args(["-R", app.to_str().unwrap(), stage.to_str().unwrap()]))?;
    // /Applications symlink for drag-install UX.
    std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
        .map_err(|e| format!("symlink Applications: {e}"))?;

    let dmg_path = dist.join(format!("Kagi-{version}-{arch}.dmg"));
    if dmg_path.exists() {
        std::fs::remove_file(&dmg_path).map_err(|e| format!("rm old dmg: {e}"))?;
    }

    println!("dmg-macos: hdiutil create {}", dmg_path.display());
    util::run(Command::new("hdiutil").args([
        "create",
        "-volname",
        DISPLAY_NAME,
        "-srcfolder",
        stage.to_str().unwrap(),
        "-ov",
        "-format",
        "UDZO",
        dmg_path.to_str().unwrap(),
    ]))?;

    util::clean_dir(&stage)?;
    println!("dmg-macos: wrote {}", dmg_path.display());
    Ok(())
}
