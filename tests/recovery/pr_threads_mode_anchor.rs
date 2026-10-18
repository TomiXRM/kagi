//! #1660: semantic reading positions survive presentation changes in the real
//! PR consumer. Reuse the retained-thread Git fixture, native wheel and paint
//! recorders; no test installs diff rows, projections or scroll offsets.
use super::*;
use crate::keyboard_nav::keys;
use gpui::AppContext;
use kagi::ui::DiffRow;

const LEFT_BODY: &str = "Current base review paragraph one.\n\nCurrent base review paragraph two.\n\nCurrent base review paragraph three.";
const RIGHT_BODY: &str = "Current head review, not the base-side body.";
const OTHER_BODY: &str = "Only the other file owns this review.";

fn anchor_text(first_edit: u32) -> String {
    let mut text = String::new();
    for line in 1..=500 {
        match line {
            223 | 224 | 280 => continue,
            220..=222 => text.push_str(&format!("unequal replacement {line}\n")),
            340 => text.push_str(&format!(
                "wrapped replacement {line} {}\n",
                "actual Git source words ".repeat(24)
            )),
            500 => text.push_str("ANCHOR_TRUE_TAIL\n"),
            _ if [first_edit, 100, 160, 400, 460].contains(&line) => {
                text.push_str(&format!("changed {line}\n"));
            }
            _ => text.push_str(&format!("line {line}\n")),
        }
        if line == 400 {
            text.push_str("ADDED_WITHOUT_LEFT_CELL\n");
        }
    }
    text
}

fn anchor_notes() -> Vec<ReviewThread> {
    let mut notes = vec![
        thread(DiffSide::Right, Some(40), 40, "upper open review"),
        thread(DiffSide::Right, Some(159), 159, "current context review"),
        thread(DiffSide::Left, Some(220), 220, LEFT_BODY),
        thread(DiffSide::Right, Some(220), 220, RIGHT_BODY),
        thread(
            DiffSide::Left,
            Some(223),
            223,
            "unequal replacement surplus",
        ),
        thread(DiffSide::Left, Some(280), 280, "removed-only review"),
        thread(DiffSide::Right, Some(337), 337, "wrapped head review"),
        thread(DiffSide::Right, Some(398), 398, "added-only review"),
        thread(DiffSide::Right, Some(498), 498, "actual tail review"),
    ];
    let mut other = thread(DiffSide::Right, Some(40), 40, OTHER_BODY);
    other.path = "d.txt".into();
    notes.push(other);
    notes
}

struct Restore {
    split: bool,
    zoom: f32,
    _saved: crate::gui_isolation::SavedKeys,
}
impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_diff_split(self.split);
        theme::set_zoom(self.zoom);
    }
}

fn with_pr(
    cx: &mut VisualTestAppContext,
    test: impl FnOnce(
        &mut VisualTestAppContext,
        &Entity<KagiApp>,
        AnyWindowHandle,
        &std::path::Path,
        &std::path::Path,
        &str,
    ),
) {
    let _restore = Restore {
        split: theme::diff_split(),
        zoom: theme::zoom(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["diff_split", "ui_zoom"]),
    };
    theme::set_diff_split(false);
    theme::set_zoom(1.);
    let _gh = OfflineGh::install();
    let (_fixture, repo, _remote_dir, remote, head) =
        projection_fixture_with_text(&anchor_text(40));
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        queue_notes(anchor_notes());
        app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&head), cx));
        wait_loaded(cx, &app, &head);
        wait_note_body(cx, &app, LEFT_BODY, 10);
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        test(cx, &app, window, &repo, &remote, &head);
    }));
    // Retire the root before draining viewed-checkbox animations, including on
    // failure, so a regression never leaves a native window or retained owner.
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire anchor PR root");
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn source_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    side: DiffSide,
    line: u32,
) -> usize {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        mode.tabs[mode.active.unwrap()]
            .diff
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .position(|row| {
                matches!(row, DiffRow::Line { old_lineno, new_lineno, .. }
                if match side {
                    DiffSide::Left => *old_lineno == Some(line),
                    DiffSide::Right => *new_lineno == Some(line),
                })
            })
            .unwrap_or_else(|| panic!("{side:?} line {line} is absent from actual Git diff"))
    })
}

fn badge(row: usize, side: DiffSide) -> String {
    format!(
        "pr-thread-badge-{row}{}",
        if theme::diff_split() {
            match side {
                DiffSide::Left => "-l",
                DiffSide::Right => "-r",
            }
        } else {
            ""
        }
    )
}

fn wheel_by(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dy: gpui::Pixels,
) {
    let viewport = diff_scroll(cx, app).viewport_bounds();
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), dy)),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    paint(cx, window);
}

fn reveal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
    down: bool,
) {
    let scroll = diff_scroll(cx, app);
    wheel(cx, window, &scroll, id, if down { -1. } else { 1. });
    painted(cx, window, id);
}

