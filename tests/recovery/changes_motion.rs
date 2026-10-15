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
        "fold reversal preserves current painted position"
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

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
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
    drop(panel);
    motion::set_clock_for_e2e(None);
    unmount(cx, app, window);
}
