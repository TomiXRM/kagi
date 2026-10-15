//! Real translated siblings must own the click, not just the painted bounds.
use super::{frame, right_click, sample, wait_editor, MotionRestore};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{point, px, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e::system_accessibility::SystemDisplayInjection, KagiApp};
use kagi_ui_core::theme;
use kagi_ui_editor::{dir_path_for_tree_index, EditorWorkspaceView, TreeMenuTarget, TreeSource};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn dir(view: &EditorWorkspaceView, path: &str) -> usize {
    (0..view.tree.len())
        .find(|&index| {
            dir_path_for_tree_index(&view.tree, index).as_deref() == Some(Path::new(path))
        })
        .unwrap()
}

fn selected(cx: &VisualTestAppContext, editor: &Entity<EditorWorkspaceView>) -> PathBuf {
    cx.read(|cx| {
        let view = editor.read(cx);
        view.files[view.selected.expect("selected file")]
            .path
            .clone()
    })
}

pub fn scenario_editor_tree_pointer_motion(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _restore = MotionRestore(theme::app_reduce_motion());
    let _system = SystemDisplayInjection::new(false, false);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::create_dir(repo.join("motion")).unwrap();
    std::fs::create_dir(repo.join("sibling")).unwrap();
    let paths = [
        "motion/a.txt",
        "motion/b.txt",
        "motion/c.txt",
        "motion/d.txt",
        "sibling/nested.txt",
        "README.md",
        "z-tail.txt",
    ];
    for path in paths {
        std::fs::write(repo.join(path), "before\n").unwrap();
    }
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["commit", "-qm", "compressed editor pointer fixture"],
    );
    for path in paths {
        std::fs::write(repo.join(path), "after\n").unwrap();
    }
    let (app, window): (Entity<KagiApp>, _) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    let mut t = Instant::now();
    wait_editor(cx, &editor, window, t);
    let mut failures = Vec::new();
    for source in [TreeSource::Changes, TreeSource::All] {
        editor.update(cx, |view, cx| view.set_source(source, cx));
        wait_editor(cx, &editor, window, t);
        for directory_target in [false, true] {
            theme::set_reduce_motion(true);
            let (motion_dir, sibling_dir, target_file) = editor.update(cx, |view, cx| {
                let motion_dir = dir(view, "motion");
                let sibling_dir = dir(view, "sibling");
                let target = view
                    .files
                    .iter()
                    .position(|file| file.path == Path::new("motion/d.txt"))
                    .unwrap();
                let baseline = view
                    .files
                    .iter()
                    .position(|file| file.path == Path::new("z-tail.txt"))
                    .unwrap();
                view.expand_all(cx);
                view.toggle_dir(motion_dir, cx);
                if !directory_target {
                    view.toggle_dir(sibling_dir, cx);
                }
                view.select(baseline, cx);
                (motion_dir, sibling_dir, target)
            });
            wait_editor(cx, &editor, window, t);
            theme::set_reduce_motion(false);
            editor.update(cx, |view, cx| view.toggle_dir(motion_dir, cx));
            frame(cx, &editor, window, t);
            let at = t + Duration::from_millis(37);
            let rows = frame(cx, &editor, window, at);
            let (path, covered) = if directory_target {
                ("sibling", "motion/d.txt")
            } else {
                ("motion/d.txt", "motion/c.txt")
            };
            let target = sample(&rows, path, false);
            let below = sample(&rows, covered, false);
            let top = if target.bounds.top() > below.bounds.top() {
                target.bounds.top()
            } else {
                below.bounds.top()
            };
            let bottom = if target.bounds.bottom() < below.bounds.bottom() {
                target.bounds.bottom()
            } else {
                below.bounds.bottom()
            };
            assert!(
                bottom - top > px(2.),
                "real compressed overlap: {source:?}/{path}: target={target:?}, covered={below:?}"
            );
            assert!(target.offset < px(0.) && target.opacity > 0.);
            let hit = point(target.bounds.center().x, top + (bottom - top) * 0.5);
            right_click(cx, window, hit);
            let menu = cx.read(|cx| editor.read(cx).tree_menu.map(|(target, _)| target));
            let expected = if directory_target {
                TreeMenuTarget::Dir(sibling_dir)
            } else {
                TreeMenuTarget::File(target_file)
            };
            assert_eq!(
                menu,
                Some(expected),
                "{source:?}/{path}: actual right-menu target"
            );
            editor.update(cx, |view, cx| view.close_tree_menu(cx));
            frame(cx, &editor, window, at);
            cx.update_window(window, |_, window, cx| {
                let root = app.read(cx).root_focus.clone().unwrap();
                root.focus(window, cx);
            })
            .unwrap();
            cx.simulate_mouse_move(window, hit, None, Modifiers::none());
            cx.simulate_click(window, hit, Modifiers::none());
            let actual = selected(cx, &editor);
            let expected = Path::new(if directory_target {
                "z-tail.txt"
            } else {
                "motion/d.txt"
            });
            if actual != expected {
                failures.push(format!(
                    "{source:?}/{path}: click selected {actual:?}, expected {expected:?}"
                ));
            }
            assert!(
                cx.update_window(window, |_, window, cx| app
                    .read(cx)
                    .root_focus
                    .as_ref()
                    .unwrap()
                    .is_focused(window))
                    .unwrap(),
                "real Editor file/directory pointer action retains root keyboard focus"
            );
            if directory_target {
                assert!(
                    cx.read(|cx| editor.read(cx).collapsed.contains(&sibling_dir)),
                    "actual directory callback must toggle its own directory"
                );
            } else if actual == expected {
                cx.simulate_keystrokes(window, "down");
                assert_eq!(
                    selected(cx, &editor),
                    Path::new("README.md"),
                    "real tree navigation survives pointer ownership"
                );
            }
            t += Duration::from_millis(500);
            wait_editor(cx, &editor, window, t);
        }
    }
    drop(editor);
    unmount(cx, app, window);
    assert!(
        failures.is_empty(),
        "compressed Editor semantic ownership: {}",
        failures.join("; ")
    );
    eprintln!("[gui-e2e] PASS editor_tree_pointer_motion");
}
