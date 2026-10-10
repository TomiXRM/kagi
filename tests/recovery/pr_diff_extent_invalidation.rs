//! #1121: actual PR consumer geometry under reflow, thread expansion and
//! equal-count source replacement. No test writes rows, hints or scroll state.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{paint, pr_at, push_pr_head, rev, wait_loaded};
use gpui::{AnyWindowHandle, AppContext, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::theme;

const LINES: usize = 600;
const SHORT: &str = "a-short.rs";
const WRAPPED: &str = "b-wrapped.rs";

// Both conversation and merge status are read through the production gh parser.
// The current RIGHT thread is on actual added line 8 of the short Git file.
const GH: &str = r#"#!/bin/sh
case "$1" in
 repo) echo 'example/repo' ;;
 pr) echo '{"reviews":[],"comments":[]}' ;;
 api)
  q=""
  for a in "$@"; do case "$a" in query=*) q="${a#query=}" ;; esac; done
  case "$q" in
   *mergeStateStatus*) echo '{"data":{"repository":{"pullRequest":{"id":"PR_extent","mergeStateStatus":"CLEAN","reviewThreads":{"nodes":[{"isResolved":false}]},"mergeQueueEntry":null}}}}' ;;
   *reviewThreads*)
    cat <<'JSON'
{"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"path":"a-short.rs","line":8,"startLine":null,"originalLine":8,"diffSide":"RIGHT","isOutdated":false,"isResolved":false,"viewerCanResolve":true,"comments":{"nodes":[{"databaseId":71,"author":{"login":"reviewer"},"body":"A current review comment with enough ordinary words to wrap across several lines when the PR navigation is widened. This body is real parser data, not a layout placeholder. We need to keep reading the same code while the comment unfolds and while the viewport geometry changes. The comment continues with useful review context about ownership, scrolling, source replacement, and the next actual line of code below the expanded card.","createdAt":"2026-10-01T00:00:00Z","diffHunk":"","replyTo":null}]}}]}}}}}
JSON
    ;;
   *) echo 'unexpected invalidation query' >&2; exit 1 ;;
  esac ;;
 *) echo 'unexpected invalidation command' >&2; exit 1 ;;
esac
"#;

struct Restore {
    split: bool,
    zoom: f32,
    theme: String,
    _saved: crate::gui_isolation::SavedKeys,
}
impl Restore {
    fn capture() -> Self {
        Self {
            split: theme::diff_split(),
            zoom: theme::zoom(),
            theme: theme::theme().slug.to_string(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["diff_split", "ui_zoom", "theme"]),
        }
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_diff_split(self.split);
        theme::set_zoom(self.zoom);
        theme::set_active(&self.theme);
    }
}

struct Fixture {
    _repo: tempfile::TempDir,
    _remote: tempfile::TempDir,
    repo: std::path::PathBuf,
    base: String,
    head: String,
}
fn fixture() -> Fixture {
    let directory = build_fixture();
    let repo = directory.path().canonicalize().unwrap();
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
    let mut short = String::new();
    let mut wrapped = String::new();
    for line in 1..=LINES {
        short.push_str(&format!(
            "let line_{line:04} = {line}; // SHORT_{line:04}\n"
        ));
        wrapped.push_str(&format!(
            "let line_{line:04} = {line}; // WRAPPED_{line:04} {}\n",
            "long actual Git content ".repeat(30)
        ));
    }
    let head = push_pr_head(
        &repo,
        &remote,
        "main",
        &[(SHORT, &short), (WRAPPED, &wrapped)],
    );
    Fixture {
        _repo: directory,
        _remote: remote_dir,
        repo,
        base,
        head,
    }
}

