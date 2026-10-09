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

/// A repo whose PR #7 (a real `refs/pull/7/head` behind `insteadOf`) rewrites
/// line 2 of `c.txt`. Returns (repo dir guard, repo, remote dir guard, head).
fn c_txt_pr() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    tempfile::TempDir,
    String,
) {
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
    (fixture, repo, remote_dir, head)
}

/// A `gh` that answers the review-thread query with two threads on `c.txt`
/// and the merge-status query with a CLEAN status and one open thread — but,
/// like GitHub, rejects a query in which any field the parser reads is not
/// asked for by its own name (#837 / #843 Tier B: two names glued into one).
const THREADS_GH: &str = r#"#!/bin/sh
case "$1" in
  pr) echo '{"reviews":[],"comments":[]}' ;;
  repo) echo 'example/repo' ;;
  api)
    q=""
    for a in "$@"; do case "$a" in query=*) q="${a#query=}" ;; esac; done
    case "$q" in *mergeStateStatus*)
      q=" $q "
      for f in id mergeStateStatus reviewThreads nodes isResolved mergeQueueEntry position \
               estimatedTimeToMerge state mergeQueue nextEntryEstimatedTimeToMerge; do
        case "$q" in
          *[!A-Za-z0-9_]"$f"[!A-Za-z0-9_]*) ;;
          *) echo "{\"errors\":[{\"message\":\"Field '$f' is not requested\"}]}"; exit 1 ;;
        esac
      done
      echo '{"data":{"repository":{"pullRequest":{"id":"PR_1","mergeStateStatus":"CLEAN","reviewThreads":{"nodes":[{"isResolved":false}]},"mergeQueueEntry":null}}}}'
      exit 0 ;;
    esac
    q=" $q "
    for f in path line startLine originalLine diffSide isOutdated isResolved \
             viewerCanResolve comments databaseId author login body createdAt diffHunk replyTo; do
      case "$q" in
        *[!A-Za-z0-9_]"$f"[!A-Za-z0-9_]*) ;;
        *) echo "{\"errors\":[{\"message\":\"Field '$f' is not requested\"}]}"; exit 1 ;;
      esac
    done
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
 {"path":"c.txt","line":2,"startLine":null,"originalLine":2,"diffSide":"RIGHT","isOutdated":false,"isResolved":false,"viewerCanResolve":true,
  "comments":{"nodes":[{"databaseId":1,"author":{"login":"alice"},"body":"right side","createdAt":"t1","diffHunk":"","replyTo":null}]}},
 {"path":"c.txt","line":2,"startLine":null,"originalLine":2,"diffSide":"LEFT","isOutdated":false,"isResolved":false,"viewerCanResolve":true,
  "comments":{"nodes":[{"databaseId":2,"author":{"login":"bob"},"body":"left side","createdAt":"t2","diffHunk":"","replyTo":null}]}}
]}}}}}
JSON
    ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#;

/// #837 Tier B: the production read path — the real `pr_review_threads` gh
/// invocation and parse, not a queued result. When `gh` answers two threads,
/// the tab holds two and both rows are badged; zero would be the old silent
/// failure.
pub fn scenario_pr_threads_via_gh(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let _gh = OfflineGh::with_script(THREADS_GH);
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let (_fixture, repo, _remote, head) = c_txt_pr();
    let (app, window) = mount(cx, &repo);
    let pr = pr_at(&head);
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_loaded(cx, &app, &head);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let loaded = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().expect("PR mode");
            mode.active
                .and_then(|i| mode.tabs.get(i))
                .is_some_and(|tab| tab.conversation_loaded && tab.merge_status_loaded)
        });
        if loaded {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "conversation never loaded"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let feed = cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode
            .active
            .and_then(|i| mode.tabs.get(i))
            .expect("a PR tab");
        tab.line_comments.len()
    });
    assert_eq!(
        feed, 2,
        "gh answered two threads; they must not read as none"
    );
    // #843: the merge-status read through the same gh answers, not nothing.
    let merge = cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode.active.and_then(|i| mode.tabs.get(i)).expect("tab");
        tab.merge_status
            .as_ref()
            .map(|s| (format!("{:?}", s.state), s.unresolved_threads))
    });
    assert_eq!(
        merge,
        Some(("Clean".to_string(), 1)),
        "gh answered a merge status; it must not read as none"
    );
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    for badge in ["pr-thread-badge-2", "pr-thread-badge-3"] {
        assert!(
            bounds(cx, window, badge).is_some(),
            "{badge}: gh's thread is placed"
        );
    }
    theme::set_diff_split(split_before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_threads_via_gh: the real gh read yields both threads");
}

