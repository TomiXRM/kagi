//! Main-thread layout regressions; included by gui_e2e_runner, not libtest.
//! Uses the production renderer and GPUI's real text/Taffy pipeline. Natural
//! heights are measured with MaxContent, independently of uniform_list's first
//! row allocation: clipping an oversized row cannot satisfy this oracle.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    canvas, div, px, size, uniform_list, AnyWindowHandle, AvailableSpace, Bounds, Context, Pixels,
    Render, ScrollHandle, Size, UniformListScrollHandle, VisualTestAppContext, Window,
};
use kagi::ui::{e2e, view_models, FooterStatus, KagiApp};
use kagi_domain::file_history::{
    CommitSummary, FileChangeSummary, FileChangeType, FileHistory, FileHistoryEntry,
    FileHistoryEntryKind,
};
use kagi_ui_core::commit_row::{commit_row_model, render_commit_row, CommitRowLayout};
use kagi_ui_core::{i18n, theme};
use kagi_ui_editor::RightPaneTab;

use crate::macos::unmount;

const SIZES: [(f32, f32); 4] = [(320., 240.), (640., 480.), (1024., 768.), (1440., 900.)];
const ZOOMS: [f32; 4] = [0.8, 1., 1.25, 1.5];
const EPS: f32 = 1.; // GPUI snaps layout edges to device pixels.
const NOW: i64 = 1_788_652_800;

fn inputs() -> Vec<(&'static str, String)> {
    vec![
        ("empty", String::new()),
        ("one-character", "A".into()),
        ("ascii-512", "a".repeat(512)),
        ("japanese-512", "界".repeat(512)),
        ("branch", format!("feature/{}", "long-branch-".repeat(64))),
        ("path", format!("src/{}/file.rs", "directory/".repeat(64))),
        ("email", format!("{}@example.invalid", "author".repeat(100))),
        (
            "multiline-message",
            "subject\n\nbody\nsecond paragraph\n末尾".into(),
        ),
        (
            "emoji-combining",
            "\u{1f469}\u{200d}\u{1f4bb}e\u{301}".repeat(128),
        ),
        (
            "url",
            format!("https://example.invalid/{}?q=end", "segment/".repeat(100)),
        ),
        // Characterization from the audit: an ordinary spaced author name.
        ("spaced-author", "Long Author Name ".repeat(40)),
    ]
}

fn entry(text: &str, ix: usize) -> FileHistoryEntry {
    // Git's subject and signature name are single-line. A multiline commit
    // message exercises the real subject/body split, not an impossible author.
    let subject = text.lines().next().unwrap_or("").to_owned();
    FileHistoryEntry {
        kind: FileHistoryEntryKind::Commit,
        commit: Some(CommitSummary {
            full_hash: format!("{ix:040x}"),
            short_hash: format!("{ix:07x}"),
            subject: subject.clone(),
            body: text.split_once('\n').map(|(_, body)| body.to_owned()),
            author_name: subject,
            author_email: "author@example.invalid".into(),
            author_date: "2026-09-05T12:00:00+00:00".into(),
            committer_name: "Committer".into(),
            committer_date: "2026-09-05T12:00:00+00:00".into(),
        }),
        change: FileChangeSummary {
            change_type: FileChangeType::Modified,
            path_before: None,
            path_after: "README.md".into(),
            insertions: Some(2),
            deletions: Some(1),
            is_binary: false,
        },
    }
}

struct Rows {
    entries: Rc<Vec<FileHistoryEntry>>,
    layout: CommitRowLayout,
    selected: usize,
    rows: [ScrollHandle; 3],
    natural: Rc<RefCell<Vec<Size<Pixels>>>>,
    list: UniformListScrollHandle,
}

