//! Native Changes row, menu keyboard and viewport regressions (#1333–#1335).
use super::*;
use std::path::{Path, PathBuf};

fn fixture(count: usize) -> tempfile::TempDir {
    let fixture = build_fixture();
    for i in 0..count {
        std::fs::write(
            fixture.path().join(format!("changes-{i:03}.txt")),
            "original\n",
        )
        .unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(
        fixture.path(),
        &["commit", "--allow-empty", "-m", "Changes keyboard fixture"],
    );
    for i in 0..count {
        let path = fixture.path().join(format!("changes-{i:03}.txt"));
        std::fs::write(&path, "staged\n").unwrap();
        git(
            fixture.path(),
            &["add", path.file_name().unwrap().to_str().unwrap()],
        );
        std::fs::write(&path, "unstaged\n").unwrap();
    }
    fixture
}

fn snapshot(repo: &Path) -> (Vec<u8>, Vec<(PathBuf, Vec<u8>)>) {
    let dot_git = repo.join(".git");
    let index_path = if dot_git.is_dir() {
        dot_git.join("index")
    } else {
        let marker = std::fs::read_to_string(&dot_git).unwrap();
        PathBuf::from(marker.trim().strip_prefix("gitdir: ").unwrap()).join("index")
    };
    let index = std::fs::read(index_path).unwrap();
    let files = std::fs::read_dir(repo)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            path.is_file()
                .then(|| (path.clone(), std::fs::read(path).unwrap()))
        })
        .collect();
    (index, files)
}

fn replace(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    app.update(cx, |app, cx| {
        app.ui()
            .commit_panel
            .as_ref()
            .unwrap()
            .update(cx, |panel, cx| {
                panel.reload_status(repo);
                cx.notify();
            });
    });
}
pub fn scenario_changes_rows_keyboard(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    for count in [0, 1, 200] {
        let fixture = fixture(count);
        for case in matrix() {
            let (app, window) = open(cx, fixture.path(), case);
            let preserved = snapshot(fixture.path());
            if count > 0 {
                for side in ["us", "st"] {
                    file_section_keys(cx, &app, window, fixture.path(), side, count);
                }
                focus(cx, window, "cp-view-list");
                press(cx, window, "right");
                for side in ["us", "st"] {
                    focus(cx, window, &format!("cp-{side}-row-0"));
                    press(cx, window, "end");
                    assert_focus(cx, window, &format!("cp-{side}-row-{}", count - 1));
                    press(cx, window, "home");
                    ax(
                        &format!("cp-{side}-row-0"),
                        Role::TreeItem,
                        &modified_row_name(),
                    );
                }
            } else {
                assert!(labels::focus_handle("cp-us-row-0").is_none());
                assert!(labels::focus_handle("cp-st-row-0").is_none());
            }
            assert_eq!(
                snapshot(fixture.path()),
                preserved,
                "navigation does not write index or working tree"
            );
            labels::clear();
            unmount(cx, app, window);
        }
    }
}

fn modified_row_name() -> String {
    format!(
        "{} changes-000.txt",
        kagi_ui_core::tree_a11y::file_status(Some(&kagi_git::ChangeKind::Modified), false)
    )
}

fn file_section_keys(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    side: &str,
    count: usize,
) {
    let first = format!("cp-{side}-row-0");
    focus(cx, window, &first);
    for (key, at) in [
        ("end", count - 1),
        ("up", count.saturating_sub(2)),
        ("home", 0),
        ("down", usize::from(count > 1)),
        ("home", 0),
    ] {
        press(cx, window, key);
        let id = format!("cp-{side}-row-{at}");
        assert_focus(cx, window, &id);
        contained(
            bounds(window, &format!("cp-{side}-viewport")),
            bounds(window, &id),
            "keyboard row in viewport",
        );
    }
    ax(&first, Role::ListBoxOption, &modified_row_name());
    for key in ["enter", "space"] {
        select_file_once(cx, app, window, side, key);
    }
    replace(cx, app, repo);
    paint(cx, window);
    assert_focus(cx, window, &first);
    press(cx, window, "tab");
    press(cx, window, "shift-tab");
    assert_focus(cx, window, &first);
    if count > 5 {
        remembered_row_tab(cx, app, window, side, count);
        press(cx, window, "home");
        assert_focus(cx, window, &first);
    }
    if count > 1 {
        app.update(cx, |app, cx| {
            app.ui()
                .commit_panel
                .as_ref()
                .unwrap()
                .update(cx, |panel, cx| {
                    panel.reload_status(repo);
                    panel.state.unstaged.reverse();
                    panel.state.staged.reverse();
                    cx.notify();
                });
        });
        paint(cx, window);
        assert_focus(cx, window, &format!("cp-{side}-row-{}", count - 1));
        replace(cx, app, repo);
        paint(cx, window);
        assert_focus(cx, window, &first);
    }
}