pub fn scenario_pr_threads(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let _gh = OfflineGh::install();
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let (_fixture, repo, _remote, head) = c_txt_pr();

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

/// Eight separated hunks leave enough real rows to exercise vertical scrolling.
/// Moving only the first edit from 40 to 41 preserves the hunk/row counts,
/// but moves the notes on line 40 from removed/added rows to a context row.
fn projection_text(first_edit: u32) -> String {
    (1..=500)
        .map(|line| {
            if [first_edit, 100, 160, 220, 280, 340, 400, 460].contains(&line) {
                format!("changed {line}\n")
            } else {
                format!("line {line}\n")
            }
        })
        .collect()
}

fn projection_fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    tempfile::TempDir,
    std::path::PathBuf,
    String,
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let base: String = (1..=500).map(|line| format!("line {line}\n")).collect();
    for path in ["c.txt", "d.txt"] {
        std::fs::write(repo.join(path), &base).unwrap();
    }
    git(&repo, &["add", "c.txt", "d.txt"]);
    git(&repo, &["commit", "-q", "-m", "overlay coordinates base"]);
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
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/repo.git",
        ],
    );
    // Identity admission reads remote.<name>.url, not the insteadOf transport.
    git(
        &repo,
        &[
            "remote",
            "add",
            "other",
            "https://github.com/other/repo.git",
        ],
    );
    let file_url = format!("file://{}", remote.display());
    for url in [
        "https://github.com/example/repo.git",
        "https://github.com/other/repo.git",
    ] {
        git(
            &repo,
            &["config", "--add", &format!("url.{file_url}.insteadOf"), url],
        );
    }
    let text = projection_text(40);
    let head = push_pr_head(
        &repo,
        &remote,
        "main",
        &[("c.txt", &text), ("d.txt", &text)],
    );
    (fixture, repo, remote_dir, remote, head)
}

fn projection_notes(body: &str, right_side: DiffSide) -> Vec<ReviewThread> {
    let mut other_file = thread(DiffSide::Right, Some(40), 40, "d.txt only");
    other_file.path = "d.txt".into();
    vec![
        thread(DiffSide::Left, Some(40), 40, "base note"),
        thread(right_side, Some(40), 40, body),
        thread(DiffSide::Right, Some(400), 400, "lower anchor"),
        other_file,
    ]
}

fn queue_notes(notes: Vec<ReviewThread>) {
    e2e::queue_github_pr_conversation(gpui::Task::ready((Ok((Vec::new(), Vec::new())), Ok(notes))));
}

fn wait_note_body(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, body: &str, count: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let loaded = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().expect("PR mode");
            let tab = mode
                .active
                .and_then(|ix| mode.tabs.get(ix))
                .expect("active PR");
            tab.conversation_loaded
                && tab.line_comments.len() == count
                && tab.line_comments.iter().any(|comment| comment.body == body)
        });
        if loaded {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "replacement note bytes never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn coordinates(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    row: usize,
) -> (Option<u32>, Option<u32>) {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode
            .active
            .and_then(|ix| mode.tabs.get(ix))
            .expect("active PR");
        match &tab.diff.as_ref().expect("file diff").rows[row] {
            kagi::ui::diff_view::DiffRow::Line {
                old_lineno,
                new_lineno,
                ..
            } => (*old_lineno, *new_lineno),
            _ => panic!("row {row} is not a numbered diff line"),
        }
    })
}

/// Clear both recorders before drawing: an old layout/paint cannot satisfy this.
fn painted(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    let layout = bounds(cx, window, id).unwrap_or_else(|| panic!("{id}: no current layout"));
    let paint = e2e::control_paint(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id}: no current paint"));
    assert!(
        paint.bounds.size.width > gpui::px(0.)
            && paint.bounds.size.height > gpui::px(0.)
            && paint.bounds.left() < paint.mask.right()
            && paint.bounds.right() > paint.mask.left()
            && paint.bounds.top() < paint.mask.bottom()
            && paint.bounds.bottom() > paint.mask.top(),
        "{id}: paint is empty or clipped away: {paint:?}"
    );
    layout
}

