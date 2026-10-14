//! Complete Graph subjects and independently scrollable Inspector messages.
use crate::macos::{git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{context_menu::CommitAction, e2e, i18n, list_a11y, theme, KagiApp};
use std::time::{Duration, Instant};

fn fixture(messages: &[String]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    for message in messages.iter().rev() {
        git(
            dir.path(),
            &[
                "commit",
                "-q",
                "--allow-empty",
                "--cleanup=verbatim",
                "-m",
                message,
            ],
        );
    }
    dir
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn resize(cx: &mut VisualTestAppContext, window: AnyWindowHandle, dimensions: (f32, f32)) {
    let wanted = gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1));
    cx.update_window(window, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(window, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(Instant::now() < deadline, "native window resized");
    }
    for _ in 0..3 {
        paint(cx, window);
    }
}

fn subjects() -> Vec<String> {
    let mut subjects: Vec<_> = [71, 72, 73, 300]
        .into_iter()
        .map(|n| "x".repeat(n))
        .collect();
    subjects.extend([
        format!("{}末尾一", "共通😀".repeat(40)),
        format!("{}末尾二", "共通😀".repeat(40)),
    ]);
    subjects.push(format!("{} safe\x1b[31m end", "件名😀".repeat(40)));
    subjects
}

fn graph_names(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &[String],
) {
    list_a11y::clear_recorded_lists();
    paint(cx, window);
    let rows = list_a11y::recorded_list("commit-list").unwrap();
    cx.read(|cx| {
        for (i, subject) in expected.iter().enumerate() {
            let row = &app.read(cx).view().rows[i];
            let safe = kagi_domain::text_safety::sanitize_control_bytes(subject);
            assert_eq!(
                row.summary.as_ref(),
                safe,
                "subject must survive model construction at row {i}"
            );
            let refs: Vec<_> = row
                .badges
                .iter()
                .map(|badge| badge.label.as_ref())
                .collect();
            let age = kagi_ui_core::time::relative_time_description(
                row.author_secs,
                kagi_ui_core::time::now_unix_secs(),
            );
            let label = list_a11y::commit_row_label(&safe, &row.author, &age, &row.short_id, &refs);
            assert_eq!(
                rows.rows.get(&i).map(|r| &r.0),
                Some(&label),
                "AX contains complete subject at row {i}"
            );
        }
    });
    assert_eq!(
        rows.rows.len(),
        expected.len(),
        "long subjects do not change visible row count"
    );
    let row = e2e::control_bounds(window.window_id(), "graph-commit-row").unwrap();
    assert!(
        (f32::from(row.size.height) - theme::scaled(29.)).abs() <= 1.,
        "29px density contract"
    );
    let subject = e2e::control_bounds(window.window_id(), "graph-commit-subject").unwrap();
    let author = e2e::control_bounds(window.window_id(), "graph-commit-author").unwrap();
    let time = e2e::control_bounds(window.window_id(), "graph-commit-time").unwrap();
    assert!(
        subject.right() <= author.left() + gpui::px(1.),
        "subject cannot displace author"
    );
    assert!(
        author.right() <= time.left() + gpui::px(1.),
        "author cannot displace time"
    );
    graph_tooltips(cx, window, expected, row, subject);
}

fn graph_tooltips(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    expected: &[String],
    row: gpui::Bounds<gpui::Pixels>,
    subject: gpui::Bounds<gpui::Pixels>,
) {
    for (i, expected) in expected.iter().enumerate() {
        // Flush dismissal before entering another row; tooltip entities are
        // retained between renders while the pointer remains inside a target.
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(1.), gpui::px(1.)),
            None,
            gpui::Modifiers::none(),
        );
        cx.advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        paint(cx, window);
        e2e::text_caps::clear();
        let at = gpui::point(
            row.left() + gpui::px(2.),
            row.top() + row.size.height * (i as f32 + 0.5),
        );
        cx.simulate_mouse_move(window, at, None, gpui::Modifiers::none());
        for _ in 0..3 {
            cx.advance_clock(Duration::from_millis(400));
            cx.run_until_parked();
            paint(cx, window);
        }
        assert_eq!(
            e2e::text_caps::tooltip().as_deref(),
            Some(kagi_domain::text_safety::sanitize_control_bytes(expected).as_str()),
            "actual hover exposes complete subject {i}: target={subject:?}, row={row:?}, at={at:?}"
        );
        let tooltip = e2e::control_bounds(window.window_id(), "graph-subject-tooltip")
            .expect("actual tooltip painted");
        let viewport = cx
            .update_window(window, |_, window, _| window.viewport_size())
            .unwrap();
        assert!(
            tooltip.left() >= gpui::px(0.)
                && tooltip.top() >= gpui::px(0.)
                && tooltip.right() <= viewport.width + gpui::px(1.)
                && tooltip.bottom() <= viewport.height + gpui::px(1.),
            "full subject tooltip fits native viewport: {tooltip:?}, viewport={viewport:?}"
        );
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(1.), gpui::px(1.)),
            None,
            gpui::Modifiers::none(),
        );
    }
}

