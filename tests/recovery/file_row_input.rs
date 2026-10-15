//! Real logical rows own clicks, without masking ancestor focus/drop/scroll.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{point, px, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{commit_panel::CommitPanelView, e2e, list_a11y, KagiApp};
use kagi_ui_core::{motion, theme};
use std::time::{Duration, Instant};

#[path = "file_row_input/inline_focus.rs"]
mod inline_focus;
pub(crate) use inline_focus::inline_pointer_focus;

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    motion::set_clock_for_e2e(Some(at));
    cx.run_until_parked();
    for _ in 0..3 {
        list_a11y::clear_recorded_lists();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}
fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
}
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, row: Bounds<Pixels>) {
    cx.simulate_click(
        window,
        point(row.left() + px(80.), row.top() + px(3.)),
        gpui::Modifiers::none(),
    );
}

pub(crate) fn assert_repo_drop_on_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    row: Bounds<Pixels>,
) -> tempfile::TempDir {
    assert_repo_drop_at(cx, app, window, point(row.left() + px(80.), row.center().y))
}

fn assert_repo_drop_at(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    position: gpui::Point<Pixels>,
) -> tempfile::TempDir {
    let target = build_fixture();
    let before = cx.read(|cx| app.read(cx).tabs.len());
    // Resume a pointer gesture after e.g. Escape: GPUI classifies FileDrop
    // before converting it to MouseMove, so Entered alone retains Keyboard
    // modality and deliberately suppresses every hovered hitbox.
    cx.simulate_mouse_move(
        window,
        position,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::none(),
    );
    cx.simulate_event(
        window,
        gpui::FileDropEvent::Entered {
            position,
            paths: gpui::ExternalPaths(vec![target.path().to_path_buf()].into_iter().collect()),
        },
    );
    frame(cx, window, motion::now());
    cx.simulate_event(window, gpui::FileDropEvent::Submit { position });
    cx.run_until_parked();
    frame(cx, window, motion::now());
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.tabs.len(),
            before + 1,
            "native repository folder drop over a real file row reaches its ancestor target"
        );
        assert_eq!(
            app.repo_path.as_deref(),
            Some(target.path().canonicalize().unwrap().as_path())
        );
    });
    target
}

pub(crate) fn top_file_ghost(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) -> tempfile::TempDir {
    let at = motion::now() + Duration::from_millis(250);
    theme::set_reduce_motion(false);
    frame(cx, window, at);
    let path = repo.join("!!!motion-ghost.txt");
    std::fs::write(&path, "removed top file\n").unwrap();
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        cx.notify();
    });
    frame(cx, window, at + Duration::from_millis(250));
    let removal = at + Duration::from_millis(300);
    frame(cx, window, removal);
    std::fs::remove_file(path).unwrap();
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        cx.notify();
    });
    let first = cx.read(|cx| panel.read(cx).state.unstaged_normal_files[0]);
    let real_id = format!("cp-us-density-file-{first}");
    let ghost_id = "cp-us-ghost:!!!motion-ghost.txt";
    e2e::clear_control_bounds(window.window_id(), &real_id);
    e2e::clear_control_bounds(window.window_id(), ghost_id);
    let middle = removal + Duration::from_millis(37);
    frame(cx, window, middle);
    let ghost = e2e::control_paint(window.window_id(), ghost_id).expect("closing top-file ghost");
    let real = e2e::control_paint(window.window_id(), &real_id).expect("real row under top ghost");
    let position = point(real.bounds.left() + px(80.), real.bounds.top() + px(3.));
    assert!(
        ghost.order > real.order
            && ghost.bounds.contains(&position)
            && ghost.mask.contains(&position)
            && real.bounds.contains(&position)
            && real.mask.contains(&position),
        "context press intersects the visible ghost painted after its covered real row: ghost={ghost:?}, real={real:?}"
    );
    // Prove actual ghost ink in the leading strip where no live row can paint.
    let ink = cx
        .update_window(window, |_, window, _| {
            let image = window
                .render_to_image()
                .expect("native top-file ghost pixels");
            let scale = window.scale_factor();
            let pixel = |value: Pixels| (f32::from(value) * scale) as u32;
            let top = pixel(ghost.bounds.top() + theme::scaled_px(3.));
            let bottom = pixel(real.bounds.top() - px(1.));
            assert!(
                bottom > top,
                "the ghost has a genuinely visible leading strip"
            );
            let background = image.get_pixel(pixel(ghost.bounds.left() + px(4.)), top).0;
            let mut ink = 0;
            for y in top..bottom {
                for x in pixel(ghost.bounds.left() + theme::scaled_px(30.))
                    ..pixel(
                        (ghost.bounds.left() + theme::scaled_px(180.)).min(ghost.bounds.right()),
                    )
                {
                    let sample = image.get_pixel(x, y).0;
                    if (0..3).any(|i| sample[i].abs_diff(background[i]) > 12) {
                        ink += 1;
                    }
                }
            }
            ink
        })
        .unwrap();
    assert!(
        ink > 10,
        "the retained ghost must actually paint visible filename ink"
    );
    let selected = cx.read(|cx| panel.read(cx).active_wip.clone());
    right_click(cx, window, real.bounds);
    frame(cx, window, middle);
    assert!(
        cx.read(|cx| app.read(cx).file_menu.is_none()),
        "right-clicking the visible removed top file must not open its covered live file menu"
    );
    assert_eq!(cx.read(|cx| panel.read(cx).active_wip.clone()), selected);
    assert_repo_drop_at(cx, app, window, position)
}

