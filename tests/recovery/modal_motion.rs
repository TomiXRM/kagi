//! #1617: native modal disclosure presentation, with the shared clock frozen.
//! Bounds come from the actual clipping box and its full-size content, not a
//! transition scalar. Existing modal_sections remains the instant-policy oracle.

use std::time::{Duration, Instant};

use gpui::{px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, modals::AmendPlanModal, KagiApp};
use kagi_ui_core::{motion, theme};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

fn frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, at: Instant) {
    // First-mount/resize measurement converges at the SAME frozen instant.
    frame_once(cx, win, id, at);
    frame_once(cx, win, id, at);
}

fn frame_once(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, at: Instant) {
    motion::set_clock_for_e2e(Some(at));
    for suffix in ["reveal-clip", "reveal-content"] {
        e2e::clear_control_bounds(win.window_id(), &format!("{id}-{suffix}"));
    }
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn height(win: AnyWindowHandle, id: &str, suffix: &str) -> Option<f32> {
    e2e::control_bounds(win.window_id(), &format!("{id}-{suffix}"))
        .map(|bounds| f32::from(bounds.size.height))
}

fn toggle(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    id: &'static str,
    open: bool,
) {
    let header = e2e::control_bounds(win.window_id(), id).expect("actual modal disclosure header");
    cx.simulate_click(win, header.center(), gpui::Modifiers::none());
    assert_eq!(
        cx.read(|cx| app.read(cx).modal_section_overrides.get(id).copied()),
        Some(open),
        "intent is immediate"
    );
}

pub fn scenario_modal_disclosure_motion(cx: &mut VisualTestAppContext) {
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "motion-delete", "HEAD~1"]);
    std::fs::write(fixture.path().join(".env"), "TOKEN=fixture-only\n").unwrap();
    std::fs::write(
        fixture.path().join("README.md"),
        "# fixture\nready to amend\n",
    )
    .unwrap();
    git(fixture.path(), &["add", ".env", "README.md"]);
    let plan = kagi_git::Backend::open(fixture.path())
        .unwrap()
        .plan_amend(kagi_git::AmendMode::Both, Some("motion disclosure"))
        .unwrap();
    assert!(plan.blockers.is_empty());
    assert!(
        !plan.warnings.is_empty(),
        "secret-named staged path supplies supporting warnings"
    );
    assert!(plan.recovery.is_some());
    let modal = AmendPlanModal {
        plan: std::sync::Arc::new(plan),
        error: None,
        mode: kagi_git::AmendMode::Both,
        message: "motion disclosure".into(),
        confirm_armed: false,
    };
    let before = repo_fingerprint(fixture.path());
    let (app, win) = mount(cx, fixture.path());
    let mut at = Instant::now();
    motion::set_clock_for_e2e(Some(at));
    app.update(cx, |app, cx| {
        app.set_amend_modal(modal.clone());
        cx.notify();
    });

    for (id, initially_open) in [
        ("amend-warnings", true),
        ("amend-recovery", false),
        ("plan-equivalent-command", false),
    ] {
        if id == "plan-equivalent-command" {
            app.update(cx, |app, cx| {
                app.clear_amend_modal();
                app.open_delete_branch_modal("motion-delete", cx);
            });
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_some()));
        }
        frame(cx, win, id, at);
        if !initially_open {
            assert!(height(win, id, "reveal-content").is_none());
            toggle(cx, &app, win, id, true);
            frame(cx, win, id, at);
            frame(cx, win, id, at + Duration::from_millis(60));
            let opening = height(win, id, "reveal-clip").unwrap();
            let natural = height(win, id, "reveal-content").unwrap();
            assert!(
                opening > 0. && opening < natural,
                "{id}: actual opening clipping {opening}/{natural}"
            );
            at += Duration::from_millis(200);
            frame(cx, win, id, at);
        }
        let full = height(win, id, "reveal-content").expect("full natural body is drawn");
        assert!(full > 0.);
        assert!((height(win, id, "reveal-clip").unwrap() - full).abs() <= 1.);
        if id != "plan-equivalent-command" {
            let target = e2e::control_bounds(win.window_id(), "modal-file-README.md")
                .expect("destructive target stays visible");
            assert!(target.size.height > px(0.));
        }

        toggle(cx, &app, win, id, false);
        frame(cx, win, id, at);
        if id != "amend-warnings" {
            assert!(
                !e2e::recorded_plan_command_disclosure(id).unwrap().2,
                "AX expanded input changes before presentation"
            );
        }
        let mid_at = at + Duration::from_millis(100);
        frame(cx, win, id, mid_at);
        let mid = height(win, id, "reveal-clip").expect("closing body retained");
        assert!(
            mid > 0. && mid < full,
            "{id}: actual closing clipping {mid}/{full}"
        );
        assert!(
            (height(win, id, "reveal-content").unwrap() - full).abs() <= 1.,
            "natural content must not shrink"
        );
        toggle(cx, &app, win, id, true);
        frame(cx, win, id, mid_at);
        assert!(
            (height(win, id, "reveal-clip").unwrap() - mid).abs() <= 1.,
            "{id}: no reversal jump"
        );
        at = mid_at + Duration::from_millis(200);
        frame(cx, win, id, at);
        assert!((height(win, id, "reveal-clip").unwrap() - full).abs() <= 1.);
        toggle(cx, &app, win, id, false);
        frame(cx, win, id, at);
        at += Duration::from_millis(200);
        frame(cx, win, id, at);
        assert!(
            height(win, id, "reveal-content").is_none(),
            "settled close releases body"
        );

        theme::set_reduce_motion(true);
        toggle(cx, &app, win, id, true);
        frame_once(cx, win, id, at);
        assert!(
            (height(win, id, "reveal-clip").unwrap() - height(win, id, "reveal-content").unwrap())
                .abs()
                <= 1.,
            "reduced opening is immediate"
        );
        toggle(cx, &app, win, id, false);
        frame_once(cx, win, id, at);
        assert!(
            height(win, id, "reveal-content").is_none(),
            "reduced close is immediate"
        );
        theme::set_reduce_motion(false);
        at += Duration::from_millis(400);
    }

    // Replace a modal while a command is closing, without an intervening draw.
    let id = "amend-recovery";
    app.update(cx, |app, cx| {
        app.set_amend_modal(modal.clone());
        cx.notify();
    });
    frame(cx, win, id, at);
    toggle(cx, &app, win, id, true);
    frame(cx, win, id, at);
    frame(cx, win, id, at + Duration::from_millis(200));
    toggle(cx, &app, win, id, false);
    frame(cx, win, id, at + Duration::from_millis(200));
    app.update(cx, |app, cx| {
        app.set_amend_modal(modal);
        cx.notify();
    });
    frame(cx, win, id, at + Duration::from_millis(220));
    assert!(
        height(win, id, "reveal-content").is_none(),
        "new modal cannot inherit a retained closing body"
    );
    // Even a never-opened command measures and opens fully on the first reduced
    // frame; retaining an old natural height must not be required.
    theme::set_reduce_motion(true);
    toggle(cx, &app, win, id, true);
    frame_once(cx, win, id, at + Duration::from_millis(220));
    let natural = height(win, id, "reveal-content").unwrap();
    assert!(natural > 0.);
    assert!((height(win, id, "reveal-clip").unwrap() - natural).abs() <= 1.);
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "disclosure never executes the plan"
    );
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS modal_disclosure_motion");
}