fn with_pr(
    cx: &mut VisualTestAppContext,
    test: impl FnOnce(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle, &Fixture),
) {
    let _restore = Restore::capture();
    theme::set_diff_split(false);
    theme::set_zoom(1.);
    let _gh = OfflineGh::with_script(GH);
    let fixture = fixture();
    let (app, window) = mount(cx, &fixture.repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&fixture.head), cx));
        wait_loaded(cx, &app, &fixture.head);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            cx.run_until_parked();
            if cx.read(|cx| {
                let mode = app.read(cx).pr_mode().unwrap();
                let tab = &mode.tabs[mode.active.unwrap()];
                tab.conversation_loaded && tab.merge_status_loaded
            }) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "invalidation conversation did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
        paint(cx, window);
        assert_owner(cx, &app, &fixture, 0);
        test(cx, &app, window, &fixture);
    }));
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire invalidation PR root");
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn assert_owner(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    fixture: &Fixture,
    file: usize,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.repo_path.as_deref(), Some(fixture.repo.as_path()));
        let mode = app.pr_mode().unwrap();
        let tab = &mode.tabs[mode.active.unwrap()];
        assert_eq!(tab.pr.number, 7);
        assert_eq!(tab.base.0, fixture.base);
        assert_eq!(tab.head.0, fixture.head);
        assert_eq!(tab.selected_commit, None);
        assert_eq!(tab.selected_file, Some(file));
        assert_eq!(
            tab.files[file].path,
            std::path::Path::new([SHORT, WRAPPED][file])
        );
        assert_eq!(tab.diff.as_ref().unwrap().rows.len(), LINES + 1);
        assert_eq!(
            tab.line_comments.len(),
            1,
            "current thread passed actual gh parser"
        );
    });
}