fn remembered_row_tab(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    side: &str,
    count: usize,
) {
    focus(cx, window, &format!("cp-{side}-row-0"));
    for _ in 0..5 {
        press(cx, window, "down");
    }
    let remembered = format!("cp-{side}-row-5");
    assert!(
        cx.update_window(window, |_, window, _| labels::focused(&remembered, window))
            .unwrap(),
        "row 5 focused after five Down keys"
    );
    press(cx, window, "shift-tab");
    assert!(
        !section_holds_focus(cx, window, side, count),
        "Shift+Tab must leave the section from remembered row 5"
    );
    press(cx, window, "tab");
    assert!(
        cx.update_window(window, |_, window, _| labels::focused(&remembered, window))
            .unwrap(),
        "Tab returns to remembered row 5 after backward exit"
    );
    press(cx, window, "tab");
    assert!(
        section_holds_focus(cx, window, side, count),
        "remembered row's Stage/Unstage action remains keyboard reachable"
    );
    if side == "us" {
        inline_action_menu_anchor(cx, app, window, &remembered);
    }
    press(cx, window, "tab");
    assert!(
        !section_holds_focus(cx, window, side, count),
        "Tab after the inline action leaves the section"
    );
    press(cx, window, "shift-tab");
    press(cx, window, "shift-tab");
    assert!(
        cx.update_window(window, |_, window, _| labels::focused(&remembered, window))
            .unwrap(),
        "Shift+Tab returns to remembered row 5 after forward exit"
    );
}

fn inline_action_menu_anchor(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    row: &str,
) {
    let (width, height) = cx
        .update_window(window, |_, window, _| {
            let viewport = window.viewport_size();
            (f32::from(viewport.width), f32::from(viewport.height))
        })
        .unwrap();
    // Stay in the presentation that owns the focused inline action. Desktop
    // widening moves the panel's left edge; compact height changes move the
    // bottom Changes surface without crossing the rem-based breakpoint.
    let enlarged = if kagi_domain::compact::is_compact(
        width,
        theme::rem_size_px(),
        theme::compact_threshold_rem(),
    ) {
        (width, height + 120.)
    } else {
        (width + 320., height)
    };
    for (dimensions, opener) in [(enlarged, "shift-f10"), ((width, height), "menu")] {
        let old = bounds(window, row);
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        draw(cx, window, dimensions);
        paint(cx, window);
        let current = bounds(window, row);
        assert_ne!(
            current.origin, old.origin,
            "resize moves the row while its Stage button owns focus"
        );
        press(cx, window, opener);
        cx.read(|cx| {
            let app = app.read(cx);
            let menu = app.file_menu.as_ref().unwrap();
            assert_eq!(menu.path, Path::new("changes-005.txt"));
            assert_eq!(
                menu.anchor,
                current.bottom_left(),
                "context menu uses current row bounds with descendant focus"
            );
        });
        press(cx, window, "escape");
        assert_focus(cx, window, row);
        press(cx, window, "tab");
    }
}

fn section_holds_focus(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    side: &str,
    count: usize,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        (0..count).any(|fi| {
            labels::focus_handle(&format!("cp-{side}-row-{fi}"))
                .is_some_and(|handle| handle.contains_focused(window, cx))
        })
    })
    .unwrap()
}

