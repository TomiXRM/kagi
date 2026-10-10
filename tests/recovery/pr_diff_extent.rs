//! #1121: a native wheel must traverse the actual PR diff, not its measured prefix.
//! The same first/tail and initial global-extent assertions run before and after
//! the production fix. No test installs rows, height hints, or scroll offsets.

use gpui::{AnyWindowHandle, AppContext, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{DiffRow, KagiApp};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{paint, pr_at, push_pr_head, rev, wait_loaded};

const LINES: usize = 20_005;
const FILE: &str = "extent.txt";
const FIRST: &str = "FIRST_EXTENT_SENTINEL";
const TAIL: &str = "TAIL_EXTENT_SENTINEL";

// Finite offline transport, but the production gh invocation and JSON parsers
// still execute. Git fetches the PR's real ref from an owned bare repository.
const GH: &str = r#"#!/bin/sh
case "$1" in
  repo) echo 'example/repo' ;;
  pr) echo '{"reviews":[],"comments":[]}' ;;
  api)
    q=""
    for a in "$@"; do case "$a" in query=*) q="${a#query=}" ;; esac; done
    case "$q" in
      *mergeStateStatus*)
        echo '{"data":{"repository":{"pullRequest":{"id":"PR_extent","mergeStateStatus":"CLEAN","reviewThreads":{"nodes":[]},"mergeQueueEntry":null}}}}' ;;
      *reviewThreads*)
        echo '{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}' ;;
      *) echo 'unexpected extent fixture query' >&2; exit 1 ;;
    esac ;;
  *) echo 'unexpected extent fixture command' >&2; exit 1 ;;
esac
"#;

fn fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    tempfile::TempDir,
    String,
    String,
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let base = rev(&repo, "HEAD");
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
    let local = format!("file://{}", remote.display());
    git(&repo, &["config", &format!("url.{local}.insteadOf"), url]);
    let mut text = String::with_capacity(LINES * 24);
    for line in 1..=LINES {
        match line {
            1 => text.push_str(FIRST),
            LINES => text.push_str(TAIL),
            _ => text.push_str(&format!("extent-data-{line:05}")),
        }
        text.push('\n');
    }
    let head = push_pr_head(&repo, &remote, "main", &[(FILE, &text)]);
    (fixture, repo, remote_dir, base, head)
}

fn wait_conversation(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let done = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().expect("PR mode");
            let tab = &mode.tabs[mode.active.expect("active PR")];
            tab.conversation_loaded && tab.merge_status_loaded
        });
        if done {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "extent PR detail did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        let tab = &mode.tabs[mode.active.unwrap()];
        assert!(tab.reviews.is_empty() && tab.comments.is_empty() && tab.line_comments.is_empty());
        assert_eq!(
            tab.merge_status
                .as_ref()
                .map(|status| format!("{:?}", status.state)),
            Some("Clean".to_string()),
            "the offline response must pass the real merge-status parser"
        );
    });
}

#[derive(Debug)]
struct Observation {
    viewport: Bounds<Pixels>,
    first: Option<Bounds<Pixels>>,
    tail: Option<Bounds<Pixels>>,
    top: usize,
    extent: f32,
}

// Read only the live consumer's native state, after a fresh native draw.
fn observe(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Observation {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = &mode.tabs[mode.active.expect("active PR")];
        let scroll = &tab.diff_scroll;
        let viewport = scroll.viewport_bounds();
        Observation {
            viewport,
            first: scroll.bounds_for_item(1),
            tail: scroll.bounds_for_item(LINES),
            top: scroll.logical_scroll_top().item_ix,
            extent: f32::from(scroll.max_offset_for_scrollbar().y + viewport.size.height),
        }
    })
}

fn inside(row: Option<Bounds<Pixels>>, viewport: Bounds<Pixels>) -> bool {
    row.is_some_and(|row| {
        row.size.height > gpui::px(0.)
            && row.top() >= viewport.top() - gpui::px(1.)
            && row.bottom() <= viewport.bottom() + gpui::px(1.)
    })
}

fn wheel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    viewport: Bounds<Pixels>,
    dy: f32,
) {
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    paint(cx, window);
}

