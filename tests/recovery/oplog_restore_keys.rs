//! #1382: keyboard-only restore inspection, native geometry and scrollbar drags.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_key, wait_idle};
use crate::recovery_oplog_panel::{paint, restore_card, run};
use gpui::{px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, KagiApp};
use kagi_git::oplog::{read_oplog_tail, Actor};
use kagi_git::{Backend, CommitId, Operation};
use kagi_ui_core::theme;
#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

/// #1382: inspection keys reveal every destructive target without approval.
pub fn scenario_oplog_restore_keyboard(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let mut backend = Backend::open(&repo).unwrap();
    run(
        &mut backend,
        Actor::Human,
        Operation::CreateBranch {
            name: "restore-point".into(),
            at: CommitId(git_output(&repo, &["rev-parse", "HEAD"])),
        },
    );
    let point = read_oplog_tail(1).pop().unwrap().id;
    for n in 0..19 {
        run(
            &mut backend,
            Actor::Human,
            Operation::CreateBranch {
                name: format!("target-{n:02}"),
                at: CommitId(git_output(&repo, &["rev-parse", "HEAD"])),
            },
        );
    }
    for n in 0..20 {
        std::fs::write(repo.join("keyboard-history.txt"), format!("{n}\n")).unwrap();
        git(&repo, &["add", "keyboard-history.txt"]);
        run(
            &mut backend,
            Actor::Human,
            Operation::Commit {
                message: format!("history {n}"),
            },
        );
    }
    let before = git_output(&repo, &["show-ref"]);
    let (app, window) = mount(cx, &repo);
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for language in [i18n::Lang::En, i18n::Lang::Ja] {
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(language, cx);
                        app.open_oplog_restore_modal(
                            Operation::RestoreToPoint { entry_id: point },
                            cx,
                        );
                    });
                    wait_idle(cx, &app);
                    paint(cx, window);
                    cx.update(|cx| {
                        gpui_component::Theme::global_mut(cx).scrollbar_show =
                            gpui_component::scroll::ScrollbarShow::Always
                    });
                    paint(cx, window);
                    inspect_restore_keyboard(cx, &app, window);
                    inspect_restore_prose(cx, &app, window);
                    assert!(!restore_card(cx, &app).confirm_armed);
                    assert_eq!(git_output(&repo, &["show-ref"]), before);
                    press_key(cx, &app, window, "escape");
                }
            }
        }
    }
    unmount(cx, app, window);
}

fn inspect_restore_prose(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    use kagi_domain::plan_note::{OplogRestoreNote, PlanNote};
    let card = restore_card(cx, app);
    for blocked in [false, true] {
        let mut plan = (*card.plan).clone();
        plan.warnings.clear();
        plan.blockers.clear();
        for n in 0..31 {
            let branch = format!("target-{n}-{}", "long-branch-".repeat(6));
            let path = format!("/worktrees/{n}/{}", "long-path-".repeat(12));
            let note = if blocked {
                OplogRestoreNote::DeletesCheckedOutBranch { branch, path }
            } else {
                OplogRestoreNote::CheckedOutDirty { branch, path }
            };
            if blocked {
                plan.blockers.push(PlanNote::OplogRestore(note));
            } else {
                plan.warnings.push(PlanNote::OplogRestore(note));
            }
        }
        app.update(cx, |app, cx| {
            app.admit_oplog_restore_plan_for_test(card.op.clone(), plan, cx);
        });
        paint(cx, window);
        for _ in 0..12 {
            cx.simulate_keystrokes(window, "tab");
            paint(cx, window);
            if cx
                .update_window(window, |_, window, cx| {
                    app.read(cx).oplog_restore_body_focused_for_e2e(window)
                })
                .unwrap()
            {
                break;
            }
        }
        assert!(
            cx.update_window(window, |_, window, cx| {
                app.read(cx).oplog_restore_body_focused_for_e2e(window)
            })
            .unwrap(),
            "Tab reaches the named safety-prose region"
        );
        cx.simulate_keystrokes(window, "pagedown");
        cx.simulate_keystrokes(window, "pageup");
        cx.simulate_keystrokes(window, "end");
        let tail = if blocked {
            "restore-blocker-30"
        } else {
            "restore-warning-30"
        };
        let mut visible = false;
        for _ in 0..80 {
            paint(cx, window);
            let viewport = e2e::control_bounds(window.window_id(), "restore-body").unwrap();
            let row = e2e::control_bounds(window.window_id(), tail).unwrap();
            visible = row.top() >= viewport.top() && row.bottom() <= viewport.bottom() + px(1.);
            if visible {
                break;
            }
            cx.simulate_keystrokes(
                window,
                if row.top() < viewport.top() {
                    "up"
                } else {
                    "down"
                },
            );
        }
        assert!(visible, "keyboard reveals final safety note: {tail}");
        assert!(!restore_card(cx, app).confirm_armed);
        cx.simulate_keystrokes(window, "home");
        paint(cx, window);
    }
}