fn diff_scroll(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> gpui::ListState {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        mode.tabs[mode.active.expect("active PR")]
            .diff_scroll
            .clone()
    })
}

fn wheel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    scroll: &gpui::ListState,
    target: &str,
    direction: f32,
) {
    // GPUI clamps each event to the currently measured item heights. A huge
    // single delta therefore cannot reach rows the virtual list has not yet
    // measured. Draw between bounded gestures, stopping on fresh visible paint.
    for _ in 0..scroll.item_count() {
        bounds(cx, window, target);
        if let Some(paint) = e2e::control_paint(window.window_id(), target) {
            if paint.bounds.size.width > gpui::px(0.)
                && paint.bounds.size.height > gpui::px(0.)
                && paint.bounds.left() >= paint.mask.left()
                && paint.bounds.right() <= paint.mask.right()
                && paint.bounds.top() >= paint.mask.top()
                && paint.bounds.bottom() <= paint.mask.bottom()
            {
                return;
            }
        }
        let viewport = bounds(cx, window, "pr-mode-center-pane").expect("PR diff pane");
        let step = viewport.size.height * (direction / 4.);
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), step)),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        paint(cx, window);
    }
    panic!(
        "{target}: bounded native gestures did not reveal the target; scroll={:?}",
        scroll.logical_scroll_top()
    );
}

fn type_draft(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let input = app.pr_comment_input.clone().expect("PR composer");
            input.update(cx, |input, cx| input.replace(text.to_owned(), window, cx));
        });
    })
    .unwrap();
    paint(cx, window);
}

fn composer(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> String {
    cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .as_ref()
            .expect("PR composer")
            .read(cx)
            .value()
            .to_string()
    })
}

