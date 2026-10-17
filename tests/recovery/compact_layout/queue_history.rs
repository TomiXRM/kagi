//! #1650: operation history must not consume the workspace or clip its tail.
use super::*;
use gpui::MouseButton;
use std::collections::BTreeSet;

pub fn scenario_queue_history_viewport(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    for locale in ["en", "ja"] {
        for preset in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                history_case(
                    cx,
                    Case {
                        height: 660.,
                        zoom,
                        locale,
                        preset,
                    },
                );
            }
        }
    }
}

fn history_case(cx: &mut VisualTestAppContext, case: Case) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for n in 0..32 {
        git(&repo, &["branch", &format!("history-{n:02}")]);
    }
    let before = index(&repo);
    let head_before = git_text(&repo, &["rev-parse", "HEAD"]);
    let (app, win) = mount(cx, &repo, case);
    let mut cancelled = Vec::new();
    for batch in 0..2 {
        git(&repo, &["branch", "anchor"]);
        let (hold, release) = crate::evidence_support::deferred::<()>(cx);
        KagiApp::hold_next_run_for_e2e(hold);
        app.update(cx, |app, cx| app.dblclick_checkout_branch("anchor", cx));
        cx.run_until_parked();
        for n in batch * 16..(batch + 1) * 16 {
            app.update(cx, |app, cx| {
                app.dblclick_checkout_branch(&format!("history-{n:02}"), cx)
            });
        }
        cx.run_until_parked();
        cancelled.extend(cx.read(|cx| app.read(cx).queue_ids_for_e2e()));
        git(&repo, &["branch", "-D", "anchor"]);
        release.send(());
        wait_idle(cx, &app, win);
    }
    let shown = cx
        .read(|cx| app.read(cx).queue_strip_for_e2e(std::time::Instant::now()))
        .unwrap();
    assert_eq!(shown.0, 0);
    assert_eq!(
        shown.2.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        (0..32)
            .rev()
            .map(|n| format!("checkout history-{n:02}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(git_text(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        index(&repo),
        before,
        "failed predecessors execute no successors"
    );
    history_geometry(win, case);
    visit_all_cancelled(cx, win, &cancelled, case);
    scrollbar_remains_usable(cx, win, &cancelled);
    pending_rows_remain_removable(cx, &app, win, &repo, case);
    click(cx, win, "queue-strip-clear");
    e2e::clear_control_bounds(win.window_id(), "queue-strip");
    settle(cx, win);
    assert!(cx.read(|cx| app
        .read(cx)
        .queue_strip_for_e2e(std::time::Instant::now())
        .is_none()));
    assert!(e2e::control_bounds(win.window_id(), "queue-strip").is_none());
    assert_eq!(git_text(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(index(&repo), before);
    unmount(cx, app, win);
}

fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    for _ in 0..80 {
        cx.advance_clock(Duration::from_millis(250));
        settle(cx, win);
        if !cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
            return;
        }
    }
    panic!("held predecessor did not settle");
}

fn history_geometry(win: AnyWindowHandle, case: Case) {
    let outer = bounds(win, "queue-strip");
    contained(viewport(case.dimensions()), outer, "bounded queue/history");
    assert!(
        outer.size.height <= px(case.height * 0.35),
        "secondary history must leave workspace usable"
    );
    let graph = bounds(win, "compact-graph");
    assert!(
        graph.size.height >= theme::scaled_px(80.),
        "{case:?}: Graph remains usable: graph={graph:?}, history={outer:?}"
    );
    contained(
        viewport(case.dimensions()),
        bounds(win, "pr-menu-footer"),
        "status bar stays visible",
    );
    contained(
        outer,
        bounds(win, "queue-strip-clear"),
        "Clear stays reachable",
    );
}

fn visit_all_cancelled(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    ids: &[u64],
    case: Case,
) {
    let expected: BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(expected.len(), 32);
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        for id in ids {
            e2e::clear_control_bounds(win.window_id(), &format!("queue-strip-cancelled-{id}"));
        }
        frame(cx, win);
        let area = bounds(win, "queue-strip-scroll");
        for id in ids {
            if e2e::control_bounds(win.window_id(), &format!("queue-strip-cancelled-{id}"))
                .is_some_and(|row| row.top() >= area.top() && row.bottom() <= area.bottom())
            {
                seen.insert(*id);
            }
        }
        history_geometry(win, case);
        if seen == expected {
            break;
        }
        wheel(cx, win, area.center(), -f32::from(area.size.height) * 0.5);
    }
    assert_eq!(
        seen, expected,
        "wheel reaches every actual cancellation, including the oldest"
    );
}