impl Render for Rows {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(theme::rem_size_px()));
        let entries = self.entries.clone();
        let measure_entries = entries.clone();
        let layout = self.layout;
        let selected = self.selected;
        let handles = self.rows.clone();
        let natural = self.natural.clone();
        div()
            .size_full()
            .font_family(theme::UI_FONT)
            .child(
                uniform_list("recovery-history", 3, move |range, _, _| {
                    range
                        .map(|ix| {
                            render_commit_row(
                                "recovery-row",
                                ix,
                                &commit_row_model(&entries[ix], NOW),
                                layout,
                                ix == selected,
                            )
                            .track_scroll(&handles[ix])
                        })
                        .collect::<Vec<_>>()
                })
                .track_scroll(&self.list)
                .size_full(),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        *natural.borrow_mut() = measure_entries
                            .iter()
                            .enumerate()
                            .map(|(ix, entry)| {
                                render_commit_row(
                                    "recovery-natural",
                                    ix,
                                    &commit_row_model(entry, NOW),
                                    layout,
                                    ix == selected,
                                )
                                .into_any_element()
                                .layout_as_root(
                                    size(
                                        AvailableSpace::Definite(bounds.size.width),
                                        AvailableSpace::MaxContent,
                                    ),
                                    window,
                                    cx,
                                )
                            })
                            .collect();
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

fn finite(bounds: Bounds<Pixels>, label: &str) {
    for value in [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ] {
        assert!(
            f32::from(value).is_finite(),
            "{label}: non-finite {bounds:?}"
        );
    }
    assert!(
        bounds.size.width >= px(0.) && bounds.size.height >= px(0.),
        "{label}: negative {bounds:?}"
    );
}

fn contained(parent: Bounds<Pixels>, child: Bounds<Pixels>, label: &str) {
    finite(parent, label);
    finite(child, label);
    assert!(
        child.left() + px(EPS) >= parent.left()
            && child.top() + px(EPS) >= parent.top()
            && child.right() <= parent.right() + px(EPS)
            && child.bottom() <= parent.bottom() + px(EPS),
        "{label}: child escapes parent: {child:?} outside {parent:?}",
    );
}

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        assert_eq!(
            window.viewport_size(),
            size(px(dimensions.0), px(dimensions.1))
        );
    })
    .expect("draw layout window at its native dimensions");
}

fn inspect(rows: &Rows, dimensions: (f32, f32), label: &str) -> Vec<Bounds<Pixels>> {
    let natural = rows.natural.borrow();
    assert_eq!(natural.len(), 3, "{label}: natural layout was not executed");
    let parent = rows.list.0.borrow().base_handle.bounds();
    assert_eq!(
        parent.size,
        size(px(dimensions.0), px(dimensions.1)),
        "{label}: wrong viewport"
    );
    let bounds = rows
        .rows
        .iter()
        .map(ScrollHandle::bounds)
        .collect::<Vec<_>>();
    for (ix, row) in bounds.iter().copied().enumerate() {
        contained(parent, row, label);
        assert!(row.size.height > px(0.), "{label}: row {ix} not mounted");
        finite(Bounds::new(row.origin, natural[ix]), label);
        assert!(
            (f32::from(natural[ix].height - natural[0].height)).abs() <= EPS,
            "{label}: row {ix} wraps: natural {:?}, short-row {:?}; uniform_list clipping is not a fix",
            natural[ix], natural[0],
        );
        assert!(
            (f32::from(row.size.height - natural[ix].height)).abs() <= EPS,
            "{label}: row {ix} is forced into a smaller slot than its natural content"
        );
        // These are Taffy child bounds, not clipped hitboxes or screenshots.
        let handle = &rows.rows[ix];
        assert!(handle.children_count() > 0, "{label}: missing row content");
        for child in 0..handle.children_count() {
            contained(
                row,
                handle.bounds_for_item(child).expect("row child bounds"),
                label,
            );
        }
    }
    for pair in bounds.windows(2) {
        assert!(
            pair[1].top() + px(EPS) >= pair[0].bottom(),
            "{label}: overlapping rows {pair:?}"
        );
    }
    bounds
}