#[derive(Clone, Copy, Debug)]
struct Reading {
    row: usize,
    fraction: f32,
    viewport: Bounds<Pixels>,
    extent: f32,
}
fn reading(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Reading {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        let scroll = &mode.tabs[mode.active.unwrap()].diff_scroll;
        let top = scroll.logical_scroll_top();
        let height = scroll
            .bounds_for_item(top.item_ix)
            .expect("reading row laid out")
            .size
            .height;
        let viewport = scroll.viewport_bounds();
        Reading {
            row: top.item_ix,
            fraction: f32::from(top.offset_in_item) / f32::from(height),
            viewport,
            extent: f32::from(scroll.max_offset_for_scrollbar().y + viewport.size.height),
        }
    })
}
fn row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    index: usize,
) -> Option<Bounds<Pixels>> {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        mode.tabs[mode.active.unwrap()]
            .diff_scroll
            .bounds_for_item(index)
    })
}
fn inside(bounds: Option<Bounds<Pixels>>, viewport: Bounds<Pixels>) -> bool {
    bounds.is_some_and(|bounds| {
        bounds.size.height > gpui::px(0.)
            && bounds.top() >= viewport.top() - gpui::px(1.)
            && bounds.bottom() <= viewport.bottom() + gpui::px(1.)
    })
}
fn wheel(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle, dy: f32) {
    let viewport = reading(cx, app).viewport;
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
fn anchor(before: Reading, after: Reading, reason: &str) {
    assert_eq!(
        after.row, before.row,
        "{reason}: reading row jumped: {before:?} -> {after:?}"
    );
    assert!(
        (after.fraction - before.fraction).abs() < 0.12,
        "{reason}: within-row reading fraction changed: {before:?} -> {after:?}"
    );
}
fn ends(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    tail: usize,
) {
    wheel(cx, app, window, -20_000_000.);
    let viewport = reading(cx, app).viewport;
    let tail_bounds = row(cx, app, tail).expect("true tail must be measured after reflow");
    if tail_bounds.size.height > viewport.size.height {
        assert!(
            (tail_bounds.bottom() - viewport.bottom()).abs() <= gpui::px(1.)
                && tail_bounds.top() < viewport.top(),
            "oversized true tail must bottom-align at the end of the viewport"
        );
    } else {
        assert!(
            inside(Some(tail_bounds), viewport),
            "true tail must be fully inside reflowed viewport"
        );
    }
    wheel(cx, app, window, 20_000_000.);
    let viewport = reading(cx, app).viewport;
    let first = row(cx, app, 1).expect("true first row must be measured after reflow");
    if first.size.height > viewport.size.height {
        let top = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            mode.tabs[mode.active.unwrap()]
                .diff_scroll
                .logical_scroll_top()
        });
        let header = row(cx, app, 0).expect("initial hunk header measured");
        assert!(
            top.item_ix == 0 && top.offset_in_item == gpui::px(0.),
            "oversized first data row must return to the native list start"
        );
        assert!(
            (header.top() - viewport.top()).abs() <= gpui::px(1.)
                && (first.top() - header.bottom()).abs() <= gpui::px(1.)
                && first.bottom() > viewport.bottom(),
            "oversized first data row must begin directly below its top-aligned hunk header"
        );
    } else {
        assert!(
            inside(Some(first), viewport),
            "true first row must return after reflow"
        );
    }
}
fn command(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(id, window, cx));
    })
    .unwrap();
    paint(cx, window);
}
fn zoom_to(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    target: f32,
) {
    for _ in 0..16 {
        let current = theme::zoom();
        if (current - target).abs() < 0.01 {
            return;
        }
        command(
            cx,
            app,
            window,
            if current < target {
                "view.zoomIn"
            } else {
                "view.zoomOut"
            },
        );
    }
    panic!("product zoom steps did not reach {target}");
}
fn control(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> Option<Bounds<Pixels>> {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    e2e::control_bounds(window.window_id(), id)
}
fn thread_revealed(
    card: Bounds<Pixels>,
    body: Bounds<Pixels>,
    badge: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
) -> bool {
    if card.size.height > viewport.size.height - badge.size.height {
        // A tall expansion cannot fit alongside its preceding collapse badge.
        // Reveal the reading start and affordance; leave its bottom scrollable.
        inside(Some(badge), viewport)
            && card.top() >= badge.bottom() - gpui::px(1.)
            && body.top() >= viewport.top()
            && body.top() < viewport.bottom()
    } else {
        inside(Some(card), viewport) && inside(Some(badge), viewport)
    }
}
fn reveal_current_thread(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    // The anchored code row is 8 and its expansion is list item 9. Zoom can
    // put both below the viewport while preserving the unrelated row-0 anchor.
    // Use only real bounded wheel events to reveal card and collapse control.
    for _ in 0..24 {
        let viewport = reading(cx, app).viewport;
        let card = control(cx, window, "pr-thread-8-0");
        let badge = control(cx, window, "pr-thread-badge-8");
        let body = control(cx, window, "pr-thread-body-8-0-0");
        if let (Some(card), Some(body), Some(badge)) = (card, body, badge) {
            if thread_revealed(card, body, badge, viewport) {
                return;
            }
        }
        let above = badge.is_some_and(|bounds| bounds.top() < viewport.top())
            || (card.is_none() && reading(cx, app).row > 9);
        wheel(cx, app, window, if above { 100. } else { -100. });
    }
    panic!("real wheel did not reveal current thread and its collapse badge");
}
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let bounds = control(cx, window, id).unwrap_or_else(|| panic!("{id} must be drawn"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    paint(cx, window);
}
fn widen_navigation(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let left = control(cx, window, "pr-mode-left-pane").expect("PR navigator");
    let y = reading(cx, app).viewport.center().y;
    let start = gpui::point(left.right() + gpui::px(2.), y);
    cx.simulate_mouse_move(window, start, None, gpui::Modifiers::none());
    cx.simulate_mouse_down(
        window,
        start,
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    for delta in [12., 100.] {
        cx.simulate_mouse_move(
            window,
            gpui::point(start.x + gpui::px(delta), y),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        paint(cx, window);
    }
    cx.simulate_mouse_up(
        window,
        gpui::point(start.x + gpui::px(100.), y),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    paint(cx, window);
}

pub fn scenario_pr_diff_extent_reflow(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, fixture| {
        app.update(cx, |app, cx| app.pr_mode_select_file(1, cx));
        paint(cx, window);
        assert_owner(cx, app, fixture, 1);
        let wrapped = row(cx, app, 1).unwrap().size.height;
        wheel(cx, app, window, -1400.);
        let before = reading(cx, app);
        assert!(before.row > 1, "read away from first row before reflow");
        widen_navigation(cx, app, window);
        let narrowed = reading(cx, app);
        assert!(
            f32::from(before.viewport.size.width - narrowed.viewport.size.width) > 30.,
            "actual divider must narrow native diff: {before:?} -> {narrowed:?}"
        );
        anchor(before, narrowed, "width reflow");
        for zoom in [0.7, 1.667] {
            let before = reading(cx, app);
            zoom_to(cx, app, window, zoom);
            anchor(before, reading(cx, app), "product zoom reflow");
            ends(cx, app, window, LINES);
            assert!(
                row(cx, app, 1).unwrap().size.height > gpui::px(20.) * theme::zoom(),
                "actual long Git line must stay wrapped, not clipped to one line"
            );
            wheel(cx, app, window, -1000.);
        }
        assert!(
            wrapped > gpui::px(40.),
            "fixture must render a genuinely multi-line row"
        );
        assert_owner(cx, app, fixture, 1);
    });
}

pub fn scenario_pr_diff_extent_threads(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, fixture| {
        let closed = reading(cx, app);
        let next_closed = row(cx, app, 9).expect("next actual line before expansion");
        click(cx, window, "pr-thread-badge-8");
        let body = control(cx, window, "pr-thread-body-8-0-0").expect("real current thread body");
        let card = control(cx, window, "pr-thread-8-0").expect("current thread card");
        let expanded = reading(cx, app);
        let expansion = row(cx, app, 9).expect("expansion list item measured");
        let next_open = row(cx, app, 10).expect("next actual line after expansion");
        assert!(body.size.height > gpui::px(20.), "actual comment must wrap");
        assert!(
            next_open.top() >= card.bottom(),
            "next code line must move beneath current thread"
        );
        assert!(
            (f32::from(next_open.top() - next_closed.top()) - f32::from(expansion.size.height))
                .abs()
                < 2.,
            "actual next-line displacement equals expansion geometry"
        );
        assert!(
            (expanded.extent - closed.extent - f32::from(expansion.size.height)).abs() < 3.,
            "global extent must absorb actual expansion height"
        );
        let before = reading(cx, app);
        widen_navigation(cx, app, window);
        let narrowed = reading(cx, app);
        assert!(
            narrowed.viewport.size.width < before.viewport.size.width - gpui::px(30.),
            "thread remeasure requires actual width change"
        );
        anchor(before, narrowed, "open-thread width reflow");
        let narrowed_body =
            control(cx, window, "pr-thread-body-8-0-0").expect("body after width reflow");
        assert!(
            narrowed_body.size.width < body.size.width
                && narrowed_body.size.height >= body.size.height,
            "initial narrowing must reduce body width without losing comment height"
        );
        // A 100px change can keep the same discrete wrap count. The product
        // zoom step also enlarges sibling panes, producing a decisive narrower
        // comment column without changing its content or synthetic geometry.
        let before_zoom = reading(cx, app);
        zoom_to(cx, app, window, 1.667);
        anchor(
            before_zoom,
            reading(cx, app),
            "open-thread product zoom reflow",
        );
        reveal_current_thread(cx, app, window);
        let resized_body =
            control(cx, window, "pr-thread-body-8-0-0").expect("body after decisive reflow");
        let resized_card =
            control(cx, window, "pr-thread-8-0").expect("card after decisive reflow");
        assert!(
            resized_body.size.width < body.size.width / 2.,
            "decisive product reflow must reduce actual body width below half"
        );
        assert!(
            resized_body.size.height > body.size.height,
            "decisively narrower actual comment must reflow to a taller body"
        );
        assert!(
            resized_body.left() >= resized_card.left()
                && resized_body.right() <= resized_card.right()
                && resized_body.top() >= resized_card.top()
                && resized_body.bottom() <= resized_card.bottom(),
            "reflowed card must contain the complete measured comment body"
        );
        let resized_badge =
            control(cx, window, "pr-thread-badge-8").expect("collapse badge after decisive reflow");
        assert!(thread_revealed(resized_card, resized_body, resized_badge, reading(cx, app).viewport), "reflowed thread must reveal its body start and collapse badge, with full containment when it fits");
        // Measure a distant code row, return above it, then ensure unchanged
        // renders retain its actual geometry rather than clearing every item.
        wheel(cx, app, window, -3000.);
        let distant_index = reading(cx, app).row + 2;
        let distant = row(cx, app, distant_index).expect("distant actual code row measured");
        wheel(cx, app, window, 20_000_000.);
        let stable = reading(cx, app);
        for _ in 0..12 {
            paint(cx, window);
        }
        let warm = reading(cx, app);
        anchor(stable, warm, "unchanged expanded repaint");
        assert!(
            (warm.extent - stable.extent).abs() < 1.,
            "warm renders must not replace measured expansion extent"
        );
        let retained = row(cx, app, distant_index)
            .expect("warm expanded renders must retain offscreen actual measurement");
        assert!(
            (f32::from(retained.size.height - distant.size.height)).abs() < 1.,
            "warm render must not replace a distant measured row with an estimate"
        );
        reveal_current_thread(cx, app, window);
        let collapse_badge =
            control(cx, window, "pr-thread-badge-8").expect("collapse badge drawn");
        let pre_collapse = reading(cx, app);
        let (expanded_count, removed_height) = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            let scroll = &mode.tabs[mode.active.unwrap()].diff_scroll;
            (
                scroll.item_count(),
                scroll
                    .bounds_for_item(9)
                    .expect("expansion measured immediately before collapse")
                    .size
                    .height,
            )
        });
        cx.simulate_click(window, collapse_badge.center(), gpui::Modifiers::none());
        paint(cx, window);
        assert!(
            control(cx, window, "pr-thread-8-0").is_none(),
            "collapsed thread must leave no drawn card"
        );
        let collapsed = reading(cx, app);
        let (collapsed_count, code8_height, code9_height) = cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            let scroll = &mode.tabs[mode.active.unwrap()].diff_scroll;
            (
                scroll.item_count(),
                scroll
                    .bounds_for_item(8)
                    .expect("anchored code row measured after collapse")
                    .size
                    .height,
                scroll
                    .bounds_for_item(9)
                    .expect("next code row measured after collapse")
                    .size
                    .height,
            )
        });
        assert_eq!(
            collapsed_count + 1,
            expanded_count,
            "collapse must remove exactly one expansion item"
        );
        assert!(code9_height < removed_height && (code9_height - code8_height).abs() <= gpui::px(1.), "item 9 after collapse must have actual code-row geometry, not retained expansion height");
        let retired_extent = pre_collapse.extent - collapsed.extent;
        assert!(retired_extent > 0. && retired_extent >= f32::from(removed_height - pre_collapse.viewport.size.height), "collapse must retire expansion extent while allowing at most one viewport of newly exposed actual row measurements");
        ends(cx, app, window, LINES);
        assert_owner(cx, app, fixture, 0);
    });
}

