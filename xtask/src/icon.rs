//! `icon` subcommand: regenerate the app icons via `scripts/make_icon.sh`.

use std::path::Path;
use std::process::Command;

use crate::util;

/// Run the icon pipeline (idempotent). macOS-only (uses swift/sips/iconutil).
pub fn generate(root: &Path) -> Result<(), String> {
    let script = root.join("scripts/make_icon.sh");
    if !script.exists() {
        return Err(format!("{} not found", script.display()));
    }
    if !cfg!(target_os = "macos") {
        return Err("`xtask icon` requires macOS (swift/sips/iconutil); \
                    commit the generated assets/icon/ from a macOS host"
            .into());
    }
    println!("icon: running {}", script.display());
    util::run(Command::new("bash").arg(&script).current_dir(root))?;
    Ok(())
}