/// Call from the existing macOS main-thread runner after e2e::init_app.
/// 4 viewports × 4 zooms × 2 locales × 11 inputs × Card/Table, followed by
/// redraw-idempotence checks and selected/unselected transitions.
///
/// One window per viewport, opened once and reused for all 704 cells (#549).
/// It used to mount a fresh native window per cell, twice: 1,408 live
/// `NSWindow`s that `unmount` could not close fast enough, which took down the
/// WindowServer. The pair of mounts existed because `MacWindow::resize`
/// schedules work on the native main queue that `VisualTestAppContext` does not
/// drain — so a viewport still gets its own window, it just gets one.
pub fn scenario_commit_row_layout(cx: &mut VisualTestAppContext) {
    let restore = GlobalSettings::capture();
    let seed = Rc::new(vec![entry("A", 1), entry("B", 2), entry("Z", 3)]);
    let windows: Vec<((f32, f32), gpui::Entity<Rows>, AnyWindowHandle)> = SIZES
        .iter()
        .map(|&dimensions| {
            let rows = cx.new(|_| Rows {
                entries: seed.clone(),
                layout: CommitRowLayout::Card,
                selected: 1,
                rows: std::array::from_fn(|_| ScrollHandle::new()),
                natural: Rc::default(),
                list: UniformListScrollHandle::new(),
            });
            let root = rows.clone();
            let win = crate::macos::open_offscreen(
                cx,
                size(px(dimensions.0), px(dimensions.1)),
                move |_, _| root,
            );
            (dimensions, rows, win.into())
        })
        .collect();
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        for zoom in ZOOMS {
            theme::set_zoom(zoom);
            for layout in [
                CommitRowLayout::Card,
                CommitRowLayout::Table {
                    row_height: 29. * zoom,
                },
            ] {
                for (name, text) in inputs() {
                    let entries = Rc::new(vec![entry("A", 1), entry(&text, 2), entry("Z", 3)]);
                    let original = entries.as_ref().clone();
                    for (dimensions, rows, win) in &windows {
                        let dimensions = *dimensions;
                        let label = format!("{layout:?}/{name}/{locale}/{zoom}/{dimensions:?}");
                        rows.update(cx, |rows, cx| {
                            rows.entries = entries.clone();
                            rows.layout = layout;
                            rows.selected = 1;
                            cx.notify();
                        });
                        draw(cx, *win, dimensions);
                        let bounds = cx.read(|cx| inspect(rows.read(cx), dimensions, &label));
                        // Was "a fresh mount reproduces this geometry"; with one
                        // window per viewport the same oracle is that redrawing
                        // unchanged state is a fixed point.
                        draw(cx, *win, dimensions);
                        assert_eq!(
                            bounds,
                            cx.read(|cx| inspect(rows.read(cx), dimensions, &label)),
                            "{label}: redrawing the same state changed geometry"
                        );
                        rows.update(cx, |rows, cx| {
                            rows.selected = 0;
                            cx.notify();
                        });
                        draw(cx, *win, dimensions);
                        assert_eq!(
                            bounds,
                            cx.read(|cx| inspect(rows.read(cx), dimensions, &label)),
                            "{label}: selection must not reflow the list"
                        );
                        assert_eq!(
                            entries.as_ref(),
                            &original,
                            "{label}: rendering changed raw commit strings"
                        );
                    }
                }
            }
        }
    }
    for (_, rows, win) in windows {
        unmount(cx, rows, win);
    }
    drop(restore);
    eprintln!("[gui-e2e] PASS commit_row_layout 704 matrix cells plus redraw/selection checks over 4 reused windows; native resize unavailable in VisualTestAppContext");
}

struct GlobalSettings {
    zoom: f32,
    language_env: Option<std::ffi::OsString>,
    language: i18n::Lang,
}

impl GlobalSettings {
    fn capture() -> Self {
        Self {
            zoom: theme::zoom(),
            language_env: std::env::var_os("KAGI_LANG"),
            language: i18n::lang(),
        }
    }
}

impl Drop for GlobalSettings {
    fn drop(&mut self) {
        theme::set_zoom(self.zoom);
        std::env::set_var("KAGI_LANG", self.language.slug());
        i18n::init_lang();
        match self.language_env.take() {
            Some(value) => std::env::set_var("KAGI_LANG", value),
            None => std::env::remove_var("KAGI_LANG"),
        }
    }
}

