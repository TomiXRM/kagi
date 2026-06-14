//! xtask — kagi build/bundle helper (ADR-0047, W20-RELEASE).
//!
//! Stdlib-only. Subcommands:
//!   icon          regenerate assets/icon/ via scripts/make_icon.sh (macOS)
//!   bundle-macos  release build → target/dist/Kagi.app (ad-hoc signed)
//!   dmg-macos     hdiutil DMG (Kagi.app + /Applications) → target/dist/Kagi-<v>-<arch>.dmg
//!   bundle-linux  tar.gz layout (bin + .desktop + 512px icon) → target/dist/
//!   bundle-appimage  Kagi.AppDir → Kagi-<arch>.AppImage (appimagetool) + zip
//!
//! Run via: `cargo run -p xtask -- <subcommand>`

mod appimage;
mod icon;
mod linux;
mod macos;
mod util;
mod windows;

use std::process::ExitCode;

fn usage() -> &'static str {
    "\
usage: cargo run -p xtask -- <subcommand>

subcommands:
  icon                     regenerate app icons (scripts/make_icon.sh; macOS only)
  bundle-macos             release build + assemble & ad-hoc-sign Kagi.app
  dmg-macos                build the distributable DMG (run bundle-macos first)
  bundle-linux [--bin P]   assemble the Linux tar.gz layout (--bin overrides the binary)
  bundle-windows [--bin P]  zip kagi.exe (+ LICENSE) → kagi-<v>-x86_64-windows.zip (Windows)
  bundle-appimage --bin P [--arch x86_64|aarch64]
                           assemble Kagi.AppDir, run appimagetool when present, and zip
"
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = util::workspace_root();
    match args.first().map(String::as_str) {
        Some("icon") => icon::generate(&root),
        Some("bundle-macos") => macos::bundle(&root),
        Some("dmg-macos") => macos::dmg(&root),
        Some("bundle-linux") => {
            // optional `--bin <path>` override
            let mut override_bin = None;
            let mut it = args.iter().skip(1);
            while let Some(a) = it.next() {
                if a == "--bin" {
                    override_bin = it.next().map(String::as_str);
                } else {
                    return Err(format!("unknown argument: {a}\n\n{}", usage()));
                }
            }
            linux::bundle(&root, override_bin)
        }
        Some("bundle-windows") => {
            // optional `--bin <path>` override
            let mut override_bin = None;
            let mut it = args.iter().skip(1);
            while let Some(a) = it.next() {
                if a == "--bin" {
                    override_bin = it.next().map(String::as_str);
                } else {
                    return Err(format!("unknown argument: {a}\n\n{}", usage()));
                }
            }
            windows::bundle(&root, override_bin)
        }
        Some("bundle-appimage") => {
            let mut override_bin = None;
            let mut arch = util::host_arch_appimage().to_string();
            let mut it = args.iter().skip(1);
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--bin" => {
                        override_bin = it.next().map(String::as_str);
                    }
                    "--arch" => {
                        arch = it
                            .next()
                            .cloned()
                            .ok_or_else(|| format!("--arch needs a value\n\n{}", usage()))?;
                    }
                    other => return Err(format!("unknown argument: {other}\n\n{}", usage())),
                }
            }
            appimage::bundle(&root, override_bin, &arch)
        }
        Some("-h") | Some("--help") | Some("help") => {
            print!("{}", usage());
            Ok(())
        }
        Some(other) => Err(format!("unknown subcommand: {other}\n\n{}", usage())),
        None => Err(format!("missing subcommand\n\n{}", usage())),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::util;

    #[test]
    fn parses_kagi_version() {
        let root = util::workspace_root();
        let v = util::kagi_version(&root).expect("version parses");
        // semver-ish: at least one dot, all parts present.
        assert!(v.split('.').count() >= 2, "unexpected version: {v}");
        assert!(!v.is_empty());
    }

    #[test]
    fn host_arch_is_known() {
        let a = util::host_arch();
        assert!(a == "arm64" || a == "x86_64");
    }

    #[test]
    fn workspace_root_has_root_manifest() {
        let root = util::workspace_root();
        assert!(root.join("Cargo.toml").exists());
        assert!(root.join("xtask").join("Cargo.toml").exists());
    }
}
