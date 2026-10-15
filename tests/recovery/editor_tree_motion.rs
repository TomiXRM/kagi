//! #1617: real editor tree paint samples, with a frozen production motion clock.
//! Reversible Before-FAIL mutation: in lib.rs::tree_motion_content insert
//! `let offset = px(0.);` immediately before styling and paint recording.
//! Insertion still fades, but the same displaced-tail midpoint assertion fails.
//! Fringe Before-FAIL: replace `for position in fringe` with
//! `for position in fringe.take(0)` in render_tree_motion_row.
//! Contrast Before-FAIL: return `theme().selection_foreground(window_active)`
//! unconditionally from tree_selection_foreground (new-selection start fails).
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{point, px, AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{e2e::system_accessibility::SystemDisplayInjection, KagiApp};
use kagi_ui_core::{
    motion, theme,
    tree_a11y::{clear_recorded_trees, recorded_tree},
};
use kagi_ui_editor::{
    dir_path_for_tree_index, EditorTreeMotionProbe, EditorWorkspaceView, TreeMenuTarget, TreeSource,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

struct MotionRestore(bool);
impl Drop for MotionRestore {
    fn drop(&mut self) {
        motion::set_clock_for_e2e(None);
        theme::set_reduce_motion(self.0);
    }
}

fn frame(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    at: Instant,
) -> Vec<EditorTreeMotionProbe> {
    motion::set_clock_for_e2e(Some(at));
    clear_recorded_trees();
    editor.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.read(|cx| editor.read(cx).tree_motion_probes_for_e2e())
}

fn right_click(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
) {
    cx.simulate_mouse_move(window, position, None, Modifiers::none());
    cx.simulate_mouse_down(window, position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(window, position, MouseButton::Right, Modifiers::none());
}

fn wait_editor(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    at: Instant,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        frame(cx, editor, window, at);
        if cx.read(|cx| !editor.read(cx).loading && !editor.read(cx).file_loading) {
            break;
        }
        assert!(Instant::now() < deadline, "editor reads did not complete");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn sample<'a>(
    rows: &'a [EditorTreeMotionProbe],
    path: &str,
    ghost: bool,
) -> &'a EditorTreeMotionProbe {
    rows.iter()
        .find(|row| row.path == Path::new(path) && row.ghost == ghost)
        .unwrap_or_else(|| panic!("missing painted {path}, ghost={ghost}: {rows:?}"))
}

fn assert_settled(rows: &[EditorTreeMotionProbe]) {
    assert!(!rows.is_empty());
    for row in rows {
        assert!(!row.ghost, "settled ghosts must be released");
        assert_eq!(row.opacity, 1.);
        assert_eq!(row.offset, px(0.));
    }
}

fn assert_fringe(rows: &[EditorTreeMotionProbe], head: bool) {
    let mut normal: Vec<_> = rows
        .iter()
        .filter(|row| !row.ghost && !row.fringe)
        .collect();
    normal.sort_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap());
    let pitch = normal[1].bounds.top() - normal[0].bounds.top();
    assert!(pitch > px(0.));
    let mut fringe: Vec<_> = rows.iter().filter(|row| row.fringe).collect();
    assert!(
        !fringe.is_empty(),
        "actual out-of-range content must be painted"
    );
    assert!(fringe.len() <= 3, "two-row splice must have bounded fringe");
    fringe.sort_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap());
    let (before, after) = if head {
        (fringe.last().unwrap().bounds.top(), normal[0].bounds.top())
    } else {
        (normal.last().unwrap().bounds.top(), fringe[0].bounds.top())
    };
    assert!(
        (after - before - pitch).abs() <= px(0.5),
        "snapped fringe must abut the final-list content"
    );
    for row in fringe {
        assert_eq!(row.opacity, 1.);
        assert!(!row.ghost);
        assert!(row.bounds.size.width > px(0.) && row.bounds.size.height > px(0.));
        assert!(!recorded_tree("ews-file-tree")
            .unwrap()
            .rows
            .values()
            .any(|ax| ax.label.contains(row.path.to_str().unwrap())));
    }
}

