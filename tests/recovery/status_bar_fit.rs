//! #1310: long branch identity must not push feedback or panel controls outside the footer.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::{contained, draw};
use crate::toolbar_overflow::ToolbarRestore;
use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, VisualTestAppContext};
use kagi::ui::{e2e, types::FooterStatus, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

fn assert_fit(win: AnyWindowHandle, dimensions: (f32, f32), branch: &str) -> [gpui::Pixels; 3] {
    let bounds = |id| e2e::control_bounds(win.window_id(), id).expect("status element drawn");
    let viewport = Bounds::new(
        point(px(0.), px(0.)),
        size(px(dimensions.0), px(dimensions.1)),
    );
    let footer = bounds("pr-menu-footer");
    contained(viewport, footer, "footer inside window");
    let terminal = bounds("status-icon-terminal");
    let oplog = bounds("status-icon-oplog");
    contained(footer, terminal, "Terminal inside footer");
    contained(footer, oplog, "Operation Log inside footer");
    // The recorder measures the content inside each fixed 2px focus border.
    assert!(
        terminal.right() + px(4.) <= oplog.left(),
        "panel buttons overlap"
    );
    let identity = bounds("status-branch");
    let refresh = bounds("status-refresh-time");
    let feedback = e2e::footer_message_bounds();
    for (name, element) in [("branch", identity), ("refresh", refresh)] {
        contained(footer, element, name);
        assert!(
            element.size.width > px(0.),
            "{name} has no readable width: {element:?}"
        );
    }
    assert!(
        feedback.size.width > px(0.),
        "feedback has no readable width: {feedback:?}"
    );
    assert!(
        identity.right() <= refresh.left(),
        "branch overlaps fixed status chips"
    );
    assert!(
        refresh.right() <= feedback.left(),
        "refresh overlaps feedback"
    );
    assert!(
        feedback.right() <= terminal.left() - px(2.),
        "feedback overlaps Terminal border"
    );
    let (ax, tooltip) = e2e::status_branch_identity().expect("branch metadata");
    assert_eq!(ax.as_ref(), branch, "full branch AX name");
    assert_eq!(tooltip.as_ref(), branch, "full branch tooltip");
    if branch.len() > 1 {
        assert!(identity.size.width >= theme::scaled_px(60.) - px(0.5));
        assert!(
            identity.size.width < theme::scaled_px(600.),
            "long branch must truncate"
        );
    }
    let ahead_behind = bounds("status-ahead-behind");
    let upstream = bounds("status-upstream");
    contained(footer, ahead_behind, "ahead/behind");
    contained(footer, upstream, "upstream");
    assert!(
        identity.right() <= ahead_behind.left(),
        "branch overlaps counts"
    );
    assert!(
        ahead_behind.right() <= upstream.left(),
        "counts overlap upstream"
    );
    assert!(
        upstream.right() <= refresh.left(),
        "upstream overlaps timer"
    );
    [
        ahead_behind.size.width,
        upstream.size.width,
        refresh.size.width,
    ]
}

fn check_window(cx: &mut VisualTestAppContext, repo: &std::path::Path, dimensions: (f32, f32)) {
    let state = e2e::app_state(repo).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win: AnyWindowHandle = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    let long = format!("feature/{}", "x".repeat(112));
    assert_eq!(long.len(), 120);
    let mut fixed_widths = None;
    for branch in ["a", long.as_str()] {
        for busy in [false, true] {
            app.update(cx, |app, cx| {
                let summary = &mut app.view_mut().status_summary;
                summary.branch = branch.into();
                summary.is_dirty = true;
                summary.staged = 2;
                summary.unstaged = 3;
                summary.conflict_count = 1;
                summary.stash_count = 1;
                summary.ahead = Some(12);
                summary.behind = Some(3);
                summary.no_upstream = false;
                summary.upstream_name = "origin/main".into();
                summary.last_refresh_secs = 1_700_000_000;
                let text = format!(
                    "{}\ncomplete operation detail",
                    i18n::Msg::OpInProgress.t().repeat(15)
                );
                app.status_footer = if busy {
                    FooterStatus::Busy(text.into())
                } else {
                    FooterStatus::Failed(text.into())
                };
                cx.notify();
            });
            for _ in 0..3 {
                draw(cx, win, dimensions);
            }
            let widths = assert_fit(win, dimensions, branch);
            assert_eq!(
                *fixed_widths.get_or_insert(widths),
                widths,
                "counts, upstream and timer never shrink"
            );
        }
    }
    unmount(cx, app, win);
}

pub fn scenario_status_bar_fit(cx: &mut VisualTestAppContext) {
    let _restore = ToolbarRestore::capture(cx);
    let fixture = build_fixture();
    for appearance in ["apple-light", "apple-dark"] {
        assert!(theme::set_active(appearance));
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for display in [
                    settings::ToolbarDisplay::IconOnly,
                    settings::ToolbarDisplay::IconAndText,
                ] {
                    settings::set_toolbar_display(display);
                    for dimensions in [(700., 500.), (940., 660.)] {
                        check_window(cx, fixture.path(), dimensions);
                    }
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS status_bar_fit: 120-char branch, fixed chips/timer, Busy/Failed feedback and panel controls fit 700x500/940x660; full tooltip/AX retained in EN/JA, 100%/167%, both toolbar presentations and Apple themes");
}
