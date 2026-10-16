//! #1633: structured note paths share the note/body scroll owner.
//! The plans below are explicitly typed renderer stress payloads, not claims
//! that a backend generated 200 restore-collision/checkout-overlap paths.
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::plan::OperationPlan;
use kagi_domain::plan_note::{
    checkout::CheckoutNote, PlanDisposition, PlanNote, PullNote, ResetNote,
};
use kagi_domain::status::{ChangeKind, FileStatus};
use kagi_git::CommitId;
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, open_offscreen, repo_fingerprint, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

const PATHS: usize = 200;

fn contains(parent: Bounds<Pixels>, child: Bounds<Pixels>) -> bool {
    child.size.width > px(0.)
        && child.size.height > px(0.)
        && child.left() >= parent.left() - px(1.)
        && child.right() <= parent.right() + px(1.)
        && child.top() >= parent.top() - px(1.)
        && child.bottom() <= parent.bottom() + px(1.)
}

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

fn stress_notes(plan: &mut OperationPlan, paths: &[String], blocked: bool) {
    // Before/after siblings prove the path list neither traps the wheel nor
    // consumes the neighboring mandatory notes. These are renderer-boundary
    // inputs; their combination is deliberately not a backend semantic plan.
    let adjacent = PlanNote::Reset(ResetNote::DetachedHead);
    let structured = if blocked {
        PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
            count: paths.len(),
            files: paths.to_vec(),
        })
    } else {
        PlanNote::Pull(PullNote::RestoreConflictPossible {
            paths: paths.to_vec(),
        })
    };
    plan.warnings.clear();
    plan.blockers.clear();
    plan.preview_commits.clear();
    let notes = vec![adjacent.clone(), structured, adjacent];
    if blocked {
        plan.blockers = notes;
        plan.disposition = PlanDisposition::Blocked;
    } else {
        plan.warnings = notes;
        plan.disposition = PlanDisposition::Ready;
    }
}

fn traverse(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    paths: &[String],
    blocked: bool,
    body_scrolls: bool,
    label: &str,
) {
    let prefix = if blocked {
        "plan-blocker"
    } else {
        "plan-warning"
    };
    let owner = if body_scrolls {
        "modal-body"
    } else if blocked {
        "plan-blockers"
    } else {
        "plan-warnings"
    };
    let path_ids: Vec<_> = paths
        .iter()
        .map(|path| format!("plan-note-path-{path}"))
        .collect();
    let note_ids: Vec<_> = (0..3).map(|index| format!("{prefix}-{index}")).collect();
    let mut seen_paths = BTreeSet::new();
    let mut seen_notes = BTreeSet::new();
    let mut pinned = None;
    for _ in 0..PATHS * 2 {
        // Bounds/paint records are observations of this draw, never stale rows
        // retained from a preceding scroll offset (or another window).
        for id in path_ids.iter().chain(&note_ids) {
            e2e::clear_control_bounds(window.window_id(), id);
        }
        for id in ["modal-card", owner, "plan-cancel", "plan-confirm"] {
            e2e::clear_control_bounds(window.window_id(), id);
        }
        let viewport = cx
            .update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
                Bounds::new(point(px(0.), px(0.)), window.viewport_size())
            })
            .unwrap();
        let card = e2e::control_bounds(window.window_id(), "modal-card").expect("modal card");
        assert!(
            contains(viewport, card),
            "{label}: card outside native viewport"
        );
        let actions = ["plan-cancel", "plan-confirm"].map(|id| {
            let bounds = e2e::control_bounds(window.window_id(), id).expect("pinned action");
            assert!(contains(card, bounds), "{label}: {id} outside card");
            bounds
        });
        if let Some(previous) = pinned {
            assert_eq!(
                actions, previous,
                "{label}: actions moved while notes scrolled"
            );
        } else {
            pinned = Some(actions);
        }
        let scroll = e2e::control_bounds(window.window_id(), owner).expect("scroll viewport");
        assert!(contains(card, scroll), "{label}: scroll owner outside card");
        for (index, id) in path_ids.iter().enumerate() {
            if let Some(paint) = e2e::control_paint(window.window_id(), id) {
                if contains(scroll, paint.bounds) && contains(paint.mask, paint.bounds) {
                    seen_paths.insert(index);
                }
            }
        }
        for (index, id) in note_ids.iter().enumerate() {
            if let Some(paint) = e2e::control_paint(window.window_id(), id) {
                if contains(scroll, paint.bounds) && contains(paint.mask, paint.bounds) {
                    seen_notes.insert(index);
                }
            }
        }
        if seen_paths.len() == PATHS && seen_notes.len() == 3 {
            break;
        }
        assert!(
            scroll.size.height > px(0.),
            "{label}: positive scroll viewport"
        );
        // Overlapping half-viewport steps cannot jump over a fully fitting row.
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: scroll.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), -scroll.size.height / 2.)),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
    }
    assert_eq!(
        seen_paths,
        (0..PATHS).collect(),
        "{label}: all unique paths, including {}, must paint fully inside the native clip",
        paths.last().unwrap()
    );
    assert_eq!(
        seen_notes,
        (0..3).collect(),
        "{label}: summary and adjacent mandatory notes must remain reachable"
    );
}

