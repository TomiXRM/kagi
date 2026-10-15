//! #1617: Changes retains fixed slots while file content eases between them.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{commit_panel::CommitPanelView, e2e};
use kagi_ui_core::{motion, theme};
use std::time::{Duration, Instant};

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    motion::set_clock_for_e2e(Some(at));
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn reload(cx: &mut VisualTestAppContext, panel: &Entity<CommitPanelView>, repo: &std::path::Path) {
    panel.update(cx, |panel, cx| {
        panel.reload_status(repo);
        cx.notify();
    });
}
fn y(window: AnyWindowHandle, index: usize) -> f32 {
    f32::from(
        e2e::control_bounds(window.window_id(), &format!("cp-us-file-label-{index}"))
            .expect("visible file content")
            .top(),
    )
}

fn fill(cx: &mut VisualTestAppContext, window: AnyWindowHandle, index: usize) -> [u8; 4] {
    let row =
        e2e::control_bounds(window.window_id(), &format!("cp-us-density-file-{index}")).unwrap();
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native file-row selection fill");
        let scale = window.scale_factor();
        image
            .get_pixel(
                (f32::from(row.left() + gpui::px(2.)) * scale) as u32,
                (f32::from(row.center().y) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

/// Contrast comes from rasterized glyph cores against the actual native fill,
/// not renderer colour metadata or an echo of the foreground selection formula.
pub(crate) fn assert_native_ink_contrast(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    background: [u8; 4],
    ids: &[String],
) {
    let regions: Vec<_> = ids
        .iter()
        .map(|id| {
            (
                id,
                e2e::control_bounds(window.window_id(), id)
                    .unwrap_or_else(|| panic!("missing native glyph region {id}")),
            )
        })
        .collect();
    let luminance = |pixel: [u8; 4]| {
        let linear = |value: u8| {
            let value = f64::from(value) / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
    };
    let background = luminance(background);
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native glyph contrast");
        let scale = window.scale_factor();
        for (id, bounds) in regions {
            let left = (f32::from(bounds.left()) * scale) as u32;
            let top = (f32::from(bounds.top()) * scale) as u32;
            let width = (f32::from(bounds.size.width).min(24.) * scale) as u32;
            let height = (f32::from(bounds.size.height) * scale) as u32;
            let mut strongest: f64 = 1.;
            for y in top..top + height {
                for x in left..left + width {
                    let ink = luminance(image.get_pixel(x, y).0);
                    strongest =
                        strongest.max((ink.max(background) + 0.05) / (ink.min(background) + 0.05));
                }
            }
            assert!(
                strongest >= 3.,
                "{id}: actual native glyph contrast {strongest:.3}:1 < 3:1"
            );
        }
    })
    .unwrap();
}

fn select(cx: &mut VisualTestAppContext, panel: &Entity<CommitPanelView>, index: usize) {
    panel.update(cx, |panel, cx| {
        panel.state.selected_file =
            Some(kagi::ui::commit_panel::CommitPanelFileRef::Unstaged { index });
        cx.notify();
    });
}

fn file_ink(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    side: &str,
    index: usize,
) -> Vec<[u8; 4]> {
    let bounds =
        e2e::control_bounds(window.window_id(), &format!("cp-{side}-file-label-{index}")).unwrap();
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native staged insertion opacity");
        let scale = window.scale_factor();
        let x = (f32::from(bounds.left()) * scale) as u32;
        let y = (f32::from(bounds.top()) * scale) as u32;
        let mut pixels = Vec::new();
        for dy in 0..(f32::from(bounds.size.height) * scale) as u32 {
            for dx in 0..(20. * scale) as u32 {
                pixels.push(image.get_pixel(x + dx, y + dy).0);
            }
        }
        pixels
    })
    .unwrap()
}