pub fn scenario_graph_subject_identity(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let expected = subjects();
    let fixture = fixture(&expected);
    let (app, window) = mount(cx, fixture.path());
    // Keep all fixture rows visible even at the narrow, high-zoom viewport.
    app.update(cx, |app, cx| {
        app.bottom_panel_open = false;
        cx.notify();
    });
    for slug in ["apple-light", "apple-dark"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for dimensions in [(940., 660.), (1392., 883.)] {
                    resize(cx, window, dimensions);
                    graph_names(cx, &app, window, &expected);
                }
            }
        }
    }
    e2e::text_caps::clear();
    unmount(cx, app, window);
}

fn messages() -> Vec<String> {
    let trailers = "\n\nCo-authored-by: Alice <alice@example.test>\nReviewed-by: Bob <bob@example.test>\nSigned-off-by: Carol <carol@example.test>";
    let mut messages: Vec<_> = [1999, 2000, 2001, 10000]
        .into_iter()
        .map(|n| {
            format!(
                "Subject\n\n{}{trailers}",
                "x".repeat(n - 9 - trailers.len())
            )
        })
        .collect();
    messages.push(format!(
        "Long message\n\n{}{trailers}",
        (0..85)
            .map(|n| format!("Paragraph {n}: 日本語😀 body with safe\x07 controls.\n\n"))
            .collect::<String>()
    ));
    messages
}

fn assert_message(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    original: &str,
    row: usize,
) {
    app.update(cx, |app, cx| {
        app.select(row);
        cx.notify();
    });
    cx.run_until_parked();
    paint(cx, window);
    let expected = kagi_domain::text_safety::sanitize_control_bytes(original.trim_end());
    let rendered = e2e::text_caps::message();
    assert_eq!(
        rendered.source.as_ref(),
        expected,
        "full message must survive model construction"
    );
    assert_eq!(
        rendered.html.as_ref(),
        kagi_domain::message::message_to_html(&kagi_domain::message::reflow_message(&expected)),
        "selectable TextView gets complete escaped message"
    );
    assert_eq!(
        rendered.coauthors,
        kagi_domain::trailers::parse_coauthors(&expected).len()
    );
    assert_eq!(
        rendered.trailers,
        kagi_domain::trailers::parse_trailers(&expected).len()
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let target = app.view().rows[row].id.clone();
            app.dispatch_commit_action(CommitAction::CopyMessage, target, window, cx);
        });
    })
    .unwrap();
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(expected)
    );
    scroll_message_tail(cx, app, window);
}