fn run_case(
    cx: &mut VisualTestAppContext,
    slug: &str,
    zoom: f32,
    blocked: bool,
    body_scrolls: bool,
) {
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    let paths: Vec<_> = (0..PATHS)
        .map(|index| format!("note-paths/file-{index:03}.txt"))
        .collect();
    let original_theme = theme::theme().slug.to_string();
    // Start each native window at the same baseline; its scroll handles and
    // per-window probe IDs must not inherit the previous case's tail offset.
    theme::set_zoom(1.);
    let (app, window) = mount_narrow(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_theme(slug, cx);
        if body_scrolls {
            app.open_merge_modal("main".into(), None, cx);
        } else {
            let target = CommitId(git_fixture::git_output(
                fixture.path(),
                &["rev-parse", "HEAD~1"],
            ));
            app.open_reset_current_modal(target, cx);
        }
    });
    cx.run_until_parked();
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        if body_scrolls {
            let mut modal = app
                .merge_modal()
                .expect("real merge planning completed")
                .clone();
            let plan = Arc::make_mut(&mut modal.plan);
            stress_notes(plan, &paths, blocked);
            // The shared Merge renderer chooses its single body scroll owner
            // when preview_files is nonempty. This preview is typed stress
            // data too; it does not assert that merging main into itself writes.
            plan.preview_files = vec![FileStatus {
                path: "README.md".into(),
                change: ChangeKind::Modified,
            }];
            app.set_merge_modal(modal);
        } else {
            let mut modal = app
                .reset_current_modal()
                .expect("real reset planning completed")
                .clone();
            let plan = Arc::make_mut(&mut modal.plan);
            stress_notes(plan, &paths, blocked);
            plan.preview_files.clear();
            app.set_reset_current_modal(modal);
        }
        cx.notify();
    });
    let label = format!(
        "{slug}/{}/{zoom}/blocked={blocked}/body-scroll={body_scrolls}",
        i18n::lang().slug()
    );
    traverse(cx, window, &paths, blocked, body_scrolls, &label);
    // No Confirm/Enter/approval is delivered, even for the ready warning case.
    app.update(cx, |app, cx| {
        if body_scrolls {
            app.cancel_merge_modal();
        } else {
            app.cancel_reset_current_modal();
        }
        app.set_theme(&original_theme, cx);
        cx.notify();
    });
    unmount(cx, app, window);
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "{label}: repository unchanged"
    );
    eprintln!("[gui-e2e] PASS modal_scale_note_paths {label}: typed renderer stress; all 200 paths and adjacent notes reached, actions pinned, no approval/execution");
}

pub fn scenario_modal_scale_note_paths(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    for slug in ["apple-light", "apple-dark"] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                for body_scrolls in [false, true] {
                    for blocked in [false, true] {
                        run_case(cx, slug, zoom, blocked, body_scrolls);
                    }
                }
            }
        }
    }
}