fn fold_motion(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    at: Instant,
    agent: bool,
    path: &str,
) {
    let fi = cx.read(|cx| {
        panel
            .read(cx)
            .state
            .unstaged
            .iter()
            .position(|f| f.path == std::path::Path::new(path))
            .unwrap()
    });
    let top = || {
        f32::from(
            e2e::control_bounds(window.window_id(), &format!("cp-us-density-file-{fi}"))
                .expect("expanded fold row")
                .top(),
        )
    };
    let toggle = |cx: &mut VisualTestAppContext, open| {
        panel.update(cx, |panel, cx| {
            if agent {
                panel.state.agent_expanded = open;
            } else {
                panel.state.generated_expanded = open;
            }
            cx.notify();
        })
    };
    theme::set_reduce_motion(false);
    frame(cx, window, at);
    toggle(cx, true);
    frame(cx, window, at);
    frame(cx, window, at + Duration::from_millis(90));
    let mid = top();
    frame(cx, window, at + Duration::from_millis(250));
    let full = top();
    assert!(
        mid < full,
        "fold child content must be displaced before settlement"
    );
    toggle(cx, false);
    frame(cx, window, at + Duration::from_millis(250));
    frame(cx, window, at + Duration::from_millis(320));
    let ghost_id = format!("cp-us-ghost:{path}");
    let ghost = e2e::control_bounds(window.window_id(), &ghost_id)
        .expect("closing content retained without list membership");
    assert!(
        f32::from(ghost.top()) < full,
        "closing fold child slides toward header"
    );
    toggle(cx, true);
    frame(cx, window, at + Duration::from_millis(320));
    assert!(
        (top() - f32::from(ghost.top())).abs() <= 1.,
        "fold reversal preserves current painted position: reopened={}, ghost={}, full={full}",
        top(),
        f32::from(ghost.top())
    );
    frame(cx, window, at + Duration::from_millis(600));
    assert!((top() - full).abs() <= 1.);
    toggle(cx, false);
    frame(cx, window, at + Duration::from_millis(600));
    e2e::clear_control_bounds(window.window_id(), &ghost_id);
    frame(cx, window, at + Duration::from_millis(850));
    assert!(
        e2e::control_bounds(window.window_id(), &ghost_id).is_none(),
        "closed fold releases presentation"
    );
    theme::set_reduce_motion(true);
    toggle(cx, true);
    frame(cx, window, at + Duration::from_millis(900));
    assert!((top() - full).abs() <= 1., "reduced fold opens immediately");
    toggle(cx, false);
    e2e::clear_control_bounds(window.window_id(), &ghost_id);
    frame(cx, window, at + Duration::from_millis(900));
    assert!(
        e2e::control_bounds(window.window_id(), &ghost_id).is_none(),
        "reduced fold closes without retained ghosts"
    );
}