fn directory(editor: &EditorWorkspaceView) -> usize {
    editor
        .tree
        .iter()
        .enumerate()
        .find_map(|(index, _)| {
            (dir_path_for_tree_index(&editor.tree, index).as_deref() == Some(Path::new("motion")))
                .then_some(index)
        })
        .expect("stable motion directory")
}

fn assert_children_absent_from_ax() {
    let tree = recorded_tree("ews-file-tree").expect("real editor AX tree");
    assert!(
        !tree
            .rows
            .values()
            .any(|row| row.label.contains("a.txt") || row.label.contains("b.txt")),
        "closing ghosts must not appear in the logical AX tree"
    );
    assert!(
        tree.rows.values().any(|row| row.expanded == Some(false)),
        "chevron/AX disclosure changes immediately"
    );
}

pub fn scenario_editor_directory_motion(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion", "theme", "ui_zoom"]);
    let _restore = MotionRestore(theme::app_reduce_motion());
    let _system = SystemDisplayInjection::new(false, false);
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for index in 0..120 {
        std::fs::write(repo.join(format!("zz-long-{index:03}.txt")), "long tree\n").unwrap();
    }
    std::fs::create_dir(repo.join("motion")).unwrap();
    for path in ["motion/a.txt", "motion/b.txt", "z-tail.txt"] {
        std::fs::write(repo.join(path), "before\n").unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "editor motion fixture"]);
    for path in ["motion/a.txt", "motion/b.txt", "z-tail.txt"] {
        std::fs::write(repo.join(path), "after\n").unwrap();
    }
    let (app, window): (Entity<KagiApp>, _) = mount(cx, &repo);
    let original_zoom = theme::zoom();
    let original_theme = theme::theme().slug.to_string();
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    let mut t = Instant::now();
    wait_editor(cx, &editor, window, t);

    for source in [TreeSource::Changes, TreeSource::All] {
        editor.update(cx, |view, cx| view.set_source(source, cx));
        wait_editor(cx, &editor, window, t);
        // Source replacement's initial disclosure is not an insertion animation.
        assert_settled(&frame(cx, &editor, window, t));
        let dir = cx.read(|cx| directory(editor.read(cx)));
        if source == TreeSource::Changes {
            theme::set_reduce_motion(true);
            editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
            frame(cx, &editor, window, t);
            theme::set_reduce_motion(false);
        }
        let collapsed = frame(cx, &editor, window, t);
        let tail_top = sample(&collapsed, "z-tail.txt", false).bounds.top();
        let row_height = sample(&collapsed, "motion", false).bounds.size.height;
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        let start = frame(cx, &editor, window, t);
        assert_eq!(sample(&start, "motion/a.txt", false).opacity, 0.);
        assert!(
            recorded_tree("ews-file-tree")
                .unwrap()
                .rows
                .values()
                .any(|row| row.label.contains("a.txt")),
            "inserted AX rows are immediate"
        );
        let derivations = cx.read(|cx| editor.read(cx).tree_layout_derivations());
        let mid = frame(cx, &editor, window, t + Duration::from_millis(90));
        let a = sample(&mid, "motion/a.txt", false);
        let tail = sample(&mid, "z-tail.txt", false);
        assert!(
            a.opacity > 0. && a.opacity < 1.,
            "actual inserted paint fades"
        );
        assert!(tail.offset < px(0.) && tail.offset > row_height * -2.);
        assert!(
            tail.bounds.top() > tail_top && tail.bounds.top() < tail_top + row_height * 2.,
            "painted tail moves between old and final slots"
        );
        assert_eq!(
            cx.read(|cx| editor.read(cx).tree_layout_derivations()),
            derivations,
            "mid-flight paint must reuse the cached logical layout"
        );
        let full = frame(cx, &editor, window, t + Duration::from_millis(300));
        assert_settled(&full);
        let file_target = cx.read(|cx| {
            editor
                .read(cx)
                .files
                .iter()
                .position(|file| file.path == Path::new("motion/a.txt"))
                .unwrap()
        });
        right_click(
            cx,
            window,
            sample(&full, "motion/a.txt", false).bounds.center(),
        );
        assert_eq!(
            cx.read(|cx| editor.read(cx).tree_menu.map(|(target, _)| target)),
            Some(TreeMenuTarget::File(file_target)),
            "ordinary file context menu retains its real file target"
        );
        editor.update(cx, |view, cx| view.close_tree_menu(cx));
        frame(cx, &editor, window, t + Duration::from_millis(300));
        right_click(cx, window, sample(&full, "motion", false).bounds.center());
        assert_eq!(
            cx.read(|cx| editor.read(cx).tree_menu.map(|(target, _)| target)),
            Some(TreeMenuTarget::Dir(dir)),
            "ordinary directory context menu retains its real directory target"
        );
        editor.update(cx, |view, cx| view.close_tree_menu(cx));
        frame(cx, &editor, window, t + Duration::from_millis(300));
        if source == TreeSource::Changes {
            let tail = sample(&full, "z-tail.txt", false).bounds;
            right_click(
                cx,
                window,
                point(tail.center().x, tail.bottom() + row_height),
            );
            assert_eq!(
                cx.read(|cx| editor.read(cx).tree_menu.map(|(target, _)| target)),
                Some(TreeMenuTarget::Root),
                "genuine blank tree space retains its root context menu"
            );
            editor.update(cx, |view, cx| view.close_tree_menu(cx));
            frame(cx, &editor, window, t + Duration::from_millis(300));
        }

        // Selection wash is path-keyed; both old and new paint coexist, while
        // the authoritative selected TreeItem changes on the first frame.
        let (a_index, b_index) = cx.read(|cx| {
            let view = editor.read(cx);
            (
                view.files
                    .iter()
                    .position(|file| file.path == Path::new("motion/a.txt"))
                    .unwrap(),
                view.files
                    .iter()
                    .position(|file| file.path == Path::new("motion/b.txt"))
                    .unwrap(),
            )
        });
        theme::set_reduce_motion(true);
        editor.update(cx, |view, cx| view.select(a_index, cx));
        frame(cx, &editor, window, t + Duration::from_millis(300));
        theme::set_reduce_motion(false);
        t += Duration::from_millis(400);
        frame(cx, &editor, window, t);
        editor.update(cx, |view, cx| view.select(b_index, cx));
        frame(cx, &editor, window, t);
        let selected_ax = recorded_tree("ews-file-tree").unwrap();
        assert!(selected_ax
            .rows
            .values()
            .any(|row| row.label.contains("b.txt") && row.selected == Some(true)));
        let selected_mid = frame(cx, &editor, window, t + Duration::from_millis(90));
        wait_editor(cx, &editor, window, t);
        editor.update(cx, |view, cx| {
            view.dirty = true;
            view.window_active_override = Some(true);
            cx.notify();
        });
        for slug in ["apple-light", "apple-dark"] {
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            for millis in [0, 45, 90, 135] {
                frame(cx, &editor, window, t + Duration::from_millis(millis));
                let painted = cx.read(|cx| editor.read(cx).tree_foreground_probes_for_e2e());
                for path in ["motion/a.txt", "motion/b.txt"] {
                    let row = painted
                        .iter()
                        .find(|row| row.path == Path::new(path))
                        .unwrap();
                    assert!(
                        row.window_active,
                        "{slug} must exercise the active accent wash: {row:?}"
                    );
                    if path == "motion/b.txt" {
                        assert!(row.dirty_visible, "probe must cover a painted dirty dot");
                    }
                    if row.wash == 0. {
                        assert_eq!(
                            row.name,
                            gpui::Hsla::from(gpui::rgb(theme::theme().text_main))
                        );
                        assert_eq!(
                            row.dirty,
                            gpui::Hsla::from(gpui::rgb(theme::theme().color_warning))
                        );
                    } else if row.wash == 1. {
                        let selected = theme::theme().selection_foreground(true);
                        assert_eq!(row.name, selected);
                        assert_eq!(row.badge, selected);
                        assert_eq!(row.dirty, selected);
                    }
                    if millis == 90 {
                        assert!(row.wash > 0. && row.wash < 1.);
                    }
                    assert!(row.bounds.size.height > px(0.));
                }
            }
        }
        // Settled endpoints retain the original theme's on-accent foreground,
        // including Apple Light where maximizing contrast would change it.
        for slug in ["apple-light", "apple-dark"] {
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            frame(cx, &editor, window, t + Duration::from_millis(300));
            let painted = cx.read(|cx| editor.read(cx).tree_foreground_probes_for_e2e());
            let row = painted
                .iter()
                .find(|row| row.path == Path::new("motion/b.txt"))
                .unwrap();
            assert_eq!(row.wash, 1.);
            let selected = theme::theme().selection_foreground(true);
            assert_eq!(row.name, selected);
            assert_eq!(row.badge, selected);
            assert_eq!(row.dirty, selected);
        }
        app.update(cx, |app, cx| app.set_theme(&original_theme, cx));
        editor.update(cx, |view, cx| {
            view.dirty = false;
            view.window_active_override = None;
            cx.notify();
        });
        for path in ["motion/a.txt", "motion/b.txt"] {
            let wash = sample(&selected_mid, path, false).selection;
            assert!(
                wash > 0. && wash < 1.,
                "both real selection washes crossfade"
            );
        }
        t += Duration::from_millis(300);
        frame(cx, &editor, window, t);

        // Remove children immediately from navigation and AX, retain their real
        // content only in the painted, noninteractive presentation layer.
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        frame(cx, &editor, window, t);
        assert_children_absent_from_ax();
        let closing = frame(cx, &editor, window, t + Duration::from_millis(90));
        let ghost = sample(&closing, "motion/b.txt", true);
        let closing_tail = sample(&closing, "z-tail.txt", false);
        assert!(ghost.opacity > 0. && ghost.opacity < 1.);
        assert!(
            ghost.offset > row_height && ghost.offset < row_height * 2.,
            "retained real second child slides toward its anchor"
        );
        assert!(closing_tail.offset > px(0.) && closing_tail.offset < row_height * 2.);
        assert_children_absent_from_ax();
        let ghost_bounds = sample(&closing, "motion/a.txt", true).bounds;
        // Collapsing fixed-height contents overlap; the first ghost's leading
        // edge remains above the following ghost and the displaced real tail.
        right_click(
            cx,
            window,
            point(ghost_bounds.center().x, ghost_bounds.top() + px(0.5)),
        );
        assert_eq!(
            cx.read(|cx| editor.read(cx).tree_menu.map(|(target, _)| target)),
            None,
            "right-clicking a closing presentation ghost must not open a root or file menu"
        );
        editor.update(cx, |view, cx| view.step_selection(1, cx));
        assert_eq!(
            cx.read(|cx| {
                let view = editor.read(cx);
                view.files[view.selected.unwrap()].path.clone()
            }),
            Path::new(if source == TreeSource::All {
                "README.md"
            } else {
                "z-tail.txt"
            })
        );

        // Reverse at the frozen midpoint: same content, position and opacity;
        // membership changes immediately, without a stale duplicate ghost.
        let reverse_at = t + Duration::from_millis(90);
        let closing_bounds = ghost.bounds;
        let closing_opacity = ghost.opacity;
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        let reversed = frame(cx, &editor, window, reverse_at);
        let child = sample(&reversed, "motion/b.txt", false);
        assert!((child.opacity - closing_opacity).abs() < 0.001);
        assert!((child.bounds.top() - closing_bounds.top()).abs() <= px(1.));
        assert!(!reversed.iter().any(|row| row.ghost));
        let reopened = frame(cx, &editor, window, t + Duration::from_millis(400));
        assert_settled(&reopened);
        // Closing also has a natural endpoint, without Reduce Motion forcing it.
        t += Duration::from_millis(500);
        frame(cx, &editor, window, t);
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        frame(cx, &editor, window, t);
        assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(250)));
        assert_children_absent_from_ax();
        theme::set_reduce_motion(true);
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(250)));
        theme::set_reduce_motion(false);

        t += Duration::from_millis(500);
        frame(cx, &editor, window, t);
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        frame(cx, &editor, window, t);
        let fading = frame(cx, &editor, window, t + Duration::from_millis(60));
        assert!(fading
            .iter()
            .any(|row| row.ghost && row.opacity > 0. && row.opacity < 1.));
        // Changing policy during flight settles on this exact clock tick and
        // disabling it again cannot resurrect retained removals.
        theme::set_reduce_motion(true);
        let reduced = frame(cx, &editor, window, t + Duration::from_millis(60));
        assert_settled(&reduced);
        assert_children_absent_from_ax();
        theme::set_reduce_motion(false);
        assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(60)));
        editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
        frame(cx, &editor, window, t + Duration::from_millis(60));
        theme::set_reduce_motion(true);
        assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(60)));
        theme::set_reduce_motion(false);
        t += Duration::from_millis(800);
    }
    theme::set_zoom(1.13);
    let dir = cx.read(|cx| directory(editor.read(cx)));
    theme::set_reduce_motion(true);
    frame(cx, &editor, window, t);
    // The previous loop leaves this directory expanded.
    editor.update(cx, |view, cx| {
        view.tree_scroll
            .scroll_to_item(35, gpui::ScrollStrategy::Center);
        cx.notify();
    });
    let baseline = frame(cx, &editor, window, t);
    assert!(
        baseline.len() < 100,
        "fixture must exceed the native viewport"
    );
    theme::set_reduce_motion(false);
    editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
    let closing_start = frame(cx, &editor, window, t);
    assert_fringe(&closing_start, true);
    let derivations = cx.read(|cx| editor.read(cx).tree_layout_derivations());
    let closing_mid = frame(cx, &editor, window, t + Duration::from_millis(90));
    assert_fringe(&closing_mid, true);
    assert_eq!(
        cx.read(|cx| editor.read(cx).tree_layout_derivations()),
        derivations
    );
    editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
    let reversed = frame(cx, &editor, window, t + Duration::from_millis(90));
    for row in reversed.iter().filter(|row| !row.ghost) {
        if let Some(before) = closing_mid.iter().find(|before| before.path == row.path) {
            assert!((row.bounds.top() - before.bounds.top()).abs() <= px(0.5));
            assert_eq!(row.opacity, before.opacity);
        }
    }
    theme::set_reduce_motion(true);
    assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(90)));
    // Settle collapsed, then expand with the directory well above the viewport.
    editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
    frame(cx, &editor, window, t + Duration::from_millis(300));
    theme::set_reduce_motion(false);
    t += Duration::from_millis(400);
    frame(cx, &editor, window, t);
    editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
    assert_fringe(&frame(cx, &editor, window, t), false);
    assert_fringe(
        &frame(cx, &editor, window, t + Duration::from_millis(90)),
        false,
    );
    theme::set_reduce_motion(true);
    assert_settled(&frame(cx, &editor, window, t + Duration::from_millis(90)));
    editor.update(cx, |view, cx| {
        view.tree_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    theme::set_zoom(original_zoom);
    frame(cx, &editor, window, t + Duration::from_millis(300));
    theme::set_reduce_motion(false);
    t += Duration::from_millis(400);
    // A retained owner returning within the same frozen tick must not resume
    // its old splice, even though no monotonic time has elapsed while away.
    let other_fixture = build_fixture();
    let dir = cx.read(|cx| directory(editor.read(cx)));
    frame(cx, &editor, window, t);
    editor.update(cx, |view, cx| view.toggle_dir(dir, cx));
    frame(cx, &editor, window, t);
    let switched_at = t + Duration::from_millis(60);
    let departing = frame(cx, &editor, window, switched_at);
    assert!(departing.iter().any(|row| row.ghost));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_fixture.path().canonicalize().unwrap(), cx));
        app.switch_repo(0, cx);
    });
    let returned = frame(cx, &editor, window, switched_at);
    assert_settled(&returned);
    assert_children_absent_from_ax();
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .editor_workspace
            .as_ref()
            .unwrap()
            .entity_id()),
        editor.entity_id(),
        "same retained owner"
    );
    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor_directory_motion");
}