pub(crate) fn following_fold(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    at: Instant,
    incoming: Bounds<Pixels>,
) {
    let covered = cx.read(|cx| {
        let view = panel.read(cx);
        let path = &view.active_wip.as_ref().unwrap().1;
        view.state
            .unstaged
            .iter()
            .position(|file| &file.path == path)
            .unwrap()
    });
    let normal = cx.read(|cx| panel.read(cx).state.unstaged_normal_files[0]);
    click(
        cx,
        window,
        bounds(window, &format!("cp-us-density-file-{normal}")),
    );
    frame(cx, window, at);
    let selected = cx.read(|cx| panel.read(cx).active_wip.clone());
    assert!(
        selected.is_some(),
        "a distinct normal file is selected through the real UI"
    );
    let header = bounds(window, "cp-us-agent-header");
    assert!(
        header.top() + px(3.) < incoming.bottom() && header.top() >= incoming.top(),
        "following fold really overlaps the incoming generated file: {header:?}, {incoming:?}"
    );
    click(cx, window, header);
    frame(cx, window, at);
    cx.read(|cx| {
        let panel = panel.read(cx);
        assert!(
            panel.state.agent_expanded,
            "intended fold toggles immediately"
        );
        assert_eq!(
            panel.active_wip, selected,
            "fold click must not also select an underlying fading file"
        );
    });
    covered_file_unfocused(cx, window, covered, "fold header");
    non_file_input(
        cx,
        app,
        window,
        bounds(window, "cp-us-agent-header"),
        at,
        "fold header",
    );
    panel.update(cx, |panel, cx| {
        panel.state.agent_expanded = false;
        cx.notify();
    });
    frame(cx, window, at);
}