fn assert_data(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &std::path::Path,
    base: &str,
    head: &str,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.repo_path.as_deref(),
            Some(repo),
            "wheel must keep the repository owner"
        );
        let mode = app.pr_mode().expect("PR mode");
        let tab = &mode.tabs[mode.active.expect("active PR")];
        assert_eq!(tab.pr.number, 7);
        assert_eq!(tab.base.0, base);
        assert_eq!(tab.head.0, head);
        assert_eq!(tab.selected_commit, None);
        assert_eq!(tab.selected_file, Some(0));
        assert_eq!(tab.files.len(), 1);
        assert_eq!(tab.files[0].path, std::path::Path::new(FILE));
        let diff = tab.diff.as_ref().expect("actual selected-file diff");
        assert_eq!(diff.title.as_ref(), FILE);
        assert_eq!(
            diff.rows.len(),
            LINES + 1,
            "one actual hunk plus all Git data lines"
        );
        assert_eq!(tab.diff_scroll.item_count(), LINES + 1);
        assert!(matches!(&diff.rows[0], DiffRow::HunkHeader(_)));
        for index in [1, LINES] {
            match &diff.rows[index] {
                DiffRow::Line {
                    kind,
                    old_lineno,
                    new_lineno,
                    ..
                } => {
                    assert_eq!(*kind, kagi_domain::diff::DiffLineKind::Added);
                    assert_eq!(*old_lineno, None);
                    assert_eq!(*new_lineno, Some(index as u32));
                }
                _ => panic!("sentinel must be actual Git content at row {index}"),
            }
        }
    });
}

// SavedKeys restores the persisted setting; this guard also restores the
// live atomic on setup, assertion, or teardown panic.
struct LiveSplit(bool);

impl Drop for LiveSplit {
    fn drop(&mut self) {
        theme::set_diff_split(self.0);
    }
}

fn run(cx: &mut VisualTestAppContext, split: bool) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let _live_split = LiveSplit(theme::diff_split());
    theme::set_diff_split(split);
    let _gh = OfflineGh::with_script(GH);
    let (_fixture, repo, _remote, base, head) = fixture();
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&head), cx));
        wait_loaded(cx, &app, &head);
        wait_conversation(cx, &app);
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        assert_data(cx, &app, &repo, &base, &head);
        let initial = observe(cx, &app);
        assert!(
            initial.viewport.size.height > gpui::px(100.),
            "native diff must have a real viewport: {initial:?}"
        );
        assert!(
            inside(initial.first, initial.viewport),
            "first sentinel must initially be fully visible: {initial:?}"
        );
        assert!(
            !inside(initial.tail, initial.viewport),
            "the fixture must require scrolling"
        );
        let row_height = f32::from(initial.first.unwrap().size.height);

        // A small ordinary wheel proves that the event reaches this consumer.
        wheel(cx, window, initial.viewport, -100.);
        let small = observe(cx, &app);
        assert!(
            small.top > initial.top,
            "native wheel did not move the PR diff: {small:?}"
        );
        // This is deliberately one event, not thousands of prefix-measuring events.
        // It exceeds the entire file's plausible extent in either display mode.
        wheel(cx, window, small.viewport, -20_000_000.);
        let bottom = observe(cx, &app);
        assert_data(cx, &app, &repo, &base, &head);
        // Record both wheel directions before the shared retirement path.
        wheel(cx, window, bottom.viewport, 20_000_000.);
        let returned = observe(cx, &app);
        (initial, small, bottom, returned, row_height)
    }));
    // PR descendants include InputState text elements whose on_next_frame
    // callbacks retain leases. Retire their tree while the display link is
    // live, drain native frames, and only then remove the window.
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire extent PR root");
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    let (initial, small, bottom, returned, row_height) = match result {
        Ok(observation) => observation,
        Err(panic) => std::panic::resume_unwind(panic),
    };

    eprintln!("[gui-e2e] pr_diff_extent split={split}: initial={initial:?}; small={small:?}; bottom={bottom:?}; returned={returned:?}");
    assert!(
        inside(bottom.tail, bottom.viewport),
        "#1121 native wheel clamps to measured prefix: actual Git tail row {LINES} is not fully inside fresh viewport; initial={initial:?}; small={small:?}; bottom={bottom:?}"
    );
    assert!(
        inside(returned.first, returned.viewport),
        "upward wheel must return the same first sentinel: {returned:?}"
    );
    // A global estimate is allowed; a measured viewport/prefix is not. This
    // short-line fixture's data rows have identical actual geometry. The broad
    // interval tolerates the hunk header without hard-coding a theme/font px.
    let expected = LINES as f32 * row_height;
    assert!(
        (0.8 * expected..=1.2 * expected).contains(&initial.extent),
        "initial scrollbar extent must represent all {LINES} actual data lines, not only laid-out prefix: extent={}, measured-row-height={row_height}, expected~{expected}",
        initial.extent
    );
}

pub fn scenario_pr_diff_extent_unified(cx: &mut VisualTestAppContext) {
    run(cx, false);
}

pub fn scenario_pr_diff_extent_split(cx: &mut VisualTestAppContext) {
    run(cx, true);
}
