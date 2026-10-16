//! #1417/#1418: system display opt-ins cannot be disabled by app settings.
use crate::macos::{build_fixture, unmount};
use e2e::system_accessibility::{self as probe, SystemDisplayInjection};
use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme, KagiApp};
use std::{cell::RefCell, rc::Rc, time::Instant};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    e2e::clear_control_bounds(win.window_id(), "bottom-panel-clip");
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn static_motion(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    assert!(
        theme::reduce_motion(),
        "system OR app resolves effective motion"
    );
    assert_eq!(
        probe::loading_animation_count(),
        0,
        "system Reduce Motion must skip animation elements even when app preference is false"
    );
    assert_eq!(
        probe::spinner_animation_count(),
        0,
        "production sync spinner is static"
    );
    // A frozen clock removes the runner's default instant-motion override.
    kagi_ui_core::motion::set_clock_for_e2e(Some(Instant::now()));
    draw(cx, win);
    let full = e2e::control_bounds(win.window_id(), "bottom-panel-clip")
        .unwrap()
        .size
        .height;
    cx.simulate_keystrokes(win, "cmd-j");
    draw(cx, win);
    assert!(
        e2e::control_bounds(win.window_id(), "bottom-panel-clip").is_none(),
        "system/app reduction closes the real pane on its first frame"
    );
    cx.simulate_keystrokes(win, "cmd-j");
    draw(cx, win);
    assert_eq!(
        e2e::control_bounds(win.window_id(), "bottom-panel-clip")
            .unwrap()
            .size
            .height,
        full
    );
    kagi_ui_core::motion::set_clock_for_e2e(None);
}

fn motion_leg(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    system: &SystemDisplayInjection,
) {
    static_motion(cx, win);
    cx.update(|cx| system.update(false, false, cx));
    assert_eq!(probe::loading_animation_count(), 3);
    assert_eq!(probe::spinner_animation_count(), 1);
    theme::set_reduce_motion(true);
    static_motion(cx, win);
    cx.update(|cx| system.update(true, false, cx));
    theme::set_reduce_motion(false);
    static_motion(cx, win);
}

fn transparency_leg(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    system: &SystemDisplayInjection,
) {
    let fill = probe::dashboard_card_fill();
    assert_eq!(
        fill.a, 1.,
        "system Reduce Transparency must select an opaque PR card token"
    );
    let expected = theme::pr_card_opaque_bg();
    assert_eq!(
        expected, 0x0a0a0c,
        "opaque equivalent of 35% black over Apple Dark base"
    );
    let contrast = |ink| {
        let ink = crate::settings_quiet::luminance(ink);
        let bg = crate::settings_quiet::luminance(expected);
        (ink.max(bg) + 0.05) / (ink.min(bg) + 0.05)
    };
    assert!(contrast(theme::theme().text_main) >= 4.5);
    assert!(contrast(theme::theme().text_muted) >= 4.5);
    let rect = e2e::control_bounds(win.window_id(), "pr-attention-tile-0")
        .expect("actual production tile");
    cx.update_window(win, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native PR tile Metal pixels");
        let scale = window.scale_factor();
        // Padding centre-left is away from the border and attention dot/text.
        let pixel = image
            .get_pixel(
                ((f32::from(rect.left()) + 4.) * scale) as u32,
                (f32::from(rect.center().y) * scale) as u32,
            )
            .0;
        for (channel, shift) in pixel[..3].iter().zip([16, 8, 0]) {
            assert!(
                channel.abs_diff((expected >> shift) as u8) <= 2,
                "opaque native tile {pixel:?}, token #{expected:06x}"
            );
        }
    })
    .unwrap();
    cx.update(|cx| system.update(false, false, cx));
    draw(cx, win);
    assert_eq!(
        probe::dashboard_card_fill().a,
        0.35,
        "system-off restores normal translucent style"
    );
    cx.update(|cx| system.update(false, true, cx));
    draw(cx, win);
}

fn mount(
    cx: &mut VisualTestAppContext,
    dimensions: (f32, f32),
    motion: bool,
) -> (tempfile::TempDir, Entity<KagiApp>, AnyWindowHandle) {
    let fixture = build_fixture();
    let state = e2e::app_state(fixture.path()).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow_mut().take().unwrap();
    app.update(cx, |app, cx| app.set_theme("apple-dark", cx));
    if !motion {
        let pr = crate::evidence_support::pull_request(1, "Readable / 読める PR", "feature/a11y");
        e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
            vec![pr],
            "",
            None,
        ))));
        app.update(cx, |app, cx| app.refresh_github_prs(cx));
        cx.run_until_parked();
        app.update(cx, |app, cx| app.show_pr_mode(cx));
        cx.run_until_parked();
    }
    (fixture, app, win)
}

fn matrix(cx: &mut VisualTestAppContext, motion: bool) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom", "reduce_motion"]);
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let previous_app = theme::app_reduce_motion();
    theme::set_reduce_motion(false);
    let system = SystemDisplayInjection::new(motion, !motion);
    for dimensions in [(940., 660.), (1392., 883.)] {
        let (_fixture, app, win) = mount(cx, dimensions, motion);
        for lang in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", lang);
            i18n::init_lang();
            for zoom in [1., 1.667] {
                theme::set_zoom(zoom);
                draw(cx, win);
                if motion {
                    motion_leg(cx, win, &system);
                } else {
                    transparency_leg(cx, win, &system);
                }
                eprintln!("[gui-e2e] accessibility {dimensions:?} {lang} zoom={zoom}: PASS");
            }
        }
        unmount(cx, app, win);
    }
    theme::set_reduce_motion(previous_app);
}

pub fn scenario_system_reduce_motion(cx: &mut VisualTestAppContext) {
    matrix(cx, true);
}
pub fn scenario_system_reduce_transparency(cx: &mut VisualTestAppContext) {
    matrix(cx, false);
}