pub(crate) fn directory(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    at: Instant,
) {
    theme::set_reduce_motion(true);
    for folder in ["000-motion", "001-motion"] {
        std::fs::create_dir_all(repo.join(folder)).unwrap();
        std::fs::write(repo.join(folder).join("base.txt"), "base\n").unwrap();
    }
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        panel.state.tree_view = true;
        panel.active_wip = None;
        panel.state.selected_file = None;
        cx.notify();
    });
    frame(cx, window, at);
    let start = cx.read(|cx| panel.read(cx).state.unstaged_tree.iter().position(|row|
        matches!(row, kagi_ui_core::file_tree::TreeRow::Dir { name, .. } if name.as_ref() == "000-motion")
    ).unwrap());
    panel.update(cx, |panel, cx| {
        panel
            .unstaged_scroll_handle
            .scroll_to_item_strict(start, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    frame(cx, window, at);
    theme::set_reduce_motion(false);
    frame(cx, window, at + Duration::from_millis(300));
    for name in ["z-a.txt", "z-b.txt"] {
        std::fs::write(repo.join("000-motion").join(name), "incoming\n").unwrap();
    }
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        cx.notify();
    });
    frame(cx, window, at + Duration::from_millis(337));
    let index = cx.read(|cx| {
        panel
            .read(cx)
            .state
            .unstaged
            .iter()
            .position(|file| file.path == std::path::Path::new("000-motion/z-b.txt"))
            .unwrap()
    });
    let incoming = bounds(window, &format!("cp-us-density-file-{index}"));
    let row = bounds(window, "cp-us-dir-001-motion");
    assert!(
        row.top() + px(3.) < incoming.bottom() && row.top() >= incoming.top(),
        "directory really overlaps an incoming file: {row:?}, {incoming:?}"
    );
    let selected = cx.read(|cx| panel.read(cx).active_wip.clone());
    click(cx, window, row);
    frame(cx, window, at + Duration::from_millis(337));
    cx.read(|cx| {
        let panel = panel.read(cx);
        assert_eq!(
            panel.active_wip, selected,
            "non-collapsible directory click must not select a fading file below it"
        );
    });
    covered_file_unfocused(cx, window, index, "directory");
    non_file_input(
        cx,
        app,
        window,
        row,
        at + Duration::from_millis(337),
        "directory",
    );
    frame(cx, window, at + Duration::from_millis(600));
    panel.update(cx, |panel, cx| {
        panel.state.tree_view = false;
        cx.notify();
    });
    frame(cx, window, at + Duration::from_millis(600));
}

