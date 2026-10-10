//! #1121: repeated PR navigation that selects the same actual Git source must
//! not discard the native diff's reading position or measured row geometry.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{paint, pr_at, push_pr_head, rev, wait_loaded};
use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext};
use kagi::ui::{e2e, DiffRow, KagiApp};
use kagi_ui_core::theme;

const FILE: &str = "same-selection.txt";
const LINES: usize = 600;
const GH: &str = r#"#!/bin/sh
case "$1" in
 repo) echo 'example/repo' ;;
 pr) echo '{"reviews":[],"comments":[]}' ;;
 api)
  q=""
  for a in "$@"; do case "$a" in query=*) q="${a#query=}" ;; esac; done
  case "$q" in
   *mergeStateStatus*) echo '{"data":{"repository":{"pullRequest":{"id":"PR_same_selection","mergeStateStatus":"CLEAN","reviewThreads":{"nodes":[]},"mergeQueueEntry":null}}}}' ;;
   *reviewThreads*) echo '{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}' ;;
   *) echo 'unexpected same-selection query' >&2; exit 1 ;;
  esac ;;
 *) echo 'unexpected same-selection command' >&2; exit 1 ;;
esac
"#;

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
fn fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    std::path::PathBuf,
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
    git(
        &repo,
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    let text: String = (1..=LINES)
        .map(|line| format!("same-selection-data-{line:04}\n"))
        .collect();
    let head = push_pr_head(&repo, &remote, "main", &[(FILE, &text)]);
    (fixture, remote_dir, repo, base, head)
}
fn scroll(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> gpui::ListState {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        mode.tabs[mode.active.expect("active PR")]
            .diff_scroll
            .clone()
    })
}
fn wheel(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle, dy: f32) {
    let viewport = scroll(cx, app).viewport_bounds();
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
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    e2e::clear_control_bounds(window.window_id(), name);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} must be drawn"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    paint(cx, window);
}
fn data(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &std::path::Path,
    base: &str,
    head: &str,
) -> Vec<(String, Option<u32>)> {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.repo_path.as_deref(), Some(repo));
        let mode = app.pr_mode().unwrap();
        let tab = &mode.tabs[mode.active.unwrap()];
        assert_eq!(tab.pr.number, 7);
        assert_eq!(tab.base.0, base);
        assert_eq!(tab.head.0, head);
        assert_eq!(tab.selected_commit, None);
        assert_eq!(tab.selected_file, Some(0));
        assert_eq!(
            tab.files.len(),
            1,
            "one actual file is both first and last; All changes keeps that source"
        );
        assert_eq!(tab.files[0].path, std::path::Path::new(FILE));
        let diff = tab.diff.as_ref().unwrap();
        assert_eq!(diff.rows.len(), LINES + 1);
        diff.rows
            .iter()
            .map(|row| match row {
                DiffRow::HunkHeader(text) => (text.to_string(), None),
                DiffRow::Line {
                    text, new_lineno, ..
                } => (text.to_string(), *new_lineno),
                DiffRow::Binary => panic!("actual text file must not be binary"),
            })
            .collect()
    })
}

pub fn scenario_pr_diff_extent_same_selection(cx: &mut VisualTestAppContext) {
    let _restore = Restore {
        split: theme::diff_split(),
        zoom: theme::zoom(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["diff_split", "ui_zoom"]),
    };
    theme::set_diff_split(false);
    theme::set_zoom(1.);
    let _gh = OfflineGh::with_script(GH);
    let (_fixture, _remote, repo, base, head) = fixture();
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&head), cx));
        wait_loaded(cx, &app, &head);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            paint(cx, window);
            let loaded = cx.read(|cx| {
                let mode = app.read(cx).pr_mode().unwrap();
                let tab = &mode.tabs[mode.active.unwrap()];
                tab.conversation_loaded && tab.merge_status_loaded
            });
            if loaded {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "same-selection PR detail did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        let expected = data(cx, &app, &repo, &base, &head);
        let mut observations = Vec::new();
        for entry in ["same file click", "j at last file", "All changes click"] {
            // Restore an explicit reading intent with native wheels between
            // entries so Before can record all three independently.
            wheel(cx, &app, window, -20_000_000.);
            assert!(
                scroll(cx, &app).bounds_for_item(LINES).is_some(),
                "actual distant row must be measured"
            );
            wheel(cx, &app, window, 20_000_000.);
            wheel(cx, &app, window, -900.);
            let native = scroll(cx, &app);
            let before = native.logical_scroll_top();
            let reading = native
                .bounds_for_item(before.item_ix)
                .expect("native reading row");
            let distant = native
                .bounds_for_item(LINES)
                .expect("known distant geometry");
            assert!(
                before.item_ix > 1 && before.item_ix + 20 < LINES,
                "wheel must establish interior reading intent"
            );
            match entry {
                "same file click" => click(cx, window, "pr-mode-file-0"),
                "j at last file" => {
                    cx.update_window(window, |_, window, cx| {
                        let root = app.read(cx).root_focus.clone().expect("PR root focus");
                        root.focus(window, cx);
                    })
                    .unwrap();
                    crate::keyboard_nav::keys(cx, window, "j");
                    paint(cx, window);
                }
                "All changes click" => {
                    // The real Commits tab has no existing bounds recorder;
                    // use its normal product transition to expose the header.
                    app.update(cx, |app, cx| {
                        app.pr_mode_show(kagi::ui::pr_mode::PrView::Commits, cx)
                    });
                    paint(cx, window);
                    click(cx, window, "pr-mode-commits-all");
                }
                _ => unreachable!(),
            }
            assert_eq!(
                data(cx, &app, &repo, &base, &head),
                expected,
                "{entry} must retain actual Git source/content/line IDs"
            );
            let live = scroll(cx, &app);
            let after = live.logical_scroll_top();
            let reading_retained = after.item_ix == before.item_ix
                && (after.offset_in_item - before.offset_in_item).abs() < gpui::px(1.);
            let geometry_retained = live.bounds_for_item(before.item_ix).is_some_and(|bounds| {
                (bounds.top() - reading.top()).abs() < gpui::px(1.)
                    && (bounds.size.height - reading.size.height).abs() < gpui::px(1.)
            }) && live.bounds_for_item(LINES).is_some_and(|bounds| {
                (bounds.size.height - distant.size.height).abs() < gpui::px(1.)
            });
            observations.push((entry, before, after, reading_retained, geometry_retained));
        }
        observations
    }));
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire same-selection PR root");
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    let observations = match result {
        Ok(observations) => observations,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    for (entry, before, after, reading_retained, geometry_retained) in observations {
        assert!(reading_retained && geometry_retained, "same-selection {entry} discarded native reading/measurement: before={before:?} after={after:?} reading_retained={reading_retained} geometry_retained={geometry_retained}");
    }
}