/// Mount the real Editor history under KagiApp at its practical desktop size.
/// Caller supplies an isolated fixture repo; this scenario only reads it.
/// The shared-row matrix above owns exact per-row bounds; the actual uniform
/// list exposes measured item/content sizes, not per-item bounds.
pub fn scenario_editor_history_layout(cx: &mut VisualTestAppContext, repo_path: &Path) {
    let restore = GlobalSettings::capture();
    theme::set_zoom(1.);
    for dimensions in [(1440., 900.), (1024., 768.), (1440., 900.)] {
        let app = e2e::app_state(repo_path).expect("fixture app state");
        let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
        let output = captured.clone();
        let win = crate::macos::open_offscreen(
            cx,
            size(px(dimensions.0), px(dimensions.1)),
            move |window, cx| e2e::mount_root(app, window, cx, &output),
        );
        let app = captured.borrow().clone().expect("captured KagiApp");
        app.update(cx, |app, cx| app.open_editor_workspace(cx));
        cx.run_until_parked();
        let editor = cx
            .read(|cx| app.read(cx).editor_workspace.clone())
            .expect("actual Editor workspace");
        for (name, text) in inputs() {
            let history = FileHistory {
                current_path: "README.md".into(),
                entries: vec![entry("A", 1), entry(&text, 2), entry("Z", 3)],
            };
            editor.update(cx, |view, cx| {
                view.right_tab = RightPaneTab::History;
                view.history_loading = false;
                view.history = Some(history.clone());
                view.selected_history_commit = Some(format!("{:040x}", 2));
                cx.notify();
            });
            draw(cx, win.into(), dimensions);
            let short_first = cx.read(|cx| {
                let view = editor.read(cx);
                let state = view.history_scroll.0.borrow();
                let measured = state.last_item_size.expect("actual history list mounted");
                assert!(
                    measured.contents.height > px(0.),
                    "{name}: history was not laid out"
                );
                finite(
                    Bounds::new(state.base_handle.bounds().origin, measured.contents),
                    name,
                );
                assert_eq!(
                    view.history.as_ref(),
                    Some(&history),
                    "{name}: raw history changed"
                );
                measured.contents
            });
            // uniform_list measures its first entry. Reordering the SAME real
            // history must not change the scroll extent just because a long
            // author became first; this catches the over-tall-first-row variant.
            let mut reordered = history.clone();
            reordered.entries.swap(0, 1);
            editor.update(cx, |view, cx| {
                view.history = Some(reordered.clone());
                cx.notify();
            });
            draw(cx, win.into(), dimensions);
            cx.read(|cx| {
                let view = editor.read(cx);
                let state = view.history_scroll.0.borrow();
                let long_first = state
                    .last_item_size
                    .expect("reordered history laid out")
                    .contents;
                assert_eq!(
                    short_first, long_first,
                    "{name}: first author changes history scroll extent"
                );
                assert_eq!(
                    view.history.as_ref(),
                    Some(&reordered),
                    "{name}: raw history changed"
                );
            });
            editor.update(cx, |view, cx| {
                view.history = Some(history.clone());
                cx.notify();
            });
        }
        drop(editor);
        drop(captured);
        unmount(cx, app, win.into());
    }
    drop(restore);
    eprintln!("[gui-e2e] PASS editor_history_layout actual KagiApp/Editor 11 inputs at fresh 1440/1024/1440 mounts; native resize unavailable");
}