pub fn inspector_more(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            motion::set_clock_for_e2e(None);
            theme::set_reduce_motion(self.0);
        }
    }
    let _restore = Restore(theme::reduce_motion());
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(false);
    theme::set_zoom(1.3);
    let (repo, paths) = crate::inspector_motion::viewport_fixture(60);
    let (app, window) = mount(cx, repo.path());
    app.update(cx, |app, cx| {
        app.inspector_visible = true;
        app.inspector_tree_view = false;
        app.inspector_generated_expanded = false;
        app.bottom_panel_open = false;
        app.select(0);
        app.close_main_diff();
        cx.notify();
    });
    let t = Instant::now();
    frame(cx, window, t);
    let generated: Vec<_> = paths
        .iter()
        .filter_map(|path| cx.read(|cx| app.read(cx).inspector_file_index_for_e2e(path)))
        .collect();
    let mut normal_paths = vec![".gitattributes".to_owned(), "a.txt".into(), "b.txt".into()];
    normal_paths.extend((0..60).map(|n| format!("n-{n:02}.txt")));
    let normal = normal_paths
        .iter()
        .filter(|path| {
            cx.read(|cx| app.read(cx).inspector_file_index_for_e2e(path))
                .is_some()
        })
        .count();
    let a = cx
        .read(|cx| app.read(cx).inspector_file_index_for_e2e("a.txt"))
        .unwrap();
    click(cx, window, bounds(window, &format!("inspector-file-{a}")));
    frame(cx, window, t);
    let scroll = cx.read(|cx| app.read(cx).inspector_scroll_for_e2e());
    scroll.scroll_to_item_strict(normal, gpui::ScrollStrategy::Top);
    frame(cx, window, t);
    click(cx, window, bounds(window, "generated-fold-header"));
    frame(cx, window, t + Duration::from_millis(37));
    assert!(cx.read(|cx| app.read(cx).inspector_generated_expanded));
    let first = bounds(window, &format!("inspector-file-{}", generated[0]));
    let second = bounds(window, &format!("inspector-file-{}", generated[1]));
    let gap = second.top() - first.top();
    let pitch = scroll.0.borrow().last_item_size.unwrap().contents.height
        / (normal + generated.len() + 2) as f32;
    let viewport = scroll.0.borrow().base_handle.bounds();
    let more_top = first.top() + gap * generated.len() as f32;
    let original_offset = scroll.0.borrow().base_handle.offset().y;
    let offset = original_offset - (more_top - viewport.top() - pitch * 3.);
    scroll
        .0
        .borrow()
        .base_handle
        .set_offset(point(px(0.), offset));
    e2e::clear_control_bounds(window.window_id(), "inspector-files-more");
    frame(cx, window, t + Duration::from_millis(37));
    let more = bounds(window, "inspector-files-more");
    let incoming = bounds(
        window,
        &format!("inspector-file-{}", generated.last().unwrap()),
    );
    assert!(
        more.top() + px(3.) < incoming.bottom() && more.top() >= incoming.top(),
        "More really overlaps a fading generated file: {more:?}, {incoming:?}"
    );
    click(cx, window, more);
    frame(cx, window, t + Duration::from_millis(37));
    non_file_input(
        cx,
        &app,
        window,
        more,
        t + Duration::from_millis(37),
        "Inspector More",
    );
    scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
    frame(cx, window, t + Duration::from_millis(37));
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    let selected: Vec<_> = ax.rows.values().filter(|(_, selected)| *selected).collect();
    assert_eq!(
        selected.len(),
        1,
        "More click must leave the original normal file selected"
    );
    assert!(selected[0].0.ends_with("a.txt"));
    let a = cx
        .read(|cx| app.read(cx).inspector_file_index_for_e2e("a.txt"))
        .unwrap();
    right_click(cx, window, bounds(window, &format!("inspector-file-{a}")));
    frame(cx, window, t + Duration::from_millis(37));
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .inspector_file_menu
            .as_ref()
            .map(|(index, _)| *index)),
        Some(a),
        "a real Inspector file retains its own right-menu target"
    );
    assert!(e2e::control_paint(window.window_id(), "insp-menu-copy").is_some());
    cx.simulate_keystrokes(window, "escape");
    e2e::clear_control_bounds(window.window_id(), "insp-menu-copy");
    frame(cx, window, t + Duration::from_millis(37));
    assert!(cx.read(|cx| app.read(cx).inspector_file_menu.is_none()));
    assert!(
        e2e::control_paint(window.window_id(), "insp-menu-copy").is_none(),
        "Escape must repaint a surface with the Inspector menu removed before a new drop"
    );
    scroll
        .0
        .borrow()
        .base_handle
        .set_offset(point(px(0.), offset));
    e2e::clear_control_bounds(window.window_id(), "inspector-files-more");
    frame(cx, window, t + Duration::from_millis(37));
    let painted_more = e2e::control_paint(window.window_id(), "inspector-files-more")
        .expect("restored More must be painted, not a retained offscreen measurement");
    let drop_position = point(
        painted_more.bounds.left() + px(80.),
        painted_more.bounds.center().y,
    );
    assert!(
        painted_more.bounds.contains(&drop_position) && painted_more.mask.contains(&drop_position),
        "drop point must intersect the actual visible More surface: {painted_more:?}, {drop_position:?}"
    );
    let _target = assert_repo_drop_on_row(cx, &app, window, bounds(window, "inspector-files-more"));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS inspector_more_motion");
}