/// #1072: native consumers, not projection identity or cache-hit counters.
/// Keep the existing `pr_threads` and `pr_threads_via_gh` oracles unchanged.
pub fn scenario_pr_threads_projection_invalidation(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let _gh = OfflineGh::install();
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let (_fixture, repo, _remote_dir, remote, head) = projection_fixture();
    let (app, window) = mount(cx, &repo);
    queue_notes(projection_notes("short head note", DiffSide::Right));
    let mut pr = pr_at(&head);
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_loaded(cx, &app, &head);
    wait_note_body(cx, &app, "short head note", 4);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    assert_eq!(counts(cx, &app), (72, 72), "eight nine-row hunks");
    assert_eq!(coordinates(cx, &app, 4), (Some(40), None));
    assert_eq!(coordinates(cx, &app, 5), (None, Some(40)));
    painted(cx, window, "pr-thread-badge-4");
    painted(cx, window, "pr-thread-badge-5");
    click(cx, window, "pr-thread-badge-5");
    painted(cx, window, "pr-thread-body-5-0-0");
    click(cx, window, "pr-thread-badge-5");

    // A real new Git head, same path and SAME row count. The PR-list producer
    // accepts it through the normal fetch/install/reload path, on the same tab.
    let text = projection_text(41);
    let head2 = push_pr_head(
        &repo,
        &remote,
        "main",
        &[("c.txt", &text), ("d.txt", &text)],
    );
    pr = pr_at(&head2);
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
        vec![pr.clone()],
        "",
        None,
    ))));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    wait_loaded(cx, &app, &head2);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    painted(cx, window, "pr-thread-badge-3");
    assert_eq!(
        counts(cx, &app),
        (72, 72),
        "same-sized replacement remains closed"
    );
    assert_eq!(coordinates(cx, &app, 3), (Some(40), Some(40)));
    assert_eq!(coordinates(cx, &app, 4), (Some(41), None));
    assert_eq!(coordinates(cx, &app, 5), (None, Some(41)));
    for stale in [
        "pr-thread-badge-4",
        "pr-thread-badge-5",
        "pr-thread-body-5-0-0",
    ] {
        assert!(
            bounds(cx, window, stale).is_none(),
            "{stale}: old coordinate must not survive"
        );
    }
    click(cx, window, "pr-thread-badge-3");
    let short = painted(cx, window, "pr-thread-body-3-1-0");

    // Four notes replace four notes while the same expansion stays open.
    // Five explicit paragraphs give the real markdown body a distinguishable
    // layout; loaded bytes alone would not catch a stale TextView/body.
    const LONG: &str = "new paragraph one\n\nnew paragraph two\n\nnew paragraph three\n\nnew paragraph four\n\nnew paragraph five";
    queue_notes(projection_notes(LONG, DiffSide::Left));
    app.update(cx, e2e::reload_pr_conversation);
    wait_note_body(cx, &app, LONG, 4);
    let long = painted(cx, window, "pr-thread-body-3-1-0");
    assert!(
        long.size.height > short.size.height * 2.,
        "accepted body must replace the short painted body: {short:?} -> {long:?}"
    );
    let old_body = cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        mode.tabs[mode.active.unwrap()]
            .line_comments
            .iter()
            .any(|comment| comment.body == "short head note")
    });
    assert!(!old_body, "the old body bytes are no longer current");
    assert_eq!(counts(cx, &app), (72, 73), "reload preserves the open row");
    theme::set_diff_split(true);
    app.update(cx, |_, cx| cx.notify());
    let badge = painted(cx, window, "pr-thread-badge-3-l");
    let body = painted(cx, window, "pr-thread-body-3-1-0");
    assert!(
        body.top() >= badge.bottom(),
        "split body remains under line 40"
    );
    assert!(
        bounds(cx, window, "pr-thread-badge-3-r").is_none(),
        "reloaded LEFT notes must not retain the old RIGHT marker"
    );
    assert_eq!(
        counts(cx, &app),
        (72, 65),
        "eight split hunks plus one expansion"
    );
    click(cx, window, "pr-thread-badge-3-l");
    assert_eq!(counts(cx, &app).1, 64);
    assert!(bounds(cx, window, "pr-thread-body-3-1-0").is_none());
    click(cx, window, "pr-thread-badge-3-l");

    // Native wheel delivery changes the viewport, not the overlay's anchors.
    let scroll = diff_scroll(cx, &app);
    let top = scroll.logical_scroll_top();
    wheel(cx, window, &scroll, "pr-thread-badge-59-r", -1.);
    let scrolled = scroll.logical_scroll_top();
    assert!(
        scrolled.item_ix > top.item_ix,
        "wheel must actually scroll the diff"
    );
    let lower = painted(cx, window, "pr-thread-badge-59-r");
    assert_eq!(coordinates(cx, &app, 59), (None, Some(400)));
    click(cx, window, "pr-thread-badge-59-r");
    let lower_body = painted(cx, window, "pr-thread-body-59-0-0");
    assert!(
        lower_body.top() >= lower.bottom(),
        "lower note belongs under line 400"
    );
    let opened = scroll.logical_scroll_top();
    assert_eq!(
        opened.item_ix, scrolled.item_ix,
        "opening below the viewport anchor preserves its row"
    );
    assert!((opened.offset_in_item - scrolled.offset_in_item).abs() < gpui::px(1.));
    click(cx, window, "pr-thread-badge-59-r");
    assert_eq!(
        scroll.logical_scroll_top().item_ix,
        scrolled.item_ix,
        "closing restores the same viewport anchor"
    );
    wheel(cx, window, &scroll, "pr-thread-body-3-1-0", 1.);
    painted(cx, window, "pr-thread-body-3-1-0");
    drop(scroll);

    // Another selected path starts closed; opening it cannot leak into c.txt.
    app.update(cx, |app, cx| app.pr_mode_select_file(1, cx));
    painted(cx, window, "pr-thread-badge-3-r");
    assert!(bounds(cx, window, "pr-thread-body-3-1-0").is_none());
    assert_eq!(counts(cx, &app).1, 64);
    click(cx, window, "pr-thread-badge-3-r");
    painted(cx, window, "pr-thread-body-3-0-0");
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    painted(cx, window, "pr-thread-badge-3-l");
    assert!(
        bounds(cx, window, "pr-thread-body-3-0-0").is_none(),
        "other file's open row must not expand this file"
    );
    click(cx, window, "pr-thread-badge-3-l");
    painted(cx, window, "pr-thread-body-3-1-0");
    type_draft(cx, &app, window, "draft for original PR");

    // The same PR number in another base repository is a different owner.
    let mut other = pr.clone();
    other.base_repo = "github.com/other/repo".into();
    other.url = "https://github.com/other/repo/pull/7".into();
    queue_notes(vec![thread(
        DiffSide::Right,
        Some(41),
        41,
        "other PrKey body",
    )]);
    app.update(cx, |app, cx| app.pr_mode_open(&other, cx));
    wait_loaded(cx, &app, &head2);
    wait_note_body(cx, &app, "other PrKey body", 1);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    painted(cx, window, "pr-thread-badge-5-r");
    assert!(bounds(cx, window, "pr-thread-badge-3-l").is_none());
    assert!(bounds(cx, window, "pr-thread-body-3-1-0").is_none());
    assert_eq!(
        composer(cx, &app),
        "",
        "another PrKey must not inherit the draft"
    );
    click(cx, window, "pr-thread-badge-5-r");
    painted(cx, window, "pr-thread-body-5-0-0");
    type_draft(cx, &app, window, "draft for other PR");
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    paint(cx, window);
    painted(cx, window, "pr-thread-body-3-1-0");
    assert!(bounds(cx, window, "pr-thread-badge-5-r").is_none());
    assert_eq!(composer(cx, &app), "draft for original PR");

    theme::set_diff_split(false);
    app.update(cx, |_, cx| cx.notify());
    painted(cx, window, "pr-thread-badge-3");
    painted(cx, window, "pr-thread-body-3-1-0");
    assert_eq!(
        counts(cx, &app),
        (72, 73),
        "unified restores the correct expansion"
    );
    click(cx, window, "pr-thread-badge-3");
    assert_eq!(counts(cx, &app), (72, 72));
    assert!(bounds(cx, window, "pr-thread-body-3-1-0").is_none());

    theme::set_diff_split(split_before);
    // Viewed-checkbox animation tasks must finish before normal root teardown.
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    unmount(cx, app, window);
}

