//! Inline controls keep their file row's keyboard origin before async removal.
use super::{bounds, click, frame};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{point, px, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{motion, theme};
use std::time::Instant;

pub fn inline_pointer_focus(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    theme::set_reduce_motion(true);
    let repo = build_fixture();
    const NAME: &str = "!!!inline-focus.txt";
    std::fs::write(repo.path().join(NAME), "inline pointer focus\n").unwrap();
    std::fs::write(repo.path().join("zz-inline-staged.txt"), "staged peer\n").unwrap();
    git(repo.path(), &["add", "--", "zz-inline-staged.txt"]);
    std::fs::write(
        repo.path().join("zzz-inline-staged-other.txt"),
        "second staged peer\n",
    )
    .unwrap();
    git(repo.path(), &["add", "--", "zzz-inline-staged-other.txt"]);
    std::fs::write(repo.path().join("zz-inline-peer.txt"), "unstaged peer\n").unwrap();
    let index_names = || {
        let output = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["diff", "--cached", "--name-only"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };
    let before = index_names();
    let (app, window) = mount(cx, repo.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx))
    })
    .unwrap();
    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
    let at = Instant::now();
    frame(cx, window, at);
    let mut observed = [(false, false, false, None), (false, false, false, None)];
    for (slot, staged) in [false, true].into_iter().enumerate() {
        let side = if staged { "st" } else { "us" };
        let action = if staged { "unstage" } else { "stage" };
        cx.read(|cx| {
            let state = &panel.read(cx).state;
            let files = if staged {
                &state.staged
            } else {
                &state.unstaged
            };
            assert_eq!(files[0].path, std::path::Path::new(NAME));
        });
        let other = if staged { 2 } else { 1 };
        let other_id = format!("cp-{side}-density-file-{other}");
        click(cx, window, bounds(window, &other_id));
        frame(cx, window, at);
        let row_id = format!("cp-{side}-row-0");
        let button = bounds(window, &format!("cp-first-row-{action}"));
        let position = button.center();
        cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
        cx.simulate_mouse_down(
            window,
            position,
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        frame(cx, window, at);
        let focused = cx
            .update_window(window, |_, window, _| {
                e2e::commit_panel_labels::focused(&row_id, window)
            })
            .unwrap();
        let ax = e2e::commit_panel_labels::accessible_role(&row_id)
            == Some(gpui::Role::ListBoxOption)
            && e2e::commit_panel_labels::accessible_name(&row_id)
                .is_some_and(|name| name.contains(NAME));
        // Inspect keyboard origin before release could asynchronously remove
        // this row. Staged rows intentionally have no context-menu action.
        let (keyboard_focused, keyboard_path) = if staged {
            cx.simulate_keystrokes(window, "down");
            frame(cx, window, at);
            let focused = cx
                .update_window(window, |_, window, _| {
                    e2e::commit_panel_labels::focused("cp-st-row-1", window)
                })
                .unwrap();
            let path = cx.read(|cx| {
                panel
                    .read(cx)
                    .active_wip
                    .as_ref()
                    .filter(|(staged, _)| *staged)
                    .map(|(_, path)| path.clone())
            });
            (focused, path)
        } else {
            cx.simulate_keystrokes(window, "menu");
            frame(cx, window, at);
            let path = cx.read(|cx| {
                app.read(cx)
                    .file_menu
                    .as_ref()
                    .map(|menu| menu.path.clone())
            });
            cx.simulate_keystrokes(window, "escape");
            frame(cx, window, at);
            (true, path)
        };
        // Complete the diagnostic press outside its button, without staging.
        cx.simulate_mouse_up(
            window,
            point(button.left() - px(8.), position.y),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        frame(cx, window, at);
        // Prove the real Git action with a fresh, uninterrupted gesture too.
        click(cx, window, bounds(window, &other_id));
        frame(cx, window, at);
        let position = bounds(window, &format!("cp-first-row-{action}")).center();
        cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
        cx.simulate_mouse_down(
            window,
            position,
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        let action_focused = cx
            .update_window(window, |_, window, _| {
                e2e::commit_panel_labels::focused(&row_id, window)
            })
            .unwrap();
        cx.simulate_mouse_up(
            window,
            position,
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        frame(cx, window, at);
        observed[slot] = (
            focused && action_focused,
            ax,
            keyboard_focused,
            keyboard_path,
        );
        if staged {
            assert_eq!(
                index_names(),
                before,
                "the actual Unstage button restores the original staged set"
            );
        } else {
            assert!(
                index_names().lines().any(|path| path == NAME),
                "the actual Stage button stages its exact file"
            );
        }
    }
    assert!(
        observed.iter().zip([NAME, "zz-inline-staged.txt"]).all(|((focus, ax, keyboard, path), expected)| {
            *focus && *ax && *keyboard && path.as_deref() == Some(std::path::Path::new(expected))
        }),
        "Stage and Unstage pointer-down must focus their own AX row and subsequent keyboard target before removal: {observed:?}"
    );
    drop(panel);
    motion::set_clock_for_e2e(None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS changes_inline_pointer_focus");
}