fn scroll_message_tail(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let scroll = e2e::control_bounds(window.window_id(), "inspector-message-frame").unwrap();
    let files = e2e::control_bounds(window.window_id(), "inspector-files-frame").unwrap();
    assert!(
        scroll.size.height > gpui::px(30.),
        "message viewport retains a usable independent height"
    );
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: scroll.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    paint(cx, window);
    let handle = cx.read(|cx| app.read(cx).ui().inspector_message_scroll.clone());
    let maximum = handle.max_offset();
    assert!(
        maximum.y > gpui::px(0.),
        "long message has scrollable content"
    );
    assert!(
        (-handle.offset().y - maximum.y).abs() <= gpui::px(1.),
        "native wheel reaches actual message tail"
    );
    let after = e2e::control_bounds(window.window_id(), "inspector-message-frame").unwrap();
    assert_eq!(
        scroll, after,
        "content scroll cannot push files or footer away"
    );
    assert_eq!(
        files,
        e2e::control_bounds(window.window_id(), "inspector-files-frame").unwrap(),
        "changed-files frame is independent of message scroll"
    );
    assert_trailer_edges(cx, window, scroll);
    let before = KagiApp::inspector_derivations_for_e2e();
    for _ in 0..3 {
        paint(cx, window);
    }
    assert_eq!(
        KagiApp::inspector_derivations_for_e2e(),
        before,
        "warm draws reuse prepared full message and metadata"
    );
}

fn assert_trailer_edges(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    scroll: gpui::Bounds<gpui::Pixels>,
) {
    let tail = e2e::control_bounds(window.window_id(), "inspector-trailer-tail").unwrap();
    assert!(
        tail.bottom() <= scroll.bottom() + gpui::px(1.) && tail.bottom() > scroll.top(),
        "last trailer's end is visibly reachable: {tail:?} in {scroll:?}"
    );
    if tail.size.height <= scroll.size.height {
        assert!(
            tail.top() >= scroll.top() - gpui::px(1.),
            "last complete trailer is visibly reachable: {tail:?} in {scroll:?}"
        );
    } else {
        // At the fitted 160px Inspector width a complete value may wrap to
        // more than one viewport. Reach both edges, never truncate the model
        // or demand impossible simultaneous containment of an oversized row.
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: scroll.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    gpui::px(0.),
                    scroll.top() - tail.top() + gpui::px(1.),
                )),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
        paint(cx, window);
        let top = e2e::control_bounds(window.window_id(), "inspector-trailer-tail").unwrap();
        assert!(
            top.top() >= scroll.top() - gpui::px(1.) && top.top() < scroll.bottom(),
            "oversized trailer's beginning is visibly reachable: {top:?} in {scroll:?}"
        );
    }
}

pub fn scenario_inspector_full_message(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let messages = messages();
    let fixture = fixture(&messages);
    let (app, window) = mount(cx, fixture.path());
    for slug in ["apple-light", "apple-dark"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                // Keep the independent desktop message/files frames mounted at the breakpoint.
                for dimensions in [(1000., 720.), (1392., 883.)] {
                    resize(cx, window, dimensions);
                    for (row, original) in messages.iter().enumerate() {
                        assert_message(cx, &app, window, original, row);
                    }
                }
            }
        }
    }
    e2e::text_caps::clear();
    unmount(cx, app, window);
}

pub fn scenario_text_caps_warm_render(cx: &mut VisualTestAppContext) {
    let Some(repo) = std::env::var_os("KAGI_TEXT_CAPS_STRESS_REPO") else {
        eprintln!("[gui-e2e] SKIP text_caps_warm_render: set KAGI_TEXT_CAPS_STRESS_REPO to the read-only stress fixture");
        return;
    };
    let (app, window) = mount(cx, std::path::Path::new(&repo));
    assert_eq!(
        cx.read(|cx| app.read(cx).view().branches.len()),
        121,
        "use the assigned 121-branch stress fixture"
    );
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    for _ in 0..20 {
        paint(cx, window);
    }
    let mut samples = Vec::with_capacity(31);
    for _ in 0..31 {
        let start = Instant::now();
        for _ in 0..10 {
            paint(cx, window);
        }
        samples.push(start.elapsed().as_secs_f64() / 10.);
    }
    samples.sort_by(f64::total_cmp);
    eprintln!("[gui-e2e] text_caps warm render median_ms={:.6} samples=31 frames_per_sample=10 fixture={}", samples[15] * 1000., std::path::Path::new(&repo).display());
    unmount(cx, app, window);
}