fn scrollbar_remains_usable(cx: &mut VisualTestAppContext, win: AnyWindowHandle, ids: &[u64]) {
    cx.update(|cx| {
        gpui_component::Theme::global_mut(cx).scrollbar_show =
            gpui_component::scroll::ScrollbarShow::Always;
    });
    frame(cx, win);
    let area = bounds(win, "queue-strip-scroll");
    wheel(cx, win, area.center(), -100_000.);
    let oldest = format!("queue-strip-cancelled-{}", ids[0]);
    let newest = format!("queue-strip-cancelled-{}", ids.last().unwrap());
    contained(area, bounds(win, &oldest), "wheel starts at the real tail");

    // The pinned kit's minimum thumb is 48px, as in surface_scrollbars::drag.
    // Start after a deep scroll: a bar attached to translated rows is now gone.
    let from = point(area.right() - px(8.), area.bottom() - px(24.));
    let to = point(from.x, area.top() + px(1.));
    cx.simulate_mouse_move(win, from, None, Modifiers::none());
    frame(cx, win);
    cx.simulate_mouse_down(win, from, MouseButton::Left, Modifiers::none());
    frame(cx, win);
    for n in 1..=5 {
        cx.simulate_mouse_move(
            win,
            from + (to - from) * (n as f32 / 5.),
            MouseButton::Left,
            Modifiers::none(),
        );
        frame(cx, win);
    }
    cx.simulate_mouse_up(win, to, MouseButton::Left, Modifiers::none());
    e2e::clear_control_bounds(win.window_id(), &newest);
    frame(cx, win);
    contained(
        area,
        bounds(win, &newest),
        "thumb drag from a deep offset reveals newest cancellation",
    );

    let lower_track = point(from.x, area.bottom() - px(2.));
    cx.simulate_mouse_down(win, lower_track, MouseButton::Left, Modifiers::none());
    frame(cx, win);
    cx.simulate_mouse_up(win, lower_track, MouseButton::Left, Modifiers::none());
    e2e::clear_control_bounds(win.window_id(), &oldest);
    frame(cx, win);
    contained(
        area,
        bounds(win, &oldest),
        "lower track remains clickable after dragging to the top",
    );
}

fn pending_rows_remain_removable(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
    case: Case,
) {
    git(repo, &["branch", "anchor"]);
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("anchor", cx));
    cx.run_until_parked();
    for n in 0..16 {
        app.update(cx, |app, cx| {
            app.dblclick_checkout_branch(&format!("history-{n:02}"), cx)
        });
    }
    cx.run_until_parked();
    let ids = cx.read(|cx| app.read(cx).queue_ids_for_e2e());
    assert_eq!(ids.len(), 16);
    let target = *ids.last().unwrap();
    let name = format!("queue-strip-remove-{target}");
    settle(cx, win);
    let area = bounds(win, "queue-strip-scroll");
    wheel(cx, win, area.center(), 100_000.);
    let mut reached = false;
    for _ in 0..32 {
        let area = bounds(win, "queue-strip-scroll");
        e2e::clear_control_bounds(win.window_id(), &name);
        frame(cx, win);
        if e2e::control_bounds(win.window_id(), &name)
            .is_some_and(|button| button.top() >= area.top() && button.bottom() <= area.bottom())
        {
            reached = true;
            break;
        }
        wheel(cx, win, area.center(), -f32::from(area.size.height) * 0.5);
    }
    assert!(
        reached,
        "last queued Remove is reachable beside 32 cancellations"
    );
    click(cx, win, &name);
    assert_eq!(cx.read(|cx| app.read(cx).queue_ids_for_e2e()), ids[..15]);
    history_geometry(win, case);
    contained(
        bounds(win, "queue-strip"),
        bounds(win, "queue-strip-cancel-all"),
        "Cancel all stays pinned",
    );
    click(cx, win, "queue-strip-cancel-all");
    assert!(cx.read(|cx| app.read(cx).queue_ids_for_e2e().is_empty()));
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "Cancel all never cancels running write"
    );
    git(repo, &["branch", "-D", "anchor"]);
    release.send(());
    wait_idle(cx, app, win);
}

fn git_text(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}
