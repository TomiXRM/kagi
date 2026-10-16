//! #1633: approval details must remain reachable at the specified scale.
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::plan_note::{PlanDisposition, PlanNote, ResetNote};
use kagi_git::CommitId;
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, mount, open_offscreen, repo_fingerprint, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn contains(parent: Bounds<Pixels>, child: Bounds<Pixels>) -> bool {
    child.size.width > px(0.)
        && child.size.height > px(0.)
        && child.left() >= parent.left() - px(1.)
        && child.right() <= parent.right() + px(1.)
        && child.top() >= parent.top() - px(1.)
        && child.bottom() <= parent.bottom() + px(1.)
}

const NOTES: usize = 200;

fn mount_narrow(cx: &mut VisualTestAppContext, repo: &Path) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let out = captured.clone();
    let window = open_offscreen(cx, size(px(940.), px(660.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &out)
    });
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window.into())
}

fn note_frame(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    prefix: &str,
) -> Bounds<Pixels> {
    for index in 0..NOTES {
        e2e::clear_control_bounds(window.window_id(), &format!("{prefix}-{index}"));
    }
    let viewport = cx
        .update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
            Bounds::new(point(px(0.), px(0.)), window.viewport_size())
        })
        .unwrap();
    let card = e2e::control_bounds(window.window_id(), "modal-card").expect("card");
    assert!(
        contains(viewport, card),
        "card outside viewport: {card:?} / {viewport:?}"
    );
    for id in ["plan-cancel", "plan-confirm"] {
        let button = e2e::control_bounds(window.window_id(), id).expect("pinned action");
        assert!(
            contains(card, button),
            "{id} outside card: {button:?} / {card:?}"
        );
    }
    card
}

fn traverse_notes(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    blocked: bool,
    label: &str,
) {
    let (prefix, panel) = if blocked {
        ("plan-blocker", "plan-blockers")
    } else {
        ("plan-warning", "plan-warnings")
    };
    let mut seen = BTreeSet::new();
    for _ in 0..NOTES * 4 {
        let card = note_frame(cx, window, prefix);
        let mut row_height = None::<Pixels>;
        for index in 0..NOTES {
            if let Some(paint) =
                e2e::control_paint(window.window_id(), &format!("{prefix}-{index}"))
            {
                row_height = Some(row_height.map_or(paint.bounds.size.height, |h| {
                    h.min(paint.bounds.size.height)
                }));
                if contains(paint.mask, paint.bounds) && contains(card, paint.bounds) {
                    seen.insert(index);
                }
            }
        }
        if seen.len() == NOTES {
            break;
        }
        let viewport = e2e::control_bounds(window.window_id(), panel).expect("note viewport");
        let step = (viewport.size.height / 2.).min(row_height.expect("painted note") / 2.);
        assert!(step > px(0.), "{label}: notes need positive scroll space");
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), -step)),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
    }
    assert_eq!(
        seen,
        (0..NOTES).collect(),
        "{label}: every note, including the last, must become fully painted inside the clip mask"
    );
}

fn notes_case(cx: &mut VisualTestAppContext, slug: &str, zoom: f32, blocked: bool) {
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    let target = CommitId(git_fixture::git_output(
        fixture.path(),
        &["rev-parse", "HEAD~1"],
    ));
    let original_theme = theme::theme().slug.to_string();
    let (app, window) = mount_narrow(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_theme(slug, cx);
        app.open_reset_current_modal(target, cx);
    });
    cx.run_until_parked();
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        let mut modal = app.reset_current_modal().expect("real reset plan").clone();
        assert!(modal.plan.blockers.is_empty());
        // Explicit renderer stress payloads, never a backend-generated count.
        let plan = Arc::make_mut(&mut modal.plan);
        if blocked {
            plan.warnings.clear();
            plan.blockers = vec![PlanNote::Reset(ResetNote::DetachedHead); NOTES];
            plan.disposition = PlanDisposition::Blocked;
        } else {
            plan.warnings =
                vec![plan.warnings.first().expect("reset safety warning").clone(); NOTES];
        }
        app.set_reset_current_modal(modal);
        cx.notify();
    });
    let label = format!("{slug}/{}/{zoom}/blocked={blocked}", i18n::lang().slug());
    traverse_notes(cx, window, blocked, &label);
    let confirm = e2e::control_bounds(window.window_id(), "plan-confirm").unwrap();
    cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).reset_current_modal().unwrap().confirm_armed),
        !blocked,
        "{label}: blocked stays inert; ready only arms"
    );
    note_frame(
        cx,
        window,
        if blocked {
            "plan-blocker"
        } else {
            "plan-warning"
        },
    );
    app.update(cx, |app, cx| {
        app.cancel_reset_current_modal();
        app.set_theme(&original_theme, cx);
    });
    unmount(cx, app, window);
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "{label}: no Git write"
    );
    eprintln!("[gui-e2e] PASS modal_scale_notes {label}: all 200 notes reached, actions pinned, no execution");
}

pub fn scenario_modal_scale_notes(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    for slug in ["apple-light", "apple-dark"] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                for blocked in [false, true] {
                    notes_case(cx, slug, zoom, blocked);
                }
            }
        }
    }
}

pub fn scenario_modal_scale_targets(cx: &mut VisualTestAppContext) {
    const TARGETS: usize = 500;
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let fixture = build_fixture();
    std::fs::create_dir(fixture.path().join("scale-targets")).unwrap();
    let paths: Vec<_> = (0..TARGETS)
        .map(|index| format!("scale-targets/file-{index:03}.txt"))
        .collect();
    for (index, path) in paths.iter().enumerate() {
        std::fs::write(fixture.path().join(path), format!("target {index}\n")).unwrap();
    }
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, fixture.path().to_path_buf(), cx)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_discard_all_modal(app.active_session().unwrap(), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).discard_modal().unwrap().paths.len(), TARGETS));
    let mut seen = BTreeSet::new();
    for _ in 0..200 {
        for path in &paths {
            e2e::clear_control_bounds(window.window_id(), &format!("modal-file-{path}"));
        }
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        let card = e2e::control_bounds(window.window_id(), "modal-card").unwrap();
        let list = e2e::control_bounds(window.window_id(), "modal-target-list").unwrap();
        for id in ["discard-cancel", "discard-confirm"] {
            assert!(contains(
                card,
                e2e::control_bounds(window.window_id(), id).unwrap()
            ));
        }
        for (index, path) in paths.iter().enumerate() {
            if let Some(paint) =
                e2e::control_paint(window.window_id(), &format!("modal-file-{path}"))
            {
                if contains(list, paint.bounds) && contains(paint.mask, paint.bounds) {
                    seen.insert(index);
                }
            }
        }
        if seen.len() == TARGETS {
            break;
        }
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: list.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), -list.size.height / 2.)),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
    }
    assert_eq!(
        seen,
        (0..TARGETS).collect(),
        "all 500 actual target rows, including file-499.txt, must be reachable"
    );
    app.update(cx, |app, _| app.cancel_discard_modal());
    unmount(cx, app, window);
    assert_eq!(repo_fingerprint(fixture.path()), before);
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(fixture.path().join(path)).unwrap(),
            format!("target {index}\n")
        );
    }
    eprintln!("[gui-e2e] PASS modal_scale_targets: all 500 target rows reached, no execution");
}
