//! #351 / ADR-0209: review threads on the PR diff.
//!
//! A real PR ref fetch (the `pr_viewed` fixture: bare remote + `insteadOf`)
//! loads `c.txt`, whose line 2 the PR rewrites. The queued conversation
//! carries three threads: RIGHT line 2, LEFT line 2, and an outdated RIGHT
//! thread written on line 3. In the unified diff each lands as a gutter badge
//! on its row; a real click folds the row's thread open directly under it
//! (its body laid out), the outdated one as the dimmed variant, and closing
//! them gives the list back one item per row. In the side-by-side view the
//! LEFT badge sits in the left gutter and the RIGHT one in the right gutter.

use gpui::{AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::ReviewComment;
use kagi_domain::review_thread::{DiffSide, ReviewThread};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{paint, pr_at, push_pr_head, wait_loaded};

fn thread(side: DiffSide, line: Option<u32>, original: u32, body: &str) -> ReviewThread {
    ReviewThread {
        path: "c.txt".into(),
        line,
        original_line: Some(original),
        diff_side: side,
        is_outdated: line.is_none(),
        comments: vec![ReviewComment {
            author: "alice".into(),
            body: body.into(),
            created_at: "2026-10-01T00:00:00Z".into(),
            ..ReviewComment::default()
        }],
        ..ReviewThread::default()
    }
}

fn bounds(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> Option<Bounds<Pixels>> {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    paint(cx, window);
    e2e::control_bounds(window.window_id(), id)
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let at = bounds(cx, window, id).unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_click(window, at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

/// (diff rows, list items) of the active tab's diff.
fn counts(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (usize, usize) {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode
            .active
            .and_then(|i| mode.tabs.get(i))
            .expect("a PR tab");
        let rows = tab.diff.as_ref().expect("a file diff").rows.len();
        (rows, tab.diff_scroll.item_count())
    })
}

pub fn scenario_pr_threads(cx: &mut VisualTestAppContext) {
    let _gh = OfflineGh::install();
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("c.txt"), "c1\nc2\nc3\n").unwrap();
    git(&repo, &["add", "c.txt"]);
    git(&repo, &["commit", "-q", "-m", "base c.txt"]);
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    let file_url = format!("file://{}", remote.display());
    git(
        &repo,
        &["config", &format!("url.{file_url}.insteadOf"), url],
    );
    let head = push_pr_head(&repo, &remote, "main", &[("c.txt", "c1\nC2\nc3\n")]);

    let (app, window) = mount(cx, &repo);
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(vec![
            thread(DiffSide::Right, Some(2), 2, "**rewrite** this line"),
            thread(DiffSide::Left, Some(2), 2, "why was this removed?"),
            thread(DiffSide::Right, None, 3, "an old note"),
        ]),
    )));
    let pr = pr_at(&head);
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_loaded(cx, &app, &head);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    paint(cx, window);

    // Unified rows: hunk, c1 (1,1), -c2 (2,-), +C2 (-,2), c3 (3,3).
    let (rows, items) = counts(cx, &app);
    assert_eq!(
        (rows, items),
        (5, 5),
        "one list item per row while all are closed"
    );
    for badge in [
        "pr-thread-badge-2",
        "pr-thread-badge-3",
        "pr-thread-badge-4",
    ] {
        assert!(
            bounds(cx, window, badge).is_some(),
            "{badge}: a thread's row is marked"
        );
    }
    assert!(
        bounds(cx, window, "pr-thread-badge-1").is_none(),
        "no thread, no badge"
    );

    // Open the RIGHT thread: it folds out directly under its row.
    let badge = bounds(cx, window, "pr-thread-badge-3").unwrap();
    click(cx, window, "pr-thread-badge-3");
    assert_eq!(counts(cx, &app).1, rows + 1, "one expansion item");
    let card = bounds(cx, window, "pr-thread-3-0").expect("the opened thread is drawn");
    assert!(
        card.top() >= badge.bottom(),
        "under its row: {card:?} vs {badge:?}"
    );
    let next_row = bounds(cx, window, "pr-thread-badge-4").expect("row 4 still drawn");
    assert!(
        next_row.top() >= card.bottom(),
        "the next row moves below the thread"
    );
    let body = bounds(cx, window, "pr-thread-body-3-0-0").expect("the comment body is laid out");
    assert!(body.size.height > gpui::px(0.), "{body:?}");

    // The outdated thread opens as the dimmed variant.
    click(cx, window, "pr-thread-badge-4");
    assert_eq!(counts(cx, &app).1, rows + 2);
    assert!(
        bounds(cx, window, "pr-thread-outdated-4-0").is_some(),
        "outdated card drawn"
    );
    assert!(
        bounds(cx, window, "pr-thread-4-0").is_none(),
        "not the current-thread variant"
    );

    // Closing both gives the list back its row numbering.
    click(cx, window, "pr-thread-badge-3");
    click(cx, window, "pr-thread-badge-4");
    assert_eq!(
        counts(cx, &app),
        (rows, rows),
        "closed: one item per row again"
    );
    assert!(bounds(cx, window, "pr-thread-3-0").is_none());

    // Side by side: LEFT in the left gutter, RIGHT in the right gutter.
    theme::set_diff_split(true);
    app.update(cx, |_, cx| cx.notify());
    let left = bounds(cx, window, "pr-thread-badge-2-l").expect("LEFT thread badge");
    let right = bounds(cx, window, "pr-thread-badge-3-r").expect("RIGHT thread badge");
    assert!(
        left.right() < right.left(),
        "left gutter {left:?} before right gutter {right:?}"
    );
    assert!(
        (left.top() - right.top()).abs() < gpui::px(1.),
        "a removed/added pair shares one split row"
    );
    assert!(
        bounds(cx, window, "pr-thread-badge-3-l").is_none(),
        "RIGHT is not on the left"
    );
    // A context row is drawn in both cells; its RIGHT thread only on the right.
    let context = bounds(cx, window, "pr-thread-badge-4-r").expect("RIGHT badge on context");
    assert!(context.left() > left.right(), "{context:?}");
    assert!(
        bounds(cx, window, "pr-thread-badge-4-l").is_none(),
        "a RIGHT thread on a context row is not repeated in the left gutter"
    );

    theme::set_diff_split(split_before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_threads: badges on their rows and sides, open under the row, outdated dimmed, closed restores numbering");
}
