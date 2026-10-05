//! An expanded Operation Log row is text: drag across it and ⌘C copies what
//! was selected — the detail, the recovery and the recorded ref moves — and a
//! press inside it does not fold the row. Its two actions are real buttons.
use gpui::{AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_git::oplog::OpOutcome;
use kagi_git::{Backend, CommitId, Operation};

use crate::macos::{build_fixture, mount, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, name: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), name).unwrap_or_else(|| panic!("{name} is painted"))
}

fn expanded(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<usize> {
    cx.read(|cx| app.read(cx).op_log.clone().unwrap().read(cx).expanded())
}

pub fn scenario_oplog_detail_select_copy(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let mut backend = Backend::open(&repo).unwrap();
    backend.set_auto_snapshot(false);
    let op = Operation::CreateBranch {
        name: "select-demo".into(),
        at: CommitId(git_output(&repo, &["rev-parse", "HEAD"])),
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    drop(backend);

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = kagi::ui::BottomTab::OperationLog;
        app.bottom_panel_height = 520.;
        cx.notify();
    });
    paint(cx, window);
    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
    let row = cx.read(|cx| {
        let repo = repo.to_string_lossy().into_owned();
        panel
            .read(cx)
            .entries()
            .iter()
            .position(|e| kagi::ui::oplog_panel::entry_worktree(e) == repo)
            .expect("the recorded create-branch row")
    });
    cx.read(|cx| panel.read(cx).scroll_handle().scroll_to_reveal_item(row));
    paint(cx, window);
    let summary = cx
        .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(row))
        .expect("row laid out");
    let mut on_summary = summary.origin;
    on_summary.x += gpui::px(40.);
    on_summary.y += gpui::px(8.);
    cx.simulate_click(window, on_summary, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        expanded(cx, &app),
        Some(row),
        "a click on the summary opens it"
    );

    // Drag from the first detail line into the recorded ref move. Both ends sit
    // on glyphs (a drag that never touches text selects nothing), and a real
    // pointer hovers before it presses: selection hit-tests against the last
    // painted hover state.
    let recovery = bounds(window, &format!("oplog-recovery-{row}"));
    let moved = bounds(
        window,
        &format!("oplog-refmove-{row}-0-refs/heads/select-demo"),
    );
    let start = gpui::point(
        recovery.origin.x + gpui::px(14.),
        summary.origin.y + gpui::px(30.),
    );
    let end = gpui::point(
        moved.origin.x + gpui::px(200.),
        moved.origin.y + moved.size.height / 2.,
    );
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    paint(cx, window);
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(window, end, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    cx.simulate_mouse_move(window, end, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        expanded(cx, &app),
        Some(row),
        "a drag inside the expanded block must not fold the row"
    );

    let selected = cx
        .update_window(window, |_, window, cx| {
            use gpui_component::WindowExt as _;
            window.selected_text(cx)
        })
        .unwrap();
    assert!(!selected.trim().is_empty(), "the drag selects text");

    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before-copy".into()));
    cx.simulate_keystrokes(window, "cmd-c");
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("clipboard text");
    assert_ne!(copied, "before-copy", "⌘C copies the selection");
    let entry = cx.read(|cx| panel.read(cx).entries()[row].clone());
    let command = entry.recovery_plan.as_ref().unwrap().commands[0].clone();
    for part in [
        "before:",
        "after:",
        command.as_str(),
        "refs/heads/select-demo",
    ] {
        assert!(
            copied.contains(part),
            "selection lacks {part:?}: {copied:?}"
        );
    }

    // A plain press inside the block keeps it open too.
    cx.simulate_click(window, start, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        expanded(cx, &app),
        Some(row),
        "a click inside keeps the row open"
    );

    // The actions are real buttons: a press on Revert asks for its plan.
    let revert = bounds(window, &format!("oplog-revert-{row}-enabled"));
    cx.simulate_click(window, revert.center(), Modifiers::none());
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).active_modal.is_some()),
        "Revert opens its plan card"
    );

    drop(panel);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS oplog_detail_select_copy: {} chars",
        copied.chars().count()
    );
}
