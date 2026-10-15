//! #1617: native modal disclosure presentation, with the shared clock frozen.
//! Bounds come from the actual clipping box and its full-size content, not a
//! transition scalar. Existing modal_sections remains the instant-policy oracle.

use std::time::{Duration, Instant};

use gpui::{px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    dialog_a11y, e2e,
    modals::{AmendPlanModal, DiscardModal},
    worktree_wip::WriteOrigin,
    KagiApp,
};
use kagi_ui_core::{motion, theme};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

fn frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, at: Instant) {
    // First-mount/resize measurement converges at the SAME frozen instant.
    frame_once(cx, win, id, at);
    frame_once(cx, win, id, at);
}

fn frame_once(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, at: Instant) {
    motion::set_clock_for_e2e(Some(at));
    dialog_a11y::clear_recorded_a11y();
    if let Some(body_id) = live_body_id(id) {
        e2e::clear_control_bounds(win.window_id(), body_id);
    }
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

fn live_body_id(id: &str) -> Option<&'static str> {
    match id {
        "amend-recovery" => Some("amend-recovery-body"),
        "plan-equivalent-command" => Some("plan-equivalent-command-body"),
        "discard-skipped" => Some("discard-skipped-list"),
        _ => None,
    }
}

fn assert_body_semantics(win: AnyWindowHandle, id: &str, open: bool) {
    let note_id = match id {
        "amend-warnings" => Some("amend-warning-0"),
        "discard-warnings" => Some("discard-warning-0"),
        _ => None,
    };
    if let Some(note_id) = note_id {
        let note = dialog_a11y::recorded_note(note_id);
        assert_eq!(note.is_some(), open, "{id}: only open bodies emit Notes");
        if let Some((role, label)) = note {
            assert_eq!(role, gpui::Role::Note);
            assert!(!label.is_empty());
        }
    }
    if let Some(body_id) = live_body_id(id) {
        assert_eq!(
            e2e::control_bounds(win.window_id(), body_id).is_some(),
            open,
            "{id}: only open bodies mount the identified input container"
        );
    }
}

fn wheel(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, delta: f32, at: Instant) {
    let clip = e2e::control_bounds(win.window_id(), &format!("{id}-reveal-clip")).unwrap();
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: clip.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    frame(cx, win, id, at);
}

pub fn scenario_modal_disclosure_motion(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
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
    let mut plan = kagi_git::Backend::open(fixture.path())
        .unwrap()
        .plan_amend(kagi_git::AmendMode::Both, Some("motion disclosure"))
        .unwrap();
    assert!(plan.blockers.is_empty());
    assert!(
        !plan.warnings.is_empty(),
        "secret-named staged path supplies supporting warnings"
    );
    assert!(plan.recovery.is_some());
    // Stress the real recovery renderer with enough reference text to scroll.
    // These remain copies of the backend's genuine advice; none is executed.
    let recovery = plan.recovery.as_mut().unwrap();
    let commands = recovery.commands.clone();
    assert!(!commands.is_empty());
    recovery.commands = commands.into_iter().cycle().take(24).collect();
    let modal = AmendPlanModal {
        plan: std::sync::Arc::new(plan),
        error: None,
        mode: kagi_git::AmendMode::Both,
        message: "motion disclosure".into(),
        confirm_armed: false,
    };
    let skipped: Vec<_> = (0..40)
        .map(|index| format!("skipped-{index:02}.txt"))
        .collect();
    for path in skipped
        .iter()
        .map(String::as_str)
        .chain(["discard-target.txt"])
    {
        std::fs::write(fixture.path().join(path), "untracked fixture\n").unwrap();
    }
    let paths = vec!["discard-target.txt".to_owned()];
    let discard_plan = kagi_git::Backend::open(fixture.path())
        .unwrap()
        .plan_discard(&paths)
        .unwrap();
    assert!(discard_plan.blockers.is_empty());
    assert!(!discard_plan.warnings.is_empty());
    let discard = DiscardModal {
        plan: std::sync::Arc::new(discard_plan),
        paths,
        kinds: Default::default(),
        skipped,
        is_all: false,
        origin: WriteOrigin::CommitPanel,
        error: None,
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
        ("discard-warnings", true),
        ("discard-skipped", false),
    ] {
        if id == "discard-warnings" {
            app.update(cx, |app, cx| {
                app.set_discard_modal(discard.clone());
                cx.notify();
            });
        }
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
        assert_body_semantics(win, id, true);
        let full = height(win, id, "reveal-content").expect("full natural body is drawn");
        assert!(full > 0.);
        assert!((height(win, id, "reveal-clip").unwrap() - full).abs() <= 1.);
        if id != "plan-equivalent-command" {
            let target = e2e::control_bounds(win.window_id(), "modal-target-list")
                .expect("destructive target stays visible");
            assert!(target.size.height > px(0.));
        }
        let scroll = cx.read(|cx| app.read(cx).modal_section_overrides.scroll_for_e2e(id));
        if matches!(id, "amend-recovery" | "discard-skipped") {
            let scroll = scroll.as_ref().unwrap();
            assert!(scroll.max_offset().y > px(0.), "{id}: real overflow");
            wheel(cx, win, id, -80., at);
            assert!(scroll.offset().y < px(0.), "{id}: open body accepts wheel");
        }

        toggle(cx, &app, win, id, false);
        frame(cx, win, id, at);
        assert_body_semantics(win, id, false);
        if let Some(scroll) = &scroll {
            let before_wheel = scroll.offset();
            wheel(cx, win, id, 60., at);
            assert_eq!(
                scroll.offset(),
                before_wheel,
                "{id}: retained closing drawing must not handle wheel input"
            );
        }
        if matches!(id, "amend-recovery" | "plan-equivalent-command") {
            assert!(
                !e2e::recorded_plan_command_disclosure(id).unwrap().2,
                "AX expanded input changes before presentation"
            );
        }
        let mid_at = at + Duration::from_millis(100);
        frame(cx, win, id, mid_at);
        let mid = height(win, id, "reveal-clip").expect("closing body retained");
        assert_body_semantics(win, id, false);
        assert!(
            mid > 0. && mid < full,
            "{id}: actual closing clipping {mid}/{full}"
        );
        assert!(
            (height(win, id, "reveal-content").unwrap() - full).abs() <= 1.,
            "natural content must not shrink"
        );
        if let Some(scroll) = &scroll {
            let before_wheel = scroll.offset();
            wheel(cx, win, id, 60., mid_at);
            assert_eq!(
                scroll.offset(),
                before_wheel,
                "{id}: partially clipped closing body is also input-inert"
            );
            assert_body_semantics(win, id, false);
        }
        toggle(cx, &app, win, id, true);
        frame(cx, win, id, mid_at);
        assert_body_semantics(win, id, true);
        assert!(
            (height(win, id, "reveal-clip").unwrap() - mid).abs() <= 1.,
            "{id}: no reversal jump"
        );
        at = mid_at + Duration::from_millis(200);
        frame(cx, win, id, at);
        assert!((height(win, id, "reveal-clip").unwrap() - full).abs() <= 1.);
        if matches!(id, "amend-recovery" | "discard-skipped") {
            let scroll = scroll.as_ref().unwrap();
            let before_wheel = scroll.offset();
            wheel(cx, win, id, 40., at);
            assert!(
                scroll.offset().y > before_wheel.y,
                "{id}: reversal restores live wheel input"
            );
        }
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
