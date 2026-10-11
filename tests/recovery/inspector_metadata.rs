//! #1237: metadata retains complete identities and hash keyboard activation.
use crate::macos::{git, mount, unmount};
use gpui::{AnyWindowHandle, ClipboardItem, Role, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme};

const NAME: &str = "Alexandria Catherine Montgomery Wellington Forty Characters";

fn fixture() -> (tempfile::TempDir, String) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), "metadata\n").unwrap();
    git(repo, &["add", "."]);
    let status = std::process::Command::new("git")
        .args(["commit", "-q", "-m", "Metadata accessibility"])
        .env("GIT_AUTHOR_NAME", NAME)
        .env("GIT_AUTHOR_EMAIL", "long@example.test")
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success());
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    let hash = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    (fixture, hash)
}

fn metadata(cx: &mut VisualTestAppContext, window: AnyWindowHandle, hash: &str, zoom: f32) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let metadata = e2e::inspector_metadata::last().expect("rendered metadata");
    assert_eq!(
        metadata.author.as_ref(),
        NAME,
        "full author must reach layout, not a character-count cut"
    );
    assert_eq!(
        metadata.identity.as_deref(),
        Some(format!("{NAME} <long@example.test>").as_str())
    );
    assert_eq!(metadata.hash_role, Some(Role::Button));
    assert_eq!(
        metadata.hash_name.as_deref(),
        Some(i18n::inspector_copy_hash(&hash[..8]).as_str())
    );
    let chip = e2e::control_bounds(window.window_id(), "inspector-hash-chip").unwrap();
    assert!(
        (chip.size.height - theme::scaled_px(18.)).abs() <= gpui::px(1.),
        "dense hash height: {chip:?}"
    );
    let author = e2e::control_bounds(window.window_id(), "inspector-author").unwrap();
    assert!(
        (author.size.height - gpui::px(20.5 * zoom)).abs() <= gpui::px(1.),
        "unchanged single-line author height: {author:?}"
    );
    assert!(
        author.size.width < theme::scaled_px(300.),
        "long name is constrained for layout ellipsis"
    );
    cx.simulate_mouse_move(window, author.center(), None, gpui::Modifiers::none());
    for _ in 0..3 {
        cx.advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
    }
    assert_eq!(
        e2e::inspector_metadata::last().unwrap().tooltip.as_deref(),
        metadata.identity.as_deref(),
        "hover exposes full identity"
    );
}

fn keyboard_copy(cx: &mut VisualTestAppContext, window: AnyWindowHandle, hash: &str) {
    // Move away and back with real Tab, then press/release native click keys.
    cx.update_window(window, |_, window, cx| {
        window.focus(&e2e::inspector_metadata::focus_handle(), cx);
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "shift-tab tab");
    for (key, poison) in [("enter", "before-copy"), ("space", "before-space")] {
        cx.write_to_clipboard(ClipboardItem::new_string(poison.into()));
        crate::keyboard_nav::keys(cx, window, key);
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(hash),
            "{key} copies raw full SHA"
        );
    }
}

pub fn scenario_inspector_metadata_a11y(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let (fixture, hash) = fixture();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            let (app, window) = mount(cx, fixture.path());
            app.update(cx, |app, cx| {
                app.select(0);
                cx.notify();
            });
            cx.run_until_parked();
            metadata(cx, window, &hash, zoom);
            keyboard_copy(cx, window, &hash);
            e2e::inspector_metadata::clear();
            unmount(cx, app, window);
        }
    }
}