fn inspect_restore_keyboard(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let card = restore_card(cx, app);
    assert_eq!(card.restores.len(), 20);
    let id = window.window_id();
    let preview = card.preview.clone().unwrap();
    let kagi_domain::restore_preview::RestorePreview::Graph { rows, .. } = &preview.graph else {
        panic!("expected graph preview");
    };
    assert!(rows.len() > 6);
    for (list, first, last) in [
        (
            "restore-refs",
            "restore-ref-0",
            "restore-ref-19".to_string(),
        ),
        (
            "restore-preview-rows",
            "restore-preview-row-0",
            format!("restore-preview-row-{}", rows.len() - 1),
        ),
    ] {
        let a11y = kagi::ui::list_a11y::recorded_list(list).unwrap();
        assert_eq!(a11y.role, Some(gpui::Role::List));
        let name = if list == "restore-refs" {
            i18n::oplog_panel::OplogPanelMsg::RestoreRefs
        } else {
            i18n::oplog_panel::OplogPanelMsg::RestoreAfter
        };
        assert_eq!(a11y.label, i18n::Msg::OplogPanel(name).t());
        for _ in 0..12 {
            cx.simulate_keystrokes(window, "tab");
            paint(cx, window);
            let focused = cx
                .update_window(window, |_, window, cx| {
                    app.read(cx)
                        .list_row_focused_for_e2e(list, window)
                        .is_some()
                })
                .unwrap();
            if focused {
                break;
            }
        }
        let focused = cx
            .update_window(window, |_, window, cx| {
                app.read(cx)
                    .list_row_focused_for_e2e(list, window)
                    .is_some()
            })
            .unwrap();
        assert!(focused, "Tab reaches {list}");
        for key in ["down", "up", "pagedown", "pageup", "end"] {
            cx.simulate_keystrokes(window, key);
        }
        paint(cx, window);
        paint(cx, window);
        let viewport = e2e::control_bounds(id, list).unwrap();
        let row = e2e::control_bounds(id, &last).expect("last inspection row drawn");
        assert!(
            row.top() >= viewport.top() && row.bottom() <= viewport.bottom() + px(1.),
            "{list}: End reveals {row:?} in {viewport:?}"
        );
        let body = e2e::control_bounds(id, "restore-body").unwrap();
        assert!(
            row.top() >= body.top() && row.bottom() <= body.bottom() + px(1.),
            "focused row must also be visible in body: {row:?} in {body:?}"
        );
        cx.simulate_keystrokes(window, "home");
        paint(cx, window);
        let viewport = e2e::control_bounds(id, list).unwrap();
        let row = e2e::control_bounds(id, first).unwrap();
        assert!(row.top() >= viewport.top() && row.bottom() <= viewport.bottom() + px(1.));
        restore_scrollbar_drag(cx, app, window, list);
    }
}

fn restore_scrollbar_drag(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    list: &str,
) {
    use gpui_component::scroll::ScrollbarHandle;
    let handle = cx
        .read(|cx| app.read(cx).oplog_restore_scroll_for_e2e(list))
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        focus.focus(window, cx);
    })
    .unwrap();
    handle.scroll_to(gpui::ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
    });
    paint(cx, window);
    let id = if list == "restore-refs" {
        "restore-refs-scrollbar"
    } else {
        "restore-preview-scrollbar"
    };
    crate::surface_scrollbars::drag(cx, window, id, &handle);
    let bounds = e2e::control_bounds(window.window_id(), id).unwrap();
    let thumb =
        (bounds.size.height / handle.content_size().height * bounds.size.height).max(px(48.));
    let from = gpui::point(
        bounds.right() - px(8.),
        bounds.bottom() - thumb / 2. - px(2.),
    );
    let to = gpui::point(from.x, bounds.top() - px(20.));
    cx.simulate_mouse_move(window, from, None, gpui::Modifiers::none());
    cx.simulate_mouse_down(
        window,
        from,
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(window, to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(window, to, gpui::MouseButton::Left, gpui::Modifiers::none());
    paint(cx, window);
    assert!(
        handle.offset().y.abs() < px(1.),
        "reverse thumb drag reaches first row"
    );
}