pub(crate) fn visibility_return(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    at: Instant,
) {
    theme::set_reduce_motion(false);
    panel.update(cx, |panel, cx| {
        panel.state.tree_view = false;
        panel
            .unstaged_scroll_handle
            .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    frame(cx, window, at);
    let (first, second, path) = cx.read(|cx| {
        let panel = panel.read(cx);
        let first = panel.state.unstaged_normal_files[0];
        (
            first,
            panel.state.unstaged_normal_files[1],
            panel.state.unstaged[first].path.clone(),
        )
    });
    let before = bounds(window, &format!("cp-us-density-file-{first}")).top();
    let pitch = bounds(window, &format!("cp-us-density-file-{second}")).top() - before;
    std::fs::write(
        repo.join("!!!motion-visibility.txt"),
        "insert before visible rows\n",
    )
    .unwrap();
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        cx.notify();
    });
    frame(cx, window, at + Duration::from_millis(37));
    let index = cx.read(|cx| {
        panel
            .read(cx)
            .state
            .unstaged
            .iter()
            .position(|file| file.path == path)
            .unwrap()
    });
    let probe = format!("cp-us-density-file-{index}");
    let middle = bounds(window, &probe).top();
    assert!(
        middle > before && middle < before + pitch,
        "hide begins with real displaced Changes content"
    );
    app.update(cx, |app, cx| app.show_editor_mode(cx));
    frame(cx, window, at + Duration::from_millis(37));
    e2e::clear_control_bounds(window.window_id(), &probe);
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    frame(cx, window, at + Duration::from_millis(37));
    assert!(
        (bounds(window, &probe).top() - before - pitch).abs() <= px(1.),
        "rapid workspace return must settle retained Changes displacement"
    );
}

pub(crate) fn focused_file_menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    index: usize,
    path: &str,
    at: Instant,
) {
    let id = format!("cp-us-row-{index}");
    assert!(
        cx.update_window(window, |_, window, _| {
            e2e::commit_panel_labels::focused(&id, window)
        })
        .unwrap(),
        "the painted compressed file must own keyboard and AX focus"
    );
    cx.simulate_keystrokes(window, "menu");
    frame(cx, window, at);
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .file_menu
            .as_ref()
            .map(|menu| menu.path.clone())),
        Some(std::path::PathBuf::from(path)),
        "keyboard menu targets the same painted file"
    );
    cx.simulate_keystrokes(window, "escape");
    frame(cx, window, at);
    right_click(
        cx,
        window,
        bounds(window, &format!("cp-us-density-file-{index}")),
    );
    frame(cx, window, at);
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .file_menu
            .as_ref()
            .map(|menu| menu.path.clone())),
        Some(std::path::PathBuf::from(path)),
        "a real compressed file retains its own right-menu target"
    );
    cx.simulate_keystrokes(window, "escape");
    frame(cx, window, at);
}

fn covered_file_unfocused(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    index: usize,
    kind: &str,
) {
    let id = format!("cp-us-row-{index}");
    assert!(
        !cx.update_window(window, |_, window, _| {
            e2e::commit_panel_labels::focused(&id, window)
        })
        .unwrap(),
        "{kind} must not focus the covered file below it"
    );
}

fn non_file_input(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    row: Bounds<Pixels>,
    at: Instant,
    kind: &str,
) {
    let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
    let root_focused = cx
        .update_window(window, |_, window, _| root.is_focused(window))
        .unwrap();
    right_click(cx, window, row);
    frame(cx, window, at);
    let menus = cx.read(|cx| {
        let app = app.read(cx);
        (
            app.file_menu.as_ref().map(|menu| menu.path.clone()),
            app.inspector_file_menu.as_ref().map(|(index, _)| *index),
        )
    });
    assert!(root_focused && menus.0.is_none() && menus.1.is_none(),
        "{kind} keeps ancestor focus and cannot open the covered file menu: root_focus={root_focused}, menus={menus:?}");
}

fn right_click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, row: Bounds<Pixels>) {
    let position = point(row.left() + px(80.), row.top() + px(3.));
    cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
    cx.simulate_mouse_down(
        window,
        position,
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        window,
        position,
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
}