fn align(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
    inset: f32,
) {
    for _ in 0..4 {
        let at = painted(cx, window, id);
        let viewport = diff_scroll(cx, app).viewport_bounds();
        let dy = viewport.top() + gpui::px(inset) - at.top();
        if dy.abs() < gpui::px(1.) {
            break;
        }
        wheel_by(cx, app, window, dy);
    }
    painted(cx, window, id);
    assert!(
        diff_scroll(cx, app).logical_scroll_top().item_ix > 0,
        "mode transfer must begin at a genuine nonzero reading position"
    );
}

fn switch(cx: &mut VisualTestAppContext, window: AnyWindowHandle, split: bool) {
    // PR headers do not use HeaderFit's bounds recorder. Exercise the same
    // mode setting and real renderer here; actual header clicks are Tier B.
    theme::set_diff_split(split);
    paint(cx, window);
}

fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read(|cx| {
        cx.read_from_clipboard()
            .expect("Copy wrote clipboard")
            .text()
            .unwrap()
    })
}

fn copy_all(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (gpui::FocusHandle, String) {
    let first = source_row(cx, app, DiffSide::Right, 40);
    let marker = painted(cx, window, &badge(first, DiffSide::Right));
    let viewport = diff_scroll(cx, app).viewport_bounds();
    cx.simulate_click(
        window,
        gpui::point(viewport.center().x, marker.center().y),
        gpui::Modifiers::none(),
    );
    keys(cx, window, "cmd-a cmd-c");
    let text = clipboard(cx);
    assert!(text.contains("ANCHOR_TRUE_TAIL\n"));
    assert!(text.contains("ADDED_WITHOUT_LEFT_CELL\n"));
    assert!(
        !text.contains("@@") && !text.contains(LEFT_BODY) && !text.contains(RIGHT_BODY),
        "PR Copy contains only code, not hunk headers or inline thread bodies"
    );
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with('+') || line.starts_with('-')),
        "Copy strips diff sigils from the actual selected source"
    );
    // Select All reveals its final caret. Wheel back without changing that
    // selection, then begin the independent middle-to-tail reading traversal.
    wheel_by(cx, app, window, gpui::px(20_000_000.));
    let focus = cx
        .update_window(window, |_, window, cx| window.focused(cx))
        .unwrap()
        .expect("diff body focused");
    (focus, text)
}

fn assert_copy_focus(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    focus: &gpui::FocusHandle,
    text: &str,
) {
    assert_eq!(
        cx.update_window(window, |_, window, cx| window.focused(cx))
            .unwrap()
            .as_ref(),
        Some(focus),
        "mode transfer must retain the focused body without another click or Tab"
    );
    cx.update_window(window, |_, _, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("not copied".into()))
    })
    .unwrap();
    keys(cx, window, "cmd-c");
    assert_eq!(
        clipboard(cx),
        text,
        "same-source mode change preserves actual code-only selection"
    );
}

pub fn scenario_pr_threads_mode_anchor_code(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, _, _, _| {
        let (focus, copied) = copy_all(cx, app, window);
        // Context, unequal paired replacement, surplus removal, wholly
        // one-sided deletion, wrapped replacement, addition-only cell, tail.
        for (side, line) in [
            (DiffSide::Right, 159),
            (DiffSide::Right, 220),
            (DiffSide::Left, 223),
            (DiffSide::Left, 280),
            (DiffSide::Right, 337),
            (DiffSide::Right, 398),
            (DiffSide::Right, 498),
        ] {
            let row = source_row(cx, app, side, line);
            reveal(cx, app, window, &badge(row, side), true);
            // Put the viewport inside this code row, not in the preceding
            // surplus deletion: split pairing can reorder those source rows.
            align(cx, app, window, &badge(row, side), -2.);
            if side == DiffSide::Right && line == 220 {
                assert_eq!(diff_scroll(cx, app).logical_scroll_top().item_ix, row);
            }
            switch(cx, window, true);
            // Fresh paint intersects the current native clip, not merely the
            // backing row vector or a previously measured geometry cache.
            painted(cx, window, &badge(row, side));
            let wrong_side = match side {
                DiffSide::Left => DiffSide::Right,
                DiffSide::Right => DiffSide::Left,
            };
            assert!(
                bounds(cx, window, &badge(row, wrong_side)).is_none(),
                "current {side:?} thread cannot move to the other gutter"
            );
            assert_copy_focus(cx, window, &focus, &copied);
            switch(cx, window, false);
            painted(cx, window, &badge(row, side));
            assert_copy_focus(cx, window, &focus, &copied);
        }
    });
}