pub fn scenario_pr_diff_extent_source_recolor(cx: &mut VisualTestAppContext) {
    with_pr(cx, |cx, app, window, fixture| {
        let short_height = row(cx, app, 1).unwrap().size.height;
        wheel(cx, app, window, -1800.);
        let before = reading(cx, app);
        assert!(before.row > 1);
        let measured_index = before.row + 2;
        let old = row(cx, app, measured_index).expect("short source actual measured row");
        for _ in 0..12 {
            paint(cx, window);
        }
        anchor(before, reading(cx, app), "unchanged text repaint");
        let other = if theme::theme().slug == "dracula" {
            "tokyo-night"
        } else {
            "dracula"
        };
        app.update(cx, |app, cx| app.set_theme(other, cx));
        let target = theme::theme().key();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            paint(cx, window);
            let ready = cx.read(|cx| {
                app.read(cx)
                    .pr_mode()
                    .and_then(|mode| mode.active.and_then(|index| mode.tabs.get(index)))
                    .and_then(|tab| tab.diff.as_ref())
                    .is_some_and(|diff| diff.highlighted == Some(target))
            });
            if ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "real asynchronous recolor did not reach the selected theme"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        anchor(before, reading(cx, app), "highlight-only theme recolor");
        cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            let diff = mode.tabs[mode.active.unwrap()].diff.as_ref().unwrap();
            assert_eq!(
                diff.highlighted,
                Some(theme::theme().key()),
                "real asynchronous recolor must settle"
            );
        });
        wheel(cx, app, window, 20_000_000.);
        app.update(cx, |app, cx| app.pr_mode_select_file(1, cx));
        paint(cx, window);
        assert_owner(cx, app, fixture, 1);
        let new = row(cx, app, 1).expect("replacement first row actually rendered");
        assert!(
            new.size.height > short_height * 2.,
            "equal-count different Git source must render its wrapped geometry"
        );
        if let Some(cached) = row(cx, app, measured_index) {
            assert!(cached.size.height > old.size.height * 2., "offscreen old source must not supply short measured geometry to equal-count wrapped source");
        }
        ends(cx, app, window, LINES);
        assert_owner(cx, app, fixture, 1);
    });
}
