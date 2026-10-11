//! #1258/#1261: content access and composer geometry use production virtual lists.
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{paint, pr_at, wait_loaded};
use gpui::{AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, pr_conversation::hunk, pr_mode::PrView};
use kagi_domain::github::ReviewComment;
use kagi_domain::review_thread::{DiffSide, ReviewThread};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};

fn bounds(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    paint(cx, window);
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} must be reachable"))
}
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let at = bounds(cx, window, id);
    cx.simulate_click(window, at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}
fn wheel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    at: Bounds<Pixels>,
    dx: f32,
    dy: f32,
) {
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: at.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(dx), gpui::px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}
fn hunk_access(cx: &mut VisualTestAppContext, window: AnyWindowHandle, source: &str) {
    let disclosure = bounds(cx, window, "pr-convo-hunk-disclosure-7000");
    let first = bounds(cx, window, "pr-convo-hunk-line-7000-0");
    let viewport = bounds(cx, window, "pr-convo-hunk-scroll-7000");
    assert!(
        first.size.width > viewport.size.width,
        "long line must overflow its bounded viewport"
    );
    assert_eq!(
        hunk::attributes(7000),
        Some((gpui::Role::Button, "Show 28 more lines".into(), 12))
    );
    click(cx, window, "pr-convo-hunk-copy-7000");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(source)
    );
    wheel(cx, window, viewport, -400., 0.);
    let shifted = bounds(cx, window, "pr-convo-hunk-line-7000-0");
    assert!(
        shifted.origin.x < first.origin.x - gpui::px(100.),
        "horizontal wheel must reveal the long line"
    );
    assert_eq!(
        shifted.origin.y, first.origin.y,
        "horizontal wheel must not move the feed"
    );
    cx.simulate_click(window, disclosure.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let last = bounds(cx, window, "pr-convo-hunk-line-7000-39");
    assert!(
        last.origin.y > first.origin.y,
        "expanded hunk retains every line in order"
    );
    let expanded = bounds(cx, window, "pr-convo-hunk-7000");
    assert!(expanded.size.height > first.size.height * 40.);
    assert_eq!(
        expanded.size.width,
        bounds(cx, window, "pr-convo-hunk-scroll-7000").size.width + gpui::px(2.)
    );
    let builds = hunk::projection_builds();
    for _ in 0..5 {
        paint(cx, window);
    }
    assert_eq!(
        hunk::projection_builds(),
        builds,
        "warm redraw must reuse the accepted projection"
    );
    i18n::set_lang(Lang::Ja);
    paint(cx, window);
    assert_eq!(
        hunk::attributes(7000),
        Some((gpui::Role::Button, "折りたたむ".into(), 40))
    );
    let viewport = bounds(cx, window, "pr-mode-center-pane");
    wheel(cx, window, viewport, 0., -10000.);
    let last = bounds(cx, window, "pr-convo-hunk-line-7000-39");
    assert!(
        last.bottom() <= viewport.bottom() + gpui::px(1.),
        "last line must be scroll-reachable inside the pane"
    );
    click(cx, window, "pr-convo-hunk-disclosure-7000");
    paint(cx, window);
    assert_eq!(
        hunk::attributes(7000),
        Some((gpui::Role::Button, "残り 28 行を表示".into(), 12))
    );
}

pub fn scenario_review_hunk_full(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let before_lang = i18n::lang();
    i18n::set_lang(Lang::En);
    let _gh = OfflineGh::install();
    let (_fixture, repo, _remote, head) = crate::pr_threads::c_txt_pr();
    let (app, window) = mount(cx, &repo);
    let source = (0..40)
        .map(|i| {
            format!(
                "+line {i:02} {}",
                if i == 0 {
                    "x".repeat(200)
                } else {
                    String::new()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(vec![ReviewThread {
            path: "c.txt".into(),
            line: Some(2),
            original_line: Some(2),
            diff_side: DiffSide::Right,
            comments: vec![ReviewComment {
                author: "alice".into(),
                body: "Review context".into(),
                diff_hunk: source.clone(),
                created_at: "2026-10-12T00:00:00Z".into(),
                ..Default::default()
            }],
            ..Default::default()
        }]),
    )));
    app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&head), cx));
    wait_loaded(cx, &app, &head);
    app.update(cx, |app, cx| app.pr_mode_show(PrView::Review, cx));
    hunk_access(cx, window, &source);
    i18n::set_lang(before_lang);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS review_hunk_full");
}

pub fn scenario_issue_body_zoom(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["ui_zoom"]);
    let before_zoom = theme::zoom();
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_body_for_e2e("one\ntwo\nthree\nfour", window, cx)
        });
    })
    .unwrap();
    let mut counts = Vec::new();
    for zoom in [1., 1.67] {
        theme::set_zoom(zoom);
        let body = bounds(cx, window, "issue-composer-body");
        counts.push(f32::from(body.size.height) / f32::from(theme::scaled_px(25.5)));
    }
    assert!(
        (counts[0] - counts[1]).abs() < 0.05,
        "visible body lines must stay equal across zoom: {counts:?}"
    );
    theme::set_zoom(before_zoom);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS issue_body_zoom");
}
