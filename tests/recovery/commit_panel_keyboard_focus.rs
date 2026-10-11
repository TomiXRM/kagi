//! Native pointer/focus regressions for the Commit Panel review corrections.
use super::*;
use kagi::ui::commit_panel::CommitPanelView;

fn click_at(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: gpui::Point<Pixels>) {
    paint(cx, window);
    cx.simulate_event(
        window,
        gpui::MouseDownEvent {
            position: at,
            button: gpui::MouseButton::Left,
            modifiers: gpui::Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        },
    );
    cx.simulate_event(
        window,
        gpui::MouseUpEvent {
            position: at,
            button: gpui::MouseButton::Left,
            modifiers: gpui::Modifiers::none(),
            click_count: 1,
        },
    );
    paint(cx, window);
}

pub(super) fn pointer_selector(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "first staged diff\n").unwrap();
    std::fs::write(fixture.path().join("selection.txt"), "second staged diff\n").unwrap();
    git(fixture.path(), &["add", "README.md", "selection.txt"]);
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        let owner = cx.read(|cx| panel.read(cx).owner);
        let paths = cx.read(|cx| {
            panel
                .read(cx)
                .state
                .staged
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(paths.len(), 2);
        cx.update_window(window, |_, _, cx| {
            app.update(cx, |app, cx| {
                app.select_commit_panel_file(
                    owner,
                    kagi::ui::commit_panel::CommitPanelFileRef::Staged { index: 0 },
                    cx,
                );
            });
        })
        .unwrap();
        paint(cx, window);
        let original = active_selection(cx, &app);
        assert_eq!(original.1, paths[0]);
        focus(cx, window, "cp-view-list");
        click_at(cx, window, bounds(window, "cp-view-tree").center());
        assert!(cx.read(|cx| panel.read(cx).state.tree_view));
        assert_root_focus(cx, window, &app);
        assert_eq!(active_selection(cx, &app), original);
        for (key, path) in [("down", &paths[1]), ("up", &paths[0])] {
            press(cx, window, key);
            assert_eq!(
                &active_selection(cx, &app).1,
                path,
                "pointer selection must retain real diff arrow routing"
            );
            assert!(
                cx.read(|cx| panel.read(cx).state.tree_view),
                "diff arrows must not hijack the selector"
            );
            assert_root_focus(cx, window, &app);
        }
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}

fn assert_root_focus(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
) {
    let root = cx
        .read(|cx| app.read(cx).root_focus.clone())
        .expect("production root focus");
    assert!(
        cx.update_window(window, |_, window, _| root.is_focused(window))
            .unwrap(),
        "selector pointer click must release to actual root focus"
    );
}

fn click_expanded_bulk(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
    hit_id: &str,
) {
    let content = bounds(window, id);
    let hit = bounds(window, hit_id);
    let at = gpui::point((hit.left() + content.left()) / 2., content.center().y);
    assert!(
        at.x < content.left(),
        "regression click must be outside visible content"
    );
    assert!(
        hit.contains(&at),
        "regression click must be within real expanded action frame"
    );
    click_at(cx, window, at);
}

pub(super) fn pointer_bulk(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "pointer bulk edit\n").unwrap();
    std::fs::write(
        fixture.path().join("selection.txt"),
        "original second file\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "selection.txt"]);
    git(
        fixture.path(),
        &["commit", "-m", "Add second pointer fixture file"],
    );
    std::fs::write(
        fixture.path().join("selection.txt"),
        "second pointer bulk edit\n",
    )
    .unwrap();
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        click_at(cx, window, bounds(window, "cp-view-tree").center());
        for (id, hit, contract, staged) in [
            (
                "cp-stage-all",
                "cp-stage-all-hit",
                "[kagi] staged-all: 2 file(s)",
                true,
            ),
            (
                "cp-unstage-all",
                "cp-unstage-all-hit",
                "[kagi] unstaged-all: 2 file(s)",
                false,
            ),
        ] {
            let before = contract_count(contract);
            click_expanded_bulk(cx, window, id, hit);
            assert_eq!(
                contract_count(contract),
                before + 1,
                "expanded bulk hit must invoke exactly one callback"
            );
            assert_eq!(
                cx.read(|cx| panel.read(cx).state.staged.len()),
                2 * usize::from(staged)
            );
            assert_eq!(
                cx.read(|cx| panel.read(cx).state.unstaged.len()),
                2 * usize::from(!staged)
            );
            assert_root_focus(cx, window, &app);
            assert_pointer_bulk_diff_keys(cx, window, &app, &panel, staged);
        }
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}

fn assert_pointer_bulk_diff_keys(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    staged: bool,
) {
    let (owner, paths) = cx.read(|cx| {
        let panel = panel.read(cx);
        let files = if staged {
            &panel.state.staged
        } else {
            &panel.state.unstaged
        };
        (
            panel.owner,
            files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(paths.len(), 2);
    cx.update_window(window, |_, _, cx| {
        app.update(cx, |app, cx| {
            let file = if staged {
                kagi::ui::commit_panel::CommitPanelFileRef::Staged { index: 0 }
            } else {
                kagi::ui::commit_panel::CommitPanelFileRef::Unstaged { index: 0 }
            };
            app.select_commit_panel_file(owner, file, cx);
        });
    })
    .unwrap();
    paint(cx, window);
    for (key, path) in [("down", &paths[1]), ("up", &paths[0])] {
        press(cx, window, key);
        let selected = cx.read(|cx| {
            let pane = app.read(cx).ui().main_diff.as_ref().unwrap();
            match &pane.read(cx).view.source {
                kagi::ui::MainDiffSource::Staged { path } if staged => path.clone(),
                kagi::ui::MainDiffSource::Unstaged { path } if !staged => path.clone(),
                _ => panic!("bulk arrows must retain the selected staged/unstaged diff section"),
            }
        });
        assert_eq!(
            &selected, path,
            "bulk pointer action must retain diff arrow routing"
        );
        assert!(
            cx.read(|cx| panel.read(cx).state.tree_view),
            "diff arrows must not hijack the selector"
        );
        assert_root_focus(cx, window, app);
    }
}

pub(super) fn hidden_bulk_focus(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "visibility focus edit\n").unwrap();
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        let unstaged = cx.read(|cx| panel.read(cx).state.unstaged.clone());
        for id in ["cp-stage-all", "cp-discard-all"] {
            focus(cx, window, id);
            press(cx, window, "tab");
            press(cx, window, "shift-tab");
            assert_focus(cx, window, id);
            panel.update(cx, |panel, cx| {
                panel.state.unstaged.clear();
                cx.notify();
            });
            paint(cx, window);
            assert_focus(cx, window, "cp-view-list");
            panel.update(cx, |panel, cx| {
                panel.state.unstaged = unstaged.clone();
                cx.notify();
            });
            paint(cx, window);
        }
        let title = cx.read(|cx| panel.read(cx).title_input.clone()).unwrap();
        cx.update_window(window, |_, window, cx| {
            title.read(cx).focus_handle(cx).focus(window, cx);
        })
        .unwrap();
        panel.update(cx, |panel, cx| {
            panel.state.unstaged.clear();
            cx.notify();
        });
        paint(cx, window);
        assert!(
            cx.update_window(window, |_, window, cx| title
                .read(cx)
                .focus_handle(cx)
                .is_focused(window))
                .unwrap(),
            "render reconciliation must preserve unrelated input focus"
        );
        drop(title);
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}
pub(super) fn bulk_keys(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "keyboard bulk edit\n").unwrap();
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        for key in ["enter", "space"] {
            ax("cp-stage-all", Role::Button, Msg::CommitPanelStageAll.t());
            ax(
                "cp-discard-all",
                Role::Button,
                Msg::CommitPanelDiscardAll.t(),
            );
            assert_eq!(labels::accessible_disabled("cp-stage-all"), Some(false));
            let row = bounds(window, "cp-selected-unstaged-row");
            focus(cx, window, "cp-stage-all");
            press(cx, window, "tab");
            assert_focus(cx, window, "cp-discard-all");
            assert_eq!(
                bounds(window, "cp-selected-unstaged-row"),
                row,
                "bulk focus must not shift file rows"
            );
            press(cx, window, "tab");
            assert_focus(cx, window, "cp-view-list");
            focus(cx, window, "cp-stage-all");
            let staged = contract_count("[kagi] staged-all: 1 file(s)");
            press(cx, window, key);
            assert_eq!(
                contract_count("[kagi] staged-all: 1 file(s)"),
                staged + 1,
                "one keyboard activation must stage once"
            );
            assert_eq!(cx.read(|cx| panel.read(cx).state.staged.len()), 1);
            assert!(cx.read(|cx| panel.read(cx).state.unstaged.is_empty()));
            assert_focus(cx, window, "cp-view-list");
            ax(
                "cp-unstage-all",
                Role::Button,
                Msg::CommitPanelUnstageAll.t(),
            );
            press(cx, window, "tab");
            assert_focus(cx, window, "cp-unstage-all");
            let unstaged = contract_count("[kagi] unstaged-all: 1 file(s)");
            press(cx, window, key);
            assert_eq!(
                contract_count("[kagi] unstaged-all: 1 file(s)"),
                unstaged + 1,
                "one keyboard activation must unstage once"
            );
            assert!(cx.read(|cx| panel.read(cx).state.staged.is_empty()));
            assert_eq!(cx.read(|cx| panel.read(cx).state.unstaged.len()), 1);
            assert_focus(cx, window, "cp-view-list");
            press(cx, window, "shift-tab");
            assert_focus(cx, window, "cp-discard-all");
            press(cx, window, key);
            assert!(cx.read(|cx| app.read(cx).discard_modal().is_some()));
            press(cx, window, "escape");
            assert!(cx.read(|cx| app.read(cx).discard_modal().is_none()));
            assert_eq!(
                std::fs::read_to_string(fixture.path().join("README.md")).unwrap(),
                "keyboard bulk edit\n"
            );
        }
        drop(panel);
        labels::clear();
        unmount(cx, app, window);
    }
}