/// Issue #547: the 22 px footer must show ONE line — the beginning of the
/// operation result — at every window width.
///
/// The regression this pins: the message was laid out as wrapped, multi-line
/// text inside a fixed-height `items_center()` row, so the bar centre-clipped
/// it and a *middle* line was what the user saw (measured: y=859.5/h=58.5 for
/// a 3-line message, y=-86/h=1950 at 320 px). Real `KagiApp`, real footer, and
/// the message element's own laid-out bounds — not a screenshot.
pub fn scenario_footer_status_line(cx: &mut VisualTestAppContext, repo_path: &Path) {
    let restore = GlobalSettings::capture();
    theme::set_zoom(1.);
    const HEIGHT: f32 = 900.;
    const BAR_H: f32 = 22.; // STATUS_BAR_H (src/ui/mod.rs)
    let messages: Vec<(&str, String)> = vec![
        // The short single-line case is first: it defines one line's height.
        ("short", "failed: nothing to do".into()),
        (
            "three-line-lf",
            "first line\nsecond line\nthird line".into(),
        ),
        (
            "three-line-crlf",
            "first line\r\nsecond line\r\nthird line".into(),
        ),
        (
            "three-line-cr",
            "first line\rsecond line\rthird line".into(),
        ),
        ("long", "failed: long message ".repeat(50)),
        ("spaces", format!("failed:{}reason", " ".repeat(4096))),
        (
            "japanese",
            "\u{5931}\u{6557}: \u{9577}\u{3044}\u{7406}\u{7531} ".repeat(50),
        ),
        (
            "emoji-combining",
            "\u{1f469}\u{200d}\u{1f4bb}e\u{301} ".repeat(128),
        ),
    ];
    let mut one_line: Option<Pixels> = None;
    for width in [1440., 640., 320.] {
        let dimensions = (width, HEIGHT);
        let app_state = e2e::app_state(repo_path).expect("fixture app state");
        let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
        let output = captured.clone();
        let win =
            crate::macos::open_offscreen(cx, size(px(width), px(HEIGHT)), move |window, cx| {
                e2e::mount_root(app_state, window, cx, &output)
            });
        let app = captured.borrow().clone().expect("captured KagiApp");
        cx.run_until_parked();
        for (name, text) in &messages {
            let label = format!("{name}/{width}");
            app.update(cx, |app, cx| {
                app.status_footer = FooterStatus::Failed(gpui::SharedString::from(text.clone()));
                cx.notify();
            });
            draw(cx, win.into(), dimensions);
            let bounds = e2e::footer_message_bounds();
            finite(bounds, &label);
            assert!(
                bounds.size.height > px(0.),
                "{label}: footer message not mounted"
            );
            // Inside the bar band — the bug put the top far above it and let
            // the bar clip down to a middle line.
            assert!(
                bounds.top() + px(EPS) >= px(HEIGHT - BAR_H)
                    && bounds.bottom() <= px(HEIGHT) + px(EPS),
                "{label}: message escapes the footer band [{}, {HEIGHT}]: {bounds:?}",
                HEIGHT - BAR_H,
            );
            assert!(
                bounds.size.height <= px(BAR_H) + px(EPS),
                "{label}: message is taller than the bar: {bounds:?}"
            );
            // flex_1 + min_w_0: the message never pushes the Terminal /
            // Operation Log icons past the right edge.
            assert!(
                bounds.right() <= px(width) + px(EPS),
                "{label}: message overflows the window: {bounds:?}"
            );
            // Every input lays out to exactly the short message's one line.
            let expected = *one_line.get_or_insert(bounds.size.height);
            assert!(
                f32::from(bounds.size.height - expected).abs() <= EPS,
                "{label}: {:?} is not one line ({expected:?})",
                bounds.size.height,
            );
            // …and the line the user reads starts at the start of the body.
            let line = view_models::footer_line(text);
            assert!(
                !line.contains(['\n', '\r']),
                "{label}: preview is multi-line: {line:?}"
            );
            let head = text
                .split(['\n', '\r'])
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .next()
                .unwrap_or_default();
            assert!(
                line.starts_with(head),
                "{label}: preview {line:?} does not start with {head:?}"
            );
            let stored = cx.read(|cx| match &app.read(cx).status_footer {
                FooterStatus::Failed(m) => m.to_string(),
                other => panic!("{label}: footer status changed: {other:?}"),
            });
            assert_eq!(&stored, text, "{label}: rendering changed the message");
        }
        drop(captured);
        unmount(cx, app, win.into());
    }
    drop(restore);
    eprintln!("[gui-e2e] PASS footer_status_line 8 messages x 1440/640/320: one line inside the 22 px bar, first line first");
}