pub fn scenario_pr_threads_mode_anchor_expansion(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, _, _, _| {
        let left = source_row(cx, app, DiffSide::Left, 220);
        let right = source_row(cx, app, DiffSide::Right, 220);
        assert_eq!(coordinates(cx, app, left), (Some(220), None));
        assert_eq!(coordinates(cx, app, right), (None, Some(220)));
        reveal(cx, app, window, &badge(left, DiffSide::Left), true);
        click(cx, window, &badge(left, DiffSide::Left));
        // Open the second member through the existing product action; the
        // first card may already put this badge below the native viewport.
        app.update(cx, |app, cx| app.pr_mode_toggle_thread(right, cx));
        let left_body = format!("pr-thread-body-{left}-0-0");
        let right_body = format!("pr-thread-body-{right}-0-0");
        reveal(cx, app, window, &left_body, true);
        align(cx, app, window, &left_body, -8.);
        let viewport = diff_scroll(cx, app).viewport_bounds();
        let card = bounds(cx, window, &format!("pr-thread-{left}-0")).expect("open left card");
        assert!(
            card.top() < viewport.top(),
            "reading anchor is inside Expansion, not its code Row"
        );
        switch(cx, window, true);
        painted(cx, window, &left_body);
        switch(cx, window, false);
        // A combined expansion deterministically chooses its first open
        // source row (left, then right). Do not promise sub-card pixel identity.
        painted(cx, window, &left_body);

        // Leave both expansions open and genuinely offscreen while the true
        // tail crosses both modes. Returning must paint the current bodies.
        let tail = source_row(cx, app, DiffSide::Right, 498);
        reveal(cx, app, window, &badge(tail, DiffSide::Right), true);
        align(cx, app, window, &badge(tail, DiffSide::Right), -2.);
        assert!(bounds(cx, window, &left_body).is_none());
        switch(cx, window, true);
        painted(cx, window, &badge(tail, DiffSide::Right));
        assert!(bounds(cx, window, &left_body).is_none());
        switch(cx, window, false);
        painted(cx, window, &badge(tail, DiffSide::Right));
        reveal(cx, app, window, &left_body, false);
        painted(cx, window, &left_body);
        reveal(cx, app, window, &right_body, true);
        painted(cx, window, &right_body);
        reveal(cx, app, window, &badge(left, DiffSide::Left), false);
        switch(cx, window, true);
        let left_badge = painted(cx, window, &badge(left, DiffSide::Left));
        let right_badge = painted(cx, window, &badge(right, DiffSide::Right));
        assert!(left_badge.right() < right_badge.left());
        assert!(
            (left_badge.top() - right_badge.top()).abs() < gpui::px(1.),
            "paired base/head notes belong to the same actual split code row"
        );
        // Collapse LEFT only; the RIGHT current body must remain open.
        click(cx, window, &badge(left, DiffSide::Left));
        assert!(bounds(cx, window, &left_body).is_none());
        reveal(cx, app, window, &right_body, true);
        painted(cx, window, &right_body);
    });
}

fn assert_start(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let top = diff_scroll(cx, app).logical_scroll_top();
    assert_eq!(
        top.item_ix, 0,
        "a different source must not inherit the retired reading anchor"
    );
    assert_eq!(top.offset_in_item, gpui::px(0.));
}

pub fn scenario_pr_threads_mode_anchor_replacement(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, repo, remote, _| {
        let tail = source_row(cx, app, DiffSide::Right, 498);
        reveal(cx, app, window, &badge(tail, DiffSide::Right), true);
        app.update(cx, |app, cx| app.pr_mode_select_file(1, cx));
        paint(cx, window);
        assert_start(cx, app);
        let first = source_row(cx, app, DiffSide::Right, 40);
        painted(cx, window, &badge(first, DiffSide::Right));
        click(cx, window, &badge(first, DiffSide::Right));
        let other_body = format!("pr-thread-body-{first}-0-0");
        painted(cx, window, &other_body);
        cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            let tab = &mode.tabs[mode.active.unwrap()];
            assert_eq!(
                tab.files[tab.selected_file.unwrap()].path,
                std::path::Path::new("d.txt")
            );
            assert!(tab
                .line_comments
                .iter()
                .any(|comment| comment.body == OTHER_BODY));
        });
        switch(cx, window, true);
        painted(cx, window, &other_body);
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        assert_start(cx, app);
        assert!(
            bounds(cx, window, &other_body).is_none(),
            "another path's expansion cannot leak into c.txt"
        );

        let old_count = counts(cx, app).0;
        reveal(cx, app, window, &badge(tail, DiffSide::Right), true);
        // Same path/count, real newly fetched Git head: a source replacement,
        // unlike a mode transfer, must start at the beginning.
        let text = anchor_text(41);
        let head = push_pr_head(repo, remote, "main", &[("c.txt", &text), ("d.txt", &text)]);
        let pr = pr_at(&head);
        e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
            vec![pr],
            "",
            None,
        ))));
        app.update(cx, |app, cx| app.refresh_github_prs(cx));
        wait_loaded(cx, app, &head);
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        assert_eq!(
            counts(cx, app).0,
            old_count,
            "replacement intentionally has equal row count"
        );
        assert_start(cx, app);
        let first = source_row(cx, app, DiffSide::Right, 40);
        assert_eq!(
            coordinates(cx, app, first),
            (Some(40), Some(40)),
            "new Git source really relocated the first replacement"
        );
        painted(cx, window, &badge(first, DiffSide::Right));
        switch(cx, window, false);
        painted(cx, window, &badge(first, DiffSide::Right));
        assert_start(cx, app);
    });
}