fn select_file_once(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    side: &str,
    key: &str,
) {
    let before = kagi_ui_core::klog::tail();
    press(cx, window, key);
    let after = kagi_ui_core::klog::tail();
    let overlap = (0..=before.len().min(after.len()))
        .rev()
        .find(|&n| before[before.len() - n..] == after[..n])
        .unwrap();
    assert_eq!(
        after[overlap..]
            .iter()
            .filter(|line| line.starts_with("[kagi] commit-panel diff: changes-000.txt "))
            .count(),
        1,
        "one diff dispatch per activation"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let diff = app.ui().main_diff.as_ref().unwrap().read(cx);
        let path = match &diff.view.source {
            kagi::ui::MainDiffSource::Staged { path } if side == "st" => path,
            kagi::ui::MainDiffSource::Unstaged { path } if side == "us" => path,
            _ => panic!("wrong diff source section"),
        };
        assert_eq!(path, Path::new("changes-000.txt"));
    });
}

pub fn scenario_changes_focus_follows_selection(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = fixture(3);
    for case in matrix() {
        let (app, window) = focus_regressions::open_changes(cx, fixture.path(), case);
        for side in ["us", "st"] {
            let first = bounds(window, &format!("cp-{side}-row-0"));
            cx.simulate_click(window, first.center(), gpui::Modifiers::none());
            paint(cx, window);
            for (key, index) in [
                ("end", 2),
                ("home", 0),
                ("down", 1),
                ("up", 0),
                ("pagedown", 2),
                ("pageup", 0),
            ] {
                press(cx, window, key);
                let row = format!("cp-{side}-row-{index}");
                assert_focus(cx, window, &row);
                cx.read(|cx| {
                    let diff = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
                    let path = match &diff.view.source {
                        kagi::ui::MainDiffSource::Staged { path } if side == "st" => path,
                        kagi::ui::MainDiffSource::Unstaged { path } if side == "us" => path,
                        _ => panic!("wrong Changes diff section"),
                    };
                    assert_eq!(
                        path,
                        &PathBuf::from(format!("changes-{index:03}.txt")),
                        "keyboard focus and active diff must select the same Changes row"
                    );
                });
                assert_eq!(labels::accessible_selected(&row), Some(true));
                if side == "us" {
                    press(cx, window, "shift-f10");
                    cx.read(|cx| {
                        assert_eq!(
                            app.read(cx).file_menu.as_ref().unwrap().path,
                            PathBuf::from(format!("changes-{index:03}.txt")),
                            "file menu target follows the selected and focused row"
                        );
                    });
                    press(cx, window, "escape");
                    assert_focus(cx, window, &row);
                }
            }
        }
        labels::clear();
        unmount(cx, app, window);
    }
}

pub fn scenario_changes_discard_keyboard(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = fixture(2);
    std::fs::write(fixture.path().join("untracked.txt"), "keep untracked\n").unwrap();
    let worktree_root = tempfile::tempdir().unwrap();
    let linked = worktree_root.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-b",
            "keyboard-linked",
            linked.to_str().unwrap(),
        ],
    );
    for path in ["changes-000.txt", "changes-001.txt", "untracked.txt"] {
        std::fs::write(linked.join(path), "linked dirty\n").unwrap();
    }
    for case in matrix() {
        for repo in [fixture.path(), linked.as_path()] {
            let (app, window) = open(cx, fixture.path(), case);
            if repo == linked {
                app.update(cx, |app, cx| {
                    e2e::open_worktree_panel_no_inputs(
                        app,
                        repo.to_path_buf(),
                        "keyboard-linked",
                        0,
                        cx,
                    )
                });
                paint(cx, window);
            }
            let preserved = snapshot(repo);
            for (index, path) in [(0, "changes-000.txt"), (2, "untracked.txt")] {
                discard_menu(cx, &app, window, repo, index, path);
                assert_eq!(
                    snapshot(repo),
                    preserved,
                    "keyboard plan/cancel never writes"
                );
            }
            labels::clear();
            unmount(cx, app, window);
        }
    }
}