fn viewport_edges(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    at: Instant,
) {
    theme::set_reduce_motion(false);
    theme::set_zoom(1.3);
    panel.update(cx, |panel, cx| {
        panel.state.tree_view = false;
        panel
            .unstaged_scroll_handle
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    frame(cx, window, at);
    frame(cx, window, at + Duration::from_millis(250));
    let normal = cx.read(|cx| panel.read(cx).state.unstaged_normal_files.clone());
    let pitch = y(window, normal[1]) - y(window, normal[0]);
    let geometry = || {
        cx.read(|cx| {
            let scroll = panel.read(cx).unstaged_scroll_handle.0.borrow();
            (scroll.base_handle.bounds(), scroll.base_handle.offset())
        })
    };
    let (viewport, _) = geometry();
    let tail = (f32::from(viewport.size.height) / pitch).ceil() as usize;
    motion::set_clock_for_e2e(Some(at + Duration::from_millis(300)));
    std::fs::write(repo.join("aaa-motion.txt"), "insert above viewport\n").unwrap();
    reload(cx, panel, repo);
    let tail_file = cx.read(|cx| panel.read(cx).state.unstaged_normal_files[tail]);
    let tail_id = format!("cp-us-density-file-{tail_file}");
    e2e::clear_control_bounds(window.window_id(), &tail_id);
    frame(cx, window, at + Duration::from_millis(300));
    let painted = e2e::control_bounds(window.window_id(), &tail_id)
        .expect("displaced tail outside final uniform_list range is mounted");
    assert!(painted.top() < viewport.bottom() && painted.bottom() > viewport.top(),
        "tail content fills the viewport during insertion: {painted:?}, {viewport:?}, pitch={pitch}");
    frame(cx, window, at + Duration::from_millis(550));

    panel.update(cx, |panel, cx| {
        panel
            .unstaged_scroll_handle
            .scroll_to_item(8, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    frame(cx, window, at + Duration::from_millis(600));
    motion::set_clock_for_e2e(Some(at + Duration::from_millis(650)));
    std::fs::remove_file(repo.join("aaa-motion.txt")).unwrap();
    reload(cx, panel, repo);
    for i in 0..40 {
        e2e::clear_control_bounds(window.window_id(), &format!("cp-us-density-file-{i}"));
    }
    frame(cx, window, at + Duration::from_millis(650));
    let (viewport, offset) = cx.read(|cx| {
        let scroll = panel.read(cx).unstaged_scroll_handle.0.borrow();
        (scroll.base_handle.bounds(), scroll.base_handle.offset())
    });
    let first = (-f32::from(offset.y) / pitch).floor() as usize;
    assert!(first > 0);
    let top_file = cx.read(|cx| panel.read(cx).state.unstaged_normal_files[first - 1]);
    let top = e2e::control_bounds(
        window.window_id(),
        &format!("cp-us-density-file-{top_file}"),
    )
    .expect("displaced top outside final uniform_list range is mounted");
    let visible = top.intersect(&viewport);
    assert!(
        visible.size.height > gpui::px(0.),
        "top content fills deletion gap"
    );
    let expected_path = cx.read(|cx| panel.read(cx).state.unstaged[top_file].path.clone());
    cx.simulate_click(window, visible.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    frame(cx, window, at + Duration::from_millis(650));
    assert_eq!(
        cx.read(|cx| panel.read(cx).active_wip.clone()),
        Some((false, expected_path)),
        "overflow row keeps its real final-index click target"
    );
    frame(cx, window, at + Duration::from_millis(900));
}

fn reopening(
    cx: &mut VisualTestAppContext,
    app: &Entity<kagi::ui::KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    at: Instant,
) {
    let paths = cx.read(|cx| {
        panel
            .read(cx)
            .state
            .unstaged
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    });
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    kagi::ui::KagiApp::hold_next_commit_panel_read_for_e2e(hold);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    for path in &paths {
        e2e::clear_control_bounds(
            window.window_id(),
            &format!("cp-us-ghost:{}", path.display()),
        );
    }
    frame(cx, window, at);
    assert!(
        paths.iter().all(|path| e2e::control_bounds(
            window.window_id(),
            &format!("cp-us-ghost:{}", path.display())
        )
        .is_none()),
        "loading placeholder does not animate the previous visit out"
    );
    release.send(());
    cx.run_until_parked();
    frame(cx, window, at);
    let fi = cx.read(|cx| panel.read(cx).state.unstaged_normal_files[0]);
    let first = file_ink(cx, window, "us", fi);
    frame(cx, window, at + Duration::from_millis(250));
    assert_eq!(
        file_ink(cx, window, "us", fi),
        first,
        "the first real status after reopening is already fully painted"
    );
}

fn foreground_motion(
    cx: &mut VisualTestAppContext,
    app: &Entity<kagi::ui::KagiApp>,
    panel: &Entity<CommitPanelView>,
    window: AnyWindowHandle,
    at: Instant,
) {
    let normal_files = cx.read(|cx| panel.read(cx).state.unstaged_normal_files.clone());
    let (fi, other) = (normal_files[0], normal_files[1]);
    for (i, preset) in ["apple-light", "apple-dark"].into_iter().enumerate() {
        assert!(theme::set_active(preset));
        theme::set_zoom(1.);
        theme::set_reduce_motion(false);
        app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
        panel.update(cx, |panel, cx| {
            panel.state.selected_file = None;
            panel
                .unstaged_scroll_handle
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
            cx.notify();
        });
        let at = at + Duration::from_secs(i as u64 * 2);
        frame(cx, window, at);
        frame(cx, window, at + Duration::from_millis(250));
        let color = || {
            e2e::control_paint(window.window_id(), &format!("cp-us-file-label-{fi}"))
                .expect("fresh filename foreground paint")
                .text_color
        };
        let normal = color();
        select(cx, panel, fi);
        frame(cx, window, at + Duration::from_millis(250));
        assert_eq!(
            color(),
            normal,
            "logical selection does not switch ink early"
        );
        frame(cx, window, at + Duration::from_millis(287));
        let background = fill(cx, window, fi);
        assert_native_ink_contrast(
            cx,
            window,
            background,
            &[
                format!("cp-us-file-label-{fi}"),
                format!("cp-us-file-badge-{fi}"),
            ],
        );
        frame(cx, window, at + Duration::from_millis(340));
        let background = fill(cx, window, fi);
        assert_native_ink_contrast(
            cx,
            window,
            background,
            &[
                format!("cp-us-file-label-{fi}"),
                format!("cp-us-file-badge-{fi}"),
            ],
        );
        frame(cx, window, at + Duration::from_millis(500));
        assert_eq!(color(), theme::theme().selection_foreground(true));
        select(cx, panel, other);
        frame(cx, window, at + Duration::from_millis(500));
        frame(cx, window, at + Duration::from_millis(560));
        let leaving = color();
        select(cx, panel, fi);
        frame(cx, window, at + Duration::from_millis(560));
        assert_eq!(color(), leaving, "foreground reversal remains continuous");
        theme::set_reduce_motion(true);
        frame(cx, window, at + Duration::from_millis(560));
        assert_eq!(color(), theme::theme().selection_foreground(true));
    }
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(fixture.path(), &["config", "commit.template", "/dev/null"]);
    for file in ["motion-b.txt", "motion-z.txt"] {
        std::fs::write(fixture.path().join(file), "new\n").unwrap();
    }
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
    let start = Instant::now();
    for _ in 0..3 {
        frame(cx, window, start);
        cx.run_until_parked();
    }
    let before = y(window, 1);
    std::fs::write(fixture.path().join("motion-a.txt"), "insert\n").unwrap();
    reload(cx, &panel, fixture.path());
    frame(cx, window, start);
    frame(cx, window, start + Duration::from_millis(90));
    let mid = y(window, 2);
    frame(cx, window, start + Duration::from_millis(250));
    let end = y(window, 2);
    assert!(before < mid && mid < end, "inserted file must push existing content mid-flight: before={before}, mid={mid}, end={end}");
    // Reverse a removal before its ghost finishes: content must not jump to
    // either endpoint when final-list membership switches back.
    let remove = start + Duration::from_millis(300);
    frame(cx, window, remove);
    std::fs::remove_file(fixture.path().join("motion-a.txt")).unwrap();
    reload(cx, &panel, fixture.path());
    frame(cx, window, remove);
    frame(cx, window, remove + Duration::from_millis(70));
    let closing = y(window, 1);
    assert!(
        before < closing && closing < end,
        "removal pushes retained rows apart"
    );
    std::fs::write(fixture.path().join("motion-a.txt"), "reinsert\n").unwrap();
    reload(cx, &panel, fixture.path());
    frame(cx, window, remove + Duration::from_millis(70));
    assert!(
        (y(window, 2) - closing).abs() <= 1.,
        "splice reversal keeps painted position"
    );
    frame(cx, window, remove + Duration::from_millis(300));
    assert!(
        (y(window, 2) - end).abs() <= 1.,
        "splice settles at fixed slots"
    );

    let selection = start + Duration::from_millis(700);
    frame(cx, window, selection);
    select(cx, &panel, 1);
    frame(cx, window, selection);
    frame(cx, window, selection + Duration::from_millis(250));
    let plain = fill(cx, window, 2);
    select(cx, &panel, 2);
    frame(cx, window, selection + Duration::from_millis(250));
    frame(cx, window, selection + Duration::from_millis(340));
    let wash = fill(cx, window, 2);
    frame(cx, window, selection + Duration::from_millis(500));
    let selected = fill(cx, window, 2);
    assert_ne!(wash, plain, "native selection wash is entering");
    assert_ne!(wash, selected, "native selection wash is not yet settled");
    select(cx, &panel, 1);
    frame(cx, window, selection + Duration::from_millis(500));
    frame(cx, window, selection + Duration::from_millis(560));
    let leaving = fill(cx, window, 2);
    select(cx, &panel, 2);
    frame(cx, window, selection + Duration::from_millis(560));
    assert_eq!(
        fill(cx, window, 2),
        leaving,
        "selection reverses from current native fill"
    );
    frame(cx, window, selection + Duration::from_millis(800));
    assert_eq!(fill(cx, window, 2), selected);

    theme::set_reduce_motion(true);
    std::fs::remove_file(fixture.path().join("motion-a.txt")).unwrap();
    reload(cx, &panel, fixture.path());
    frame(cx, window, selection + Duration::from_millis(900));
    assert!(
        (y(window, 1) - before).abs() <= 1.,
        "reduced removal is immediate"
    );
    select(cx, &panel, 0);
    frame(cx, window, selection + Duration::from_millis(900));
    let reduced = fill(cx, window, 0);
    frame(cx, window, selection + Duration::from_millis(1200));
    assert_eq!(
        fill(cx, window, 0),
        reduced,
        "reduced selection is already settled"
    );
    theme::set_reduce_motion(false);
    let stage = start + Duration::from_millis(2100);
    frame(cx, window, stage);
    let owner = cx.read(|cx| panel.read(cx).owner);
    app.update(cx, |app, cx| {
        app.do_stage_file_by_path(owner, "motion-b.txt".into(), cx)
    });
    frame(cx, window, stage);
    frame(cx, window, stage + Duration::from_millis(90));
    let stage_mid = y(window, 0);
    let staged_mid = file_ink(cx, window, "st", 0);
    frame(cx, window, stage + Duration::from_millis(250));
    let stage_end = y(window, 0);
    assert!(
        stage_end < stage_mid && stage_mid < before,
        "staging closes the old list gap gradually"
    );
    assert_ne!(
        staged_mid,
        file_ink(cx, window, "st", 0),
        "the staged-side arrival fades its native text"
    );
    app.update(cx, |app, cx| {
        app.do_unstage_file_by_path(owner, "motion-b.txt".into(), cx)
    });
    frame(cx, window, stage + Duration::from_millis(250));
    frame(cx, window, stage + Duration::from_millis(340));
    let unstage_mid = y(window, 1);
    assert!(
        stage_end < unstage_mid && unstage_mid < before,
        "unstaging pushes destination rows apart gradually"
    );
    frame(cx, window, stage + Duration::from_millis(500));
    assert!((y(window, 1) - before).abs() <= 1.);
    theme::set_reduce_motion(true);
    // Both existing fixed-height fold owners use the same splice convention.
    std::fs::write(
        fixture.path().join(".gitattributes"),
        "*.generated linguist-generated=true\n",
    )
    .unwrap();
    git(fixture.path(), &["add", ".gitattributes"]);
    git(fixture.path(), &["commit", "-qm", "generated attributes"]);
    for file in [
        "a.generated",
        "b.generated",
        ".claude/a.json",
        ".claude/b.json",
    ] {
        let path = fixture.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}\n").unwrap();
    }
    reload(cx, &panel, fixture.path());
    let folds = start + Duration::from_secs(3);
    frame(cx, window, folds);
    fold_motion(
        cx,
        &panel,
        window,
        folds + Duration::from_millis(10),
        false,
        "b.generated",
    );
    fold_motion(
        cx,
        &panel,
        window,
        folds + Duration::from_secs(2),
        true,
        ".claude/b.json",
    );
    // More than one visible screen settles the complete status batch, even
    // though it is below the core's defensive active-key ceiling.
    let bulk = folds + Duration::from_secs(4);
    theme::set_reduce_motion(false);
    frame(cx, window, bulk);
    for i in 0..32 {
        std::fs::write(fixture.path().join(format!("bulk-{i:02}.txt")), "bulk\n").unwrap();
    }
    reload(cx, &panel, fixture.path());
    frame(cx, window, bulk);
    let first_bulk = cx.read(|cx| {
        panel
            .read(cx)
            .state
            .unstaged
            .iter()
            .position(|file| file.path == std::path::Path::new("bulk-00.txt"))
            .unwrap()
    });
    let bulk_ink = file_ink(cx, window, "us", first_bulk);
    let bulk_top = y(window, first_bulk);
    frame(cx, window, bulk + Duration::from_millis(250));
    assert_eq!(
        file_ink(cx, window, "us", first_bulk),
        bulk_ink,
        "large status batches do not fade a partial subset"
    );
    assert_eq!(
        y(window, first_bulk),
        bulk_top,
        "large status batches immediately occupy final slots"
    );
    viewport_edges(
        cx,
        &panel,
        window,
        fixture.path(),
        bulk + Duration::from_secs(1),
    );
    reopening(cx, &app, &panel, window, bulk + Duration::from_secs(3));
    foreground_motion(cx, &app, &panel, window, bulk + Duration::from_secs(5));
    drop(panel);
    motion::set_clock_for_e2e(None);
    unmount(cx, app, window);
}
