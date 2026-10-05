use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{AnyWindowHandle, AppContext, VisualTestAppContext};
use kagi::ui::i18n::Lang;

use super::{Action, Entry, Fixture};

fn values(key: &str, default: &str, allowed: &[&str]) -> Result<Vec<String>, String> {
    let raw = std::env::var(key).unwrap_or_else(|_| default.into());
    let mut result = Vec::new();
    for value in raw.split(',').map(str::trim) {
        if !allowed.contains(&value) {
            return Err(format!(
                "{key}: invalid value {value:?}; expected {}",
                allowed.join(",")
            ));
        }
        if !result.iter().any(|item| item == value) {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

pub(super) fn prepare(parent: bool) -> Result<(), String> {
    values("KAGI_INVENTORY_LANG", "en", &["en", "ja"])?;
    values("KAGI_INVENTORY_THEME", "dark", &["dark", "light"])?;
    if parent {
        let out = std::env::var_os("KAGI_INVENTORY_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos();
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/inventory")
                    .join(stamp.to_string())
            });
        fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
        let out = out.canonicalize().map_err(|error| error.to_string())?;
        // Refuse to mix evidence from different invocations, including stale PNGs.
        if out.join("index.md").exists() {
            return Err(format!(
                "{} already contains an inventory; choose a fresh output directory",
                out.display()
            ));
        }
        let header = "# Native UI inventory\n\n| Name | Opening / fixture | Result | File |\n|---|---|---|---|\n";
        fs::write(out.join("index.md"), header).map_err(|error| error.to_string())?;
        std::env::set_var("KAGI_INVENTORY_OUT", &out);
        eprintln!("[inventory] output {}", out.display());
    }
    // This opt-in affects only inventory runs, never the default hidden runner.
    std::env::set_var("KAGI_GUI_E2E_ONSCREEN", "1");
    std::env::set_var("KAGI_GUI_E2E_VISIBLE", "1");
    Ok(())
}

fn out_dir() -> PathBuf {
    std::env::var_os("KAGI_INVENTORY_OUT")
        .map(PathBuf::from)
        .expect("inventory parent output directory")
}

fn record(entry: &Entry, result: &str, file: &str) {
    let description = entry.description.replace('|', "\\|").replace('\n', " ");
    let result = result.replace('|', "\\|").replace('\n', " ");
    let mut index = OpenOptions::new()
        .append(true)
        .open(out_dir().join("index.md"))
        .expect("inventory index");
    writeln!(
        index,
        "| {} | {description} | {result} | {file} |",
        entry.name
    )
    .expect("append inventory result");
}

pub(super) fn run(entry: &Entry, cx: &mut VisualTestAppContext) {
    if let Err(panic) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_entry(entry, cx)))
    {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("setup, opening or capture failed");
        record(entry, &format!("failed: {message}"), "—");
        std::panic::resume_unwind(panic);
    }
}

fn run_entry(entry: &Entry, cx: &mut VisualTestAppContext) {
    let Action::Capture(open) = entry.action else {
        if let Action::Skip(reason) = entry.action {
            record(entry, &format!("skip: {reason}"), "—");
            eprintln!("[inventory] SKIP {}: {reason}", entry.name);
        }
        return;
    };
    let languages =
        values("KAGI_INVENTORY_LANG", "en", &["en", "ja"]).expect("validated languages");
    let themes =
        values("KAGI_INVENTORY_THEME", "dark", &["dark", "light"]).expect("validated themes");
    for language in &languages {
        for theme in &themes {
            let _keys = crate::gui_isolation::SavedKeys::keep(&["lang", "theme"]);
            let _ports = crate::gui_isolation::PortStore::keep();
            let fixture = Fixture::build(entry.fixture);
            let (app, window) = crate::macos::mount(cx, &fixture.repo);
            app.update(cx, |app, cx| {
                app.set_lang(if language == "ja" { Lang::Ja } else { Lang::En }, cx);
                app.set_theme(
                    if theme == "light" {
                        "apple-light"
                    } else {
                        "apple-dark"
                    },
                    cx,
                );
            });
            open(&fixture, &app, window, cx);
            app.update(cx, |_, cx| cx.notify());
            let name = entry
                .name
                .strip_prefix("inventory:")
                .expect("inventory scenario prefix");
            let filename = format!("{name}-{language}-{theme}.png");
            assert_eq!(
                kagi::ui::i18n::lang(),
                if language == "ja" { Lang::Ja } else { Lang::En },
                "capture language"
            );
            assert_eq!(
                kagi::ui::theme::theme().slug.as_ref(),
                if theme == "light" {
                    "apple-light"
                } else {
                    "apple-dark"
                },
                "capture theme"
            );
            let result = shot(cx, window, &out_dir().join(&filename));
            fixture.finish(cx);
            // TextElement queues strong InputState handles in on_next_frame.
            // Retire the input tree, then let native frames drain that queue
            // while its display link still exists (unmount cancels the link).
            cx.update_window(window, |_, window, cx| {
                window.replace_root(cx, |window, cx| {
                    let empty = cx.new(|_| gpui::Empty);
                    gpui_component::Root::new(empty, window, cx)
                });
            })
            .expect("retire captured root");
            for _ in 0..30 {
                crate::gui_evidence::present_windows();
                crate::macos::drain_native_events();
                cx.run_until_parked();
            }
            crate::macos::unmount(cx, app, window);
            match result {
                Ok(()) => {
                    record(entry, "captured", &format!("[{filename}]({filename})"));
                    eprintln!("[inventory] CAPTURE {filename}");
                }
                Err(error) => {
                    panic!("{}: {error}", entry.name);
                }
            }
        }
    }
}

fn shot(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    path: &std::path::Path,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .map_err(|error| error.to_string())?;
    cx.run_until_parked();
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .map_err(|error| error.to_string())?;
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    let windows: Vec<_> = crate::gui_evidence::app_windows()
        .into_iter()
        .filter(|(id, visible)| *id > 0 && *visible)
        .collect();
    if windows.len() != 1 {
        return Err(format!(
            "expected one visible owned window, found {}",
            windows.len()
        ));
    }
    let output = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", &format!("-l{}", windows[0].0)])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "screencapture {}: {} (check Screen Recording permission)",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    image::open(path).map_err(|error| format!("invalid captured PNG: {error}"))?;
    Ok(())
}