fn discard_menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    index: usize,
    path: &str,
) {
    for (opener, activation) in [("shift-f10", "enter"), ("menu", "space")] {
        let row = format!("cp-us-row-{index}");
        if index > 0 {
            focus(cx, window, "cp-us-row-0");
            press(cx, window, "end");
        }
        focus(cx, window, &row);
        let focused_name = cx
            .update_window(window, |_, window, _| {
                (0..3)
                    .map(|fi| format!("cp-us-row-{fi}"))
                    .find(|id| labels::focused(id, window))
                    .and_then(|id| labels::accessible_name(&id))
                    .expect("a rendered Changes row owns keyboard focus")
            })
            .unwrap();
        assert_focus(cx, window, &row);
        press(cx, window, opener);
        let expected = cx.read(|cx| app.read(cx).file_menu.clone().unwrap());
        assert!(
            focused_name.ends_with(&format!(" {}", expected.path.display())),
            "Discard path {:?} must match the focused row AX name {focused_name:?}",
            expected.path
        );
        assert_eq!(
            expected.path,
            Path::new(path),
            "requested {row}; actual focused AX name {focused_name:?}"
        );
        for (key, at) in [("end", 2), ("up", 1), ("home", 0), ("down", 1), ("end", 2)] {
            press(cx, window, key);
            assert_focus(
                cx,
                window,
                ["file-menu-ext", "file-menu-history", "file-menu-discard"][at],
            );
        }
        press(cx, window, "escape");
        assert_focus(cx, window, &row);
        press(cx, window, opener);
        app.update(cx, |app, cx| {
            app.ui()
                .commit_panel
                .as_ref()
                .unwrap()
                .update(cx, |panel, cx| {
                    panel.reload_status(repo);
                    panel.state.unstaged.swap(0, 1);
                    cx.notify();
                });
        });
        paint(cx, window);
        assert_eq!(cx.read(|cx| app.read(cx).file_menu.clone()), Some(expected));
        press(cx, window, "end");
        press(cx, window, activation);
        cx.read(|cx| {
            let modal = app.read(cx).discard_modal().unwrap();
            assert_eq!(modal.paths, vec![path.to_string()]);
            assert!(
                focused_name.ends_with(&format!(" {}", modal.paths[0])),
                "planned Discard path must match its opener's focused row AX name"
            );
            assert!(modal.plan.blockers.is_empty());
        });
        crate::destructive_modal_return::root_return_keeps_modal(cx, app, window, repo);
        cx.read(|cx| {
            let modal = app.read(cx).discard_modal().unwrap();
            assert_eq!(modal.paths, vec![path.to_string()]);
            assert!(!modal.confirm_armed);
        });
        press(cx, window, "escape");
        assert!(
            cx.read(|cx| app.read(cx).discard_modal().is_none()),
            "Escape cancels the unchanged destructive sheet"
        );
        replace(cx, app, repo);
        paint(cx, window);
    }
}
pub fn scenario_changes_file_menu_viewport(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = fixture(1);
    for case in matrix() {
        let (app, window) = open(cx, fixture.path(), case);
        for (x, y) in [
            (0., 0.),
            (case.dimensions.0, 0.),
            (0., case.dimensions.1),
            case.dimensions,
            (case.dimensions.0 / 2., 0.),
            (case.dimensions.0 / 2., case.dimensions.1),
            (0., case.dimensions.1 / 2.),
            (case.dimensions.0, case.dimensions.1 / 2.),
        ] {
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    e2e::defer_file_menu(app, 0, gpui::point(px(x), px(y)), window, cx)
                })
            })
            .unwrap();
            paint(cx, window);
            let viewport = Bounds::new(
                gpui::point(px(8.), px(8.)),
                size(px(case.dimensions.0 - 16.), px(case.dimensions.1 - 16.)),
            );
            contained(
                viewport,
                bounds(window, "changes-file-menu"),
                "complete file menu",
            );
            assert_eq!(
                cx.read(|cx| app.read(cx).file_menu.as_ref().unwrap().path.clone()),
                Path::new("changes-000.txt")
            );
            for (id, label) in [
                ("file-menu-ext", Msg::OpenInExternalEditor),
                ("file-menu-history", Msg::MenuShowFileHistory),
                ("file-menu-discard", Msg::EditorTreeDiscard),
            ] {
                ax(id, Role::MenuItem, label.t());
            }
            for id in ["file-menu-ext", "file-menu-history", "file-menu-discard"] {
                contained(viewport, bounds(window, id), id);
                assert!(bounds(window, id).size.height >= px(20.));
            }
            press(cx, window, "escape");
        }
        labels::clear();
        unmount(cx, app, window);
    }
}