/// Keep the independent same-PrKey/session composer boundary runnable even
/// when a cache assertion fails, and vice versa. The owner oracle is unchanged.
pub fn scenario_pr_threads_projection_session_owner(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let _gh = OfflineGh::install();
    let split_before = theme::diff_split();
    theme::set_diff_split(true);
    let (_fixture, repo, _remote_dir, remote, _) = projection_fixture();
    let text = projection_text(41);
    let head2 = push_pr_head(
        &repo,
        &remote,
        "main",
        &[("c.txt", &text), ("d.txt", &text)],
    );
    let pr = pr_at(&head2);
    let (app, window) = mount(cx, &repo);
    queue_notes(projection_notes("original session body", DiffSide::Left));
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_loaded(cx, &app, &head2);
    wait_note_body(cx, &app, "original session body", 4);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    painted(cx, window, "pr-thread-badge-3-l");
    assert_eq!(coordinates(cx, &app, 3), (Some(40), Some(40)));
    click(cx, window, "pr-thread-badge-3-l");
    painted(cx, window, "pr-thread-body-3-1-0");
    type_draft(cx, &app, window, "draft for original PR");

    // A second real repository session can hold the same PrKey with different
    // coordinates and notes. Returning restores the original session's owner.
    let (_fixture_b, repo_b, _remote_b, _remote_path_b, head_b) = projection_fixture();
    app.update(cx, |app, cx| assert!(app.open_repository(repo_b, cx)));
    cx.run_until_parked();
    queue_notes(vec![thread(
        DiffSide::Right,
        Some(40),
        40,
        "other session body",
    )]);
    let pr_b = pr_at(&head_b);
    app.update(cx, |app, cx| app.pr_mode_open(&pr_b, cx));
    wait_loaded(cx, &app, &head_b);
    wait_note_body(cx, &app, "other session body", 1);
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    painted(cx, window, "pr-thread-badge-5-r");
    assert!(bounds(cx, window, "pr-thread-badge-3-l").is_none());
    assert_eq!(composer(cx, &app), "");
    click(cx, window, "pr-thread-badge-5-r");
    painted(cx, window, "pr-thread-body-5-0-0");
    type_draft(cx, &app, window, "draft for other session");
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    paint(cx, window);
    painted(cx, window, "pr-thread-body-3-1-0");
    assert_eq!(composer(cx, &app), "draft for original PR");
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    paint(cx, window);
    painted(cx, window, "pr-thread-body-5-0-0");
    assert_eq!(composer(cx, &app), "draft for other session");
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    theme::set_diff_split(split_before);
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    unmount(cx, app, window);
}
