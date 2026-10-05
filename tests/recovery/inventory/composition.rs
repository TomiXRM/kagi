//! Tier A proof of the standalone before/after PNG composition CLI.
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gpui::VisualTestAppContext;
use image::{ImageFormat, Rgba, RgbaImage};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("inventory-diff.sh")
}

fn run(before: &Path, after: &Path, path: Option<&Path>) -> Output {
    let mut command = Command::new("/bin/bash");
    command.arg(script()).arg(before).arg(after);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().expect("run inventory-diff.sh")
}

fn binary(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("required test binary {name} not found"))
}

fn assert_png(path: &Path, expected: &RgbaImage) {
    let actual = image::open(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .to_rgba8();
    assert_eq!(actual.dimensions(), expected.dimensions());
    assert_eq!(actual, *expected, "pixels differ: {}", path.display());
}

pub fn scenario_inventory_composition(_cx: &mut VisualTestAppContext) {
    let root = tempfile::tempdir().expect("composition tempdir");
    let before = root.path().join("before dir");
    let after = root.path().join("after dir");
    std::fs::create_dir(&before).unwrap();
    std::fs::create_dir(&after).unwrap();

    let name = "modal with spaces-en-dark.png";
    let prior = RgbaImage::from_fn(2, 3, |x, y| Rgba([40 * x as u8, 70 * y as u8, 95, 255]));
    let current = RgbaImage::from_fn(3, 3, |x, y| Rgba([125, 30 * x as u8, 65 * y as u8, 255]));
    prior
        .save_with_format(before.join(name), ImageFormat::Png)
        .unwrap();
    current
        .save_with_format(after.join(name), ImageFormat::Png)
        .unwrap();
    let output = after.join("modal with spaces-en-dark-before-after.png");
    let expected = RgbaImage::from_fn(5, 3, |x, y| {
        if x < 2 {
            *prior.get_pixel(x, y)
        } else {
            *current.get_pixel(x - 2, y)
        }
    });

    // A missing counterpart must fail *before* writing any paired output.
    let orphan = before.join("unpaired-ja-light.png");
    prior.save_with_format(&orphan, ImageFormat::Png).unwrap();
    let failed = run(&before, &after, None);
    assert!(!failed.status.success(), "unpaired input must fail");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("missing after image"));
    assert!(!output.exists(), "failed preflight must not write output");
    std::fs::remove_file(orphan).unwrap();

    let orphan = after.join("unpaired-en-light.png");
    current.save_with_format(&orphan, ImageFormat::Png).unwrap();
    let failed = run(&before, &after, None);
    assert!(!failed.status.success(), "unpaired after image must fail");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("missing before image"));
    assert!(!output.exists());
    std::fs::remove_file(orphan).unwrap();

    // No image tools cannot silently claim a successful composition.
    let empty_path = root.path().join("empty-bin");
    std::fs::create_dir(&empty_path).unwrap();
    let failed = run(&before, &after, Some(&empty_path));
    assert!(!failed.status.success(), "missing image tools must fail");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("合成なし"));
    assert!(!output.exists());

    // Force the sips + stdlib path even on a machine with ImageMagick installed.
    let fallback_path = root.path().join("fallback-bin");
    std::fs::create_dir(&fallback_path).unwrap();
    for tool in ["sips", "python3", "dirname", "mktemp", "mv", "rm"] {
        symlink(binary(tool), fallback_path.join(tool)).unwrap();
    }
    let composed = run(&before, &after, Some(&fallback_path));
    assert!(
        composed.status.success(),
        "fallback failed: {}",
        String::from_utf8_lossy(&composed.stderr)
    );
    assert_png(&output, &expected);

    // A generated comparison must never be reused as a source on a later run.
    let rerun = run(&before, &after, Some(&fallback_path));
    assert!(
        rerun.status.success(),
        "generated image was treated as input: {}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_png(&output, &expected);

    // Exercise the preferred ImageMagick route where it is installed.
    if std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
        .map(|dir| dir.join("magick"))
        .any(|path| path.is_file())
    {
        let composed = run(&before, &after, None);
        assert!(
            composed.status.success(),
            "magick failed: {}",
            String::from_utf8_lossy(&composed.stderr)
        );
        assert_png(&output, &expected);
    }
    eprintln!("[gui-e2e] PASS inventory_composition");
}
