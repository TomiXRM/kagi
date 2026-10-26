//! Persistent conflict feedback reserves space, including its overflow summary.
use super::*;

pub(super) fn run(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let old_zoom = theme::zoom();
    let old_lang = i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    theme::set_zoom(1.);
    for preset in ["apple-light", "apple-dark"] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            for layout in ["text", "raw", "directory-file", "saved"] {
                run_case(cx, layout, lang, preset);
            }
        }
    }
    theme::set_zoom(old_zoom);
    theme::set_active(&old_theme);
    i18n::set_lang(old_lang);
}

fn fixture(layout: &str) -> tempfile::TempDir {
    match layout {
        "raw" => crate::recovery_diff_presentation::conflict_image_fixture(),
        "directory-file" => crate::app_conflict::dir_file_fixture(),
        "saved" => {
            let fixture = crate::app_conflict::content_fixture();
            std::fs::write(fixture.path().join("file.txt"), "resolved\n").unwrap();
            git(fixture.path(), &["add", "file.txt"]);
            fixture
        }
        "text" => crate::app_conflict::content_fixture(),
        _ => unreachable!(),
    }
}

fn run_case(cx: &mut VisualTestAppContext, layout: &str, lang: i18n::Lang, preset: &str) {
    let fixture = fixture(layout);
    let index = std::fs::read(fixture.path().join(".git/index")).unwrap();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        theme::set_active(preset);
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        if layout == "saved" {
            view.conflict_open_saved(0, cx);
        } else {
            view.conflict_select_file(0);
        }
        cx.notify();
    });
    cx.run_until_parked();
    for dimensions in [(1440., 900.), (940., 660.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        exercise_lane(cx, &app, window, layout);
    }
    assert_eq!(
        std::fs::read(fixture.path().join(".git/index")).unwrap(),
        index
    );
    drop(conflict);
    unmount(cx, app, window);
}

fn controls(layout: &str) -> &'static [&'static str] {
    match layout {
        "text" => &[
            "conflict-hunk-0-incoming-use",
            "conflict-hunk-0-incoming-both",
        ],
        "raw" => &["conflict-raw-choices"],
        "directory-file" => &["conflict-directory-file-choices"],
        "saved" => &["conflict-saved-preview"],
        _ => unreachable!(),
    }
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} must be actually painted"))
}

fn control_bounds(window: AnyWindowHandle, layout: &str) -> Vec<Bounds<Pixels>> {
    controls(layout)
        .iter()
        .map(|id| {
            let bounds = bounds(window, id);
            assert!(bounds.size.height > px(0.), "{id} remains visible");
            bounds
        })
        .collect()
}

fn assert_safe_lane(window: AnyWindowHandle, layout: &str) -> Bounds<Pixels> {
    let lane = bounds(window, "conflict-notification-lane");
    let stack = bounds(window, "toast-persistent-stack");
    assert!(stack.top() >= lane.top() && stack.bottom() <= lane.bottom());
    assert!(stack.size.width <= lane.size.width * 0.45 + px(1.));
    let headers: &[&str] = match layout {
        "text" => &["conflict-source-headers"],
        "raw" => &[
            "conflict-image-current-header",
            "conflict-image-incoming-header",
        ],
        "saved" => &["conflict-saved-header"],
        "directory-file" => &["conflict-content-header"],
        _ => unreachable!(),
    };
    for id in headers {
        assert!(
            bounds(window, id).bottom() <= lane.top(),
            "{id} is never covered"
        );
    }
    for id in controls(layout) {
        let control = bounds(window, id);
        assert!(
            stack.bottom() <= control.top() && lane.bottom() <= control.top(),
            "{layout}: persistent cards AND summary must not intersect {id}"
        );
    }
    lane
}

fn exercise_lane(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    layout: &str,
) {
    draw(cx, window);
    let original = control_bounds(window, layout);
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let errors: Vec<_> = stack.update(cx, |stack, cx| {
        (0..3)
            .map(|index| {
                stack.push_notify(
                    kagi::ui::ToastKind::Error,
                    format!("Persistent {layout} lane fixture {index}"),
                    cx,
                );
                stack.toasts().last().unwrap().id
            })
            .collect()
    });
    cx.run_until_parked();
    draw(cx, window);
    let lane = assert_safe_lane(window, layout);
    let shifted = control_bounds(window, layout);
    for (before, after) in original.iter().zip(&shifted) {
        assert!(
            after.top() > before.top(),
            "persistent feedback reserves real height"
        );
    }
    for kind in [kagi::ui::ToastKind::Info, kagi::ui::ToastKind::Success] {
        let transient = stack.update(cx, |stack, cx| {
            stack.push_notify(kind, "Unrelated transient fixture", cx);
            stack.toasts().last().unwrap().id
        });
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(assert_safe_lane(window, layout), lane);
        assert_eq!(control_bounds(window, layout), shifted);
        stack.update(cx, |stack, cx| stack.begin_exit(transient, cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(assert_safe_lane(window, layout), lane);
        assert_eq!(control_bounds(window, layout), shifted);
    }
    stack.update(cx, |stack, cx| {
        for error in errors {
            stack.begin_exit(error, cx);
        }
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(
        control_bounds(window, layout),
        original,
        "dismissal leaves no empty lane"
    );
}
