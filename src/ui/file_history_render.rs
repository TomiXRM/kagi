//! File History view rendering (ADR-0089), split out of `render_helpers.rs`
//! (T-SPLIT-HELPERS-001 / ADR-0116 Wave 3) to sit next to its state in
//! `file_history.rs`. Reuses `render_main_diff_view` (kept in `render_helpers`)
//! for the diff body. Behaviour-preserving move — no DOM/style/handler/[kagi]/
//! i18n change.

#![allow(clippy::too_many_arguments)]

use super::file_history::FileHistoryView;
use super::render_helpers::*;
use super::*;
use gpui_component::button::{Button, ButtonVariants};

// ──────────────────────────────────────────────────────────────
// ADR-0117: File History as its own `Entity<FileHistoryView>`.
//
// The entity owns the read-only Backend loads (history + diff) — it holds
// `repo_path` — so every entity-initiated action updates *self* and never
// re-enters `KagiApp.file_history` (which would double-borrow the leased entity
// and panic). The only parent callbacks are `close` and `jump_to_commit`.
// ──────────────────────────────────────────────────────────────

impl Render for FileHistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_file_history_view(self, cx)
    }
}

impl FileHistoryView {
    /// Kick off the async history load for the current `rel_path` /
    /// `follow_renames`. Marshals the result back into *this* entity, guarded by
    /// the per-entity `generation` so a superseded load (rapid refresh) or a
    /// dropped entity (close/reopen) is discarded.
    ///
    /// `emit_loaded` preserves the pre-extraction contract: the initial open and
    /// Refresh emit `[kagi] file-history: loaded N entries`; the follow-toggle
    /// reload does NOT (it only emits the `open` line).
    pub fn start_load(
        &mut self,
        origin: Option<CommitId>,
        emit_loaded: bool,
        cx: &mut Context<Self>,
    ) {
        let generation = self.data.generation;
        let follow = self.data.follow_renames;
        let req_path = self.data.rel_path.clone();
        let bg_path = self.repo_path.clone();
        let task = cx.background_spawn(async move {
            let req = kagi_git::FileHistoryRequest {
                repo_dir: bg_path,
                file_path: req_path,
                follow_renames: follow,
                include_wip: true,
                limit: 500,
            };
            kagi_git::file_history(&req)
        });

        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| {
                // Per-entity generation guard: discard a superseded load.
                if v.data.generation != generation {
                    return;
                }
                match result {
                    Ok(history) => {
                        if emit_loaded {
                            klog!("file-history: loaded {} entries", history.entries.len());
                        }
                        let initial = Self::pick_initial_index(&history, &origin);
                        v.data.history = Some(history);
                        v.data.error = None;
                        v.data.selected = initial;
                        v.load_diff(cx);
                    }
                    Err(e) => {
                        v.data.history = None;
                        v.data.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-run the history load for the current file (Refresh / Retry / Follow
    /// toggle), preserving the current selection's commit as the re-selection
    /// origin. Bumps `generation` so any in-flight older load is discarded.
    pub fn reload(&mut self, emit_loaded: bool, cx: &mut Context<Self>) {
        let origin = self
            .data
            .selected_entry()
            .and_then(|e| e.commit.as_ref())
            .map(|c| CommitId(c.full_hash.clone()));
        klog!("file-history: open {}", self.data.rel_path.display());
        self.data.history = None;
        self.data.error = None;
        self.data.diff = None;
        self.data.selected = 0;
        self.data.generation = self.data.generation.wrapping_add(1);
        cx.notify();
        self.start_load(origin, emit_loaded, cx);
    }

    /// Select a history entry and (synchronously) load its diff.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let valid = self
            .data
            .history
            .as_ref()
            .is_some_and(|h| index < h.entries.len());
        if !valid {
            return;
        }
        self.data.selected = index;
        self.load_diff(cx);
        cx.notify();
    }

    /// Move the entry selection up/down by `delta` (arrow keys), clamped.
    pub fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let len = self
            .data
            .history
            .as_ref()
            .map(|h| h.entries.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        let cur = self.data.selected;
        let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
        if next != cur {
            self.select(next, cx);
        }
    }

    /// Update the list/diff vertical split ratio (divider drag), child-scoped.
    pub fn set_split(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if (ratio - self.data.split).abs() > 0.002 {
            self.data.split = ratio;
            cx.notify();
        }
    }

    /// Choose the initial selected index: the WIP row (always index 0 if
    /// present), else the `origin` commit if it appears, else the newest entry.
    fn pick_initial_index(history: &kagi_git::FileHistory, origin: &Option<CommitId>) -> usize {
        use kagi_git::FileHistoryEntryKind;
        if let Some(first) = history.entries.first() {
            if first.kind == FileHistoryEntryKind::Wip {
                return 0;
            }
        }
        if let Some(id) = origin {
            if let Some(ix) = history.entries.iter().position(|e| {
                e.commit
                    .as_ref()
                    .is_some_and(|c| c.full_hash == id.0 || c.full_hash.starts_with(&id.0))
            }) {
                return ix;
            }
        }
        0
    }

    /// Build the `MainDiffView` for the currently-selected entry, reusing the
    /// existing diff renderer pipeline. The git diff computation (Backend open +
    /// per-file diff) runs **off the UI thread** via `cx.background_spawn` so
    /// selecting a row on a large file/repo can't jank the frame. Only the plain
    /// `kagi_git::FileDiff` crosses back; the row build + syntax highlight then
    /// run on the UI thread in the marshalled result, because `MainDiffView` /
    /// `DiffRow` hold GPUI types and are therefore `!Send`.
    ///
    /// The result is guarded by the monotonic `diff_req` token (bumped here on
    /// every call) plus `generation`: a newer diff load (rapid arrowing / Refresh,
    /// including A→B→A) bumps `diff_req`, and a history reload bumps `generation`,
    /// so a superseded load is discarded instead of overwriting the current
    /// selection's diff. Never panics on missing / binary / deleted files — falls
    /// back to `diff = None` so the banner messaging covers it. (Moved from
    /// `KagiApp::load_file_history_diff`, sourcing `repo_path` from `self`.)
    pub fn load_diff(&mut self, cx: &mut Context<Self>) {
        use kagi_git::FileHistoryEntryKind;

        let repo_path = self.repo_path.clone();
        let Some(entry) = self.data.selected_entry() else {
            self.data.diff = None;
            return;
        };

        let path = entry.change.path_after.clone();
        let kind = entry.kind;
        let commit_id = entry.commit.as_ref().map(|c| CommitId(c.full_hash.clone()));

        // Bump a monotonic per-diff token and capture it (plus `generation`) so
        // the marshalled result is applied only if neither a newer diff load nor
        // a history reload has superseded it. `diff_req` alone covers row changes
        // (incl. A→B→A); `generation` additionally covers the window after a
        // reload but before its post-load `load_diff` runs.
        let req = self.data.diff_req.wrapping_add(1);
        self.data.diff_req = req;
        let generation = self.data.generation;

        // Off-thread: open the repo and compute the per-file diff (the expensive
        // I/O + diff work). `FileDiff` is plain `kagi_git` data (`Send`); the GPUI
        // view types are built on the UI thread in the marshalled result below.
        let diff_path = path.clone();
        let task = cx.background_spawn(async move {
            let repo = kagi_git::Backend::open(&repo_path).ok()?;
            let result = match kind {
                FileHistoryEntryKind::Wip => {
                    // Prefer the unstaged diff; fall back to staged (e.g. fully
                    // staged change) so the WIP entry still shows something.
                    match repo.unstaged_file_diff(&diff_path) {
                        Ok(d) if !d.hunks.is_empty() || d.is_binary => Ok(d),
                        _ => repo.staged_file_diff(&diff_path),
                    }
                }
                FileHistoryEntryKind::Commit => match commit_id {
                    Some(id) => repo.commit_file_diff(&id, &diff_path),
                    None => return None,
                },
            };
            Some(result.map_err(|e| e.to_string()))
        });

        cx.spawn(async move |view, acx| {
            let result = task.await;
            let _ = view.update(acx, |v, cx| {
                // Discard a superseded diff load (newer selection) or one
                // invalidated by a history reload.
                if v.data.diff_req != req || v.data.generation != generation {
                    return;
                }
                let built = match result {
                    // T-WS-EDITOR-005 finding #10: shared builder (count →
                    // from_file_diff → stats → highlight → assemble), same
                    // pipeline as `EditorWorkspaceView`'s WIP-diff loader and
                    // `set_commit_main_diff`'s headless path.
                    Some(Ok(file_diff)) => Some(build_main_diff_view(
                        &file_diff,
                        &path,
                        0,
                        // The source is unused by the File History renderer;
                        // Unstaged carries the path for completeness.
                        MainDiffSource::Unstaged { path: path.clone() },
                    )),
                    Some(Err(e)) => {
                        klog!("file-history diff error: {}", e);
                        None
                    }
                    None => None,
                };
                v.data.diff = built;
                cx.notify();
            });
        })
        .detach();
    }
}

// ──────────────────────────────────────────────────────────────
// ADR-0089: File History view rendering
// ──────────────────────────────────────────────────────────────

/// A small text "chip" button used in the File History header.
pub(crate) fn fh_header_button(
    id: &'static str,
    label: impl Into<SharedString>,
    on_click: impl Fn(
            &mut FileHistoryView,
            &gpui::ClickEvent,
            &mut gpui::Window,
            &mut Context<FileHistoryView>,
        ) + 'static,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    Button::new(id)
        .label(label.into())
        .ghost()
        .small()
        .on_click(cx.listener(on_click))
}

/// Render the entire File History view (center + right detail pane), ADR-0089.
///
/// Reuses [`render_main_diff_view`] for the diff body.  Returns the body
/// fragment that `render_body` drops in place of the normal center+right area.
pub(crate) fn render_file_history_view(
    view: &FileHistoryView,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // ADR-0117: the entity renders itself. Read only `self`'s data here — never
    // upgrade `view.app` in a render path (it would re-enter the parent and
    // panic). The owned bindings below come straight from `view.data`.
    let state = &view.data;
    let file_history_menu = view.menu;
    let fh_branch = view.data.branch.clone();
    let panel_width = view.panel_width;
    let geom = view.geom.clone();

    // Extract the scalar/owned view data from the `state` borrow.
    let (rel_path, follow, split, count, is_loading, error, is_empty, is_untracked) = (
        state.rel_path.clone(),
        state.follow_renames,
        state.split,
        state.commit_count(),
        state.is_loading(),
        state.error.clone(),
        state.is_empty(),
        state.is_untracked(),
    );
    let rel_path_str = SharedString::from(rel_path.to_string_lossy().into_owned());

    // ── Header ──────────────────────────────────────────────────────
    let back = fh_header_button(
        "fh-back",
        "\u{2190} Back",
        |this, _e, _w, cx| {
            this.app
                .update(cx, |app, cx| {
                    app.close_file_history();
                    cx.notify();
                })
                .ok();
        },
        cx,
    );

    let path_for_copy = rel_path.clone();
    let copy_path = fh_header_button(
        "fh-copy-path",
        "Copy Path",
        move |_this, _e, _w, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                path_for_copy.to_string_lossy().into_owned(),
            ));
        },
        cx,
    );

    let refresh = fh_header_button(
        "fh-refresh",
        "Refresh",
        |this, _e, _w, cx| {
            this.reload(true, cx);
        },
        cx,
    );

    let path_for_open = rel_path.clone();
    let open_file = fh_header_button(
        "fh-open-file",
        "Open File",
        move |this, _e, _w, cx| {
            // v1: return to the normal body; the file's diff is reachable via
            // the commit panel / inspector.  Keep it simple per the spec.
            let _ = &path_for_open;
            this.app
                .update(cx, |app, _cx| app.close_file_history())
                .ok();
        },
        cx,
    );

    let follow_label = if follow {
        "Follow Renames: On"
    } else {
        "Follow Renames: Off"
    };
    let follow_btn = fh_header_button(
        "fh-follow",
        follow_label,
        |this, _e, _w, cx| {
            this.data.follow_renames = !this.data.follow_renames;
            this.reload(false, cx);
        },
        cx,
    );

    let header = div()
        .id("fh-header")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .px_3()
        .py_1()
        .gap_2()
        .bg(rgb(theme().surface))
        .child(back)
        .child(
            div()
                .id("fh-title")
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(SharedString::from(format!(
                    "File History: {}",
                    rel_path_str
                )))
                .tooltip(move |window, cx| Tooltip::new(rel_path_str.clone()).build(window, cx)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .child(fh_branch.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{} commits", count))),
        )
        .child(refresh)
        .child(copy_path)
        .child(open_file)
        .child(follow_btn);

    // ── Center column (list + diff) selection of the body content ──
    let center_body: gpui::AnyElement = if is_loading {
        render_fh_message("Loading file history...", false, cx).into_any_element()
    } else if let Some(err) = error {
        render_fh_error(err, cx).into_any_element()
    } else if is_empty {
        render_fh_message("No history found for this file.", false, cx).into_any_element()
    } else if is_untracked {
        // Untracked: show the message but still allow the WIP diff below.
        render_fh_list_and_diff(
            state,
            split,
            Some("This file is untracked. No commit history yet."),
            geom,
            cx,
        )
        .into_any_element()
    } else {
        render_fh_list_and_diff(state, split, None, geom, cx).into_any_element()
    };

    let center = div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_w(px(0.))
        .bg(rgb(theme().panel))
        .child(header)
        .child(center_body);

    // ── Right detail pane ──────────────────────────────────────────
    let detail_divider = div()
        .id("fh-detail-divider")
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::Panel,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let detail_pane = render_fh_detail_pane(state, panel_width, cx);

    // ── Optional row context menu overlay ──────────────────────────
    let menu_overlay = file_history_menu.map(|(ix, pos)| render_fh_row_menu(state, ix, pos, cx));

    div()
        .id("file-history-view")
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .child(center)
        .child(detail_divider)
        .child(detail_pane)
        .children(menu_overlay)
        .into_any_element()
}

/// A centered single-line message (Loading / Empty), optionally an error tint.
pub(crate) fn render_fh_message(
    msg: &'static str,
    is_error: bool,
    _cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let color = if is_error {
        theme().color_blocker
    } else {
        theme().text_muted
    };
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(color))
        .child(SharedString::from(msg))
}

/// Error state: message + detail + Retry button.
pub(crate) fn render_fh_error(
    detail: String,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let retry = div()
        .id("fh-retry")
        .px_3()
        .py_1()
        .rounded_sm()
        .bg(rgb(theme().bg_base))
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .on_click(cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.reload(true, cx);
        }))
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .child(SharedString::from("Retry"));

    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .child(SharedString::from("Failed to load file history.")),
        )
        .child(
            div()
                .max_w(px(520.))
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(detail)),
        )
        .child(retry)
}

/// The vertically-split commit list (top) + diff viewer (bottom).
pub(crate) fn render_fh_list_and_diff(
    state: &file_history::FileHistoryState,
    split: f32,
    banner: Option<&'static str>,
    geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let list = render_fh_commit_list(state, cx);
    let (diff_view, diff_scroll, sel_banner) = {
        let diff = state.diff.clone();
        let scroll = state.diff_scroll.clone();
        // Per-entry banner (Added / Deleted / Renamed) above the diff.
        let sel_banner = state.selected_entry().map(|e| {
            use kagi_git::FileChangeType;
            match e.change.change_type {
                FileChangeType::Added => "This file was added in this commit.".to_string(),
                FileChangeType::Deleted => "This file was deleted in this commit.".to_string(),
                FileChangeType::Renamed => {
                    let before = e
                        .change
                        .path_before
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let after = e.change.path_after.to_string_lossy().into_owned();
                    format!("{} \u{2192} {}", before, after)
                }
                _ if e.change.is_binary => {
                    "Binary file changed. Preview is not available.".to_string()
                }
                _ => String::new(),
            }
        });
        (diff, scroll, sel_banner)
    };

    // Divider between list and diff (horizontal drag).
    let h_divider = div()
        .id("fh-rows-divider")
        .w_full()
        .h(theme::scaled_px(4.))
        .flex_shrink_0()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
        .cursor_row_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::FileHistoryRows,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let list_frac = split.clamp(0.15, 0.85);
    let diff_frac = 1.0 - list_frac;

    let diff_section = div()
        .w_full()
        .flex()
        .flex_col()
        .flex_grow()
        .flex_basis(gpui::relative(diff_frac))
        .min_h(px(0.))
        // Optional view-level banner (untracked note).
        .when_some(banner, |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().color_warning))
                    .child(SharedString::from(b)),
            )
        })
        // Per-entry banner (added/deleted/renamed/binary).
        .when_some(sel_banner.filter(|s| !s.is_empty()), |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .bg(rgb(theme().bg_row_alt))
                    .child(SharedString::from(b)),
            )
        })
        .child(match diff_view {
            // ADR-0117: render the diff list directly on the FileHistoryView
            // context (no standalone Back/History buttons — FH has its own Back).
            Some(view) => render_diff_list::<FileHistoryView>(view, None, None, diff_scroll, cx)
                .into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("No diff available for this entry."))
                .into_any_element(),
        });

    // Paint-time canvas records the real (top, bottom) screen bounds of this
    // list+diff region so the divider drag maps the cursor exactly (a constant
    // top offset misses the variable-height header → the drag would jump).
    let measure = {
        let geom = geom.clone();
        gpui::canvas(
            move |_b: gpui::Bounds<gpui::Pixels>, _w: &mut Window, _cx: &mut gpui::App| {},
            move |b: gpui::Bounds<gpui::Pixels>, _p: (), _w: &mut Window, _cx: &mut gpui::App| {
                let top = f32::from(b.origin.y);
                geom.set((top, top + f32::from(b.size.height)));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };

    div()
        .relative()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(measure)
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .flex_grow()
                .flex_basis(gpui::relative(list_frac))
                .min_h(px(0.))
                .child(list),
        )
        .child(h_divider)
        .child(diff_section)
}

/// The commit list (upper pane) of the File History view.
pub(crate) fn render_fh_commit_list(
    state: &file_history::FileHistoryState,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    let Some(history) = state.history.as_ref() else {
        return div().into_any_element();
    };
    let entries = &history.entries;
    let selected = state.selected;
    let now = commit_list::now_unix_secs();

    let mut list = div()
        .id("fh-commit-list")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .min_h(px(0.));

    for (ix, entry) in entries.iter().enumerate() {
        list = list.child(render_fh_row(ix, entry, ix == selected, now, cx));
    }

    list.into_any_element()
}

/// One row in the File History commit list.
pub(crate) fn render_fh_row(
    ix: usize,
    entry: &kagi_git::FileHistoryEntry,
    is_selected: bool,
    now: i64,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    use kagi_git::FileHistoryEntryKind;

    let (badge, badge_color) = file_history::entry_badge(entry);
    let is_wip = entry.kind == FileHistoryEntryKind::Wip;

    let (subject, author, date, short_hash) = if is_wip {
        (
            SharedString::from("WIP \u{2014} Uncommitted changes"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    } else if let Some(c) = entry.commit.as_ref() {
        let date = file_history::iso_to_epoch(&c.author_date)
            .map(|e| commit_list::relative_time(e, now))
            .unwrap_or_default();
        (
            SharedString::from(c.subject.clone()),
            SharedString::from(c.author_name.clone()),
            SharedString::from(date),
            SharedString::from(c.short_hash.clone()),
        )
    } else {
        (
            SharedString::from("(unknown)"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    };

    let ins = entry.change.insertions;
    let del = entry.change.deletions;
    let stat = if entry.change.is_binary {
        SharedString::from("bin")
    } else {
        SharedString::from(format!(
            "+{} \u{2212}{}",
            ins.unwrap_or(0),
            del.unwrap_or(0)
        ))
    };

    let row_bg = if is_selected {
        theme().selected
    } else if ix % 2 == 1 {
        theme().bg_row_alt
    } else {
        theme().panel
    };

    let click = cx.listener(move |this, e: &gpui::ClickEvent, _w, cx| {
        this.menu = None;
        if e.click_count() >= 2 {
            // Double-click: jump to the commit in the graph (commits only).
            if let Some(id) = this
                .data
                .history
                .as_ref()
                .and_then(|h| h.entries.get(ix))
                .and_then(|e| e.commit.as_ref())
                .map(|c| CommitId(c.full_hash.clone()))
            {
                this.app
                    .update(cx, |app, cx| {
                        app.close_file_history();
                        app.jump_to_commit(&id);
                        cx.notify();
                    })
                    .ok();
                return;
            }
        }
        this.select(ix, cx);
    });
    let ctx = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
        this.menu = Some((ix, e.position));
        cx.stop_propagation();
        cx.notify();
    });

    div()
        .id(("fh-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .px_3()
        .py_px()
        .h(px(row_height(false)))
        .flex_shrink_0()
        .bg(rgb(row_bg))
        .on_click(click)
        .on_mouse_down(MouseButton::Right, ctx)
        .cursor_pointer()
        // Hover uses the subtle `surface` tint (like the commit panel / branch
        // list), NOT `selected` — using the selection colour made a hovered row
        // indistinguishable from the selected one, so the row the mouse was left
        // on after a click looked "still selected" while the arrows moved the
        // real selection elsewhere. The selected row keeps its colour on hover.
        .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        // change-type letter
        .child(
            div()
                .w(theme::scaled_px(18.))
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        // subject
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(subject),
        )
        // author
        .child(
            div()
                .w(theme::scaled_px(90.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(author),
        )
        // relative date
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(date),
        )
        // +ins / -del
        .child(
            div()
                .w(theme::scaled_px(72.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(stat),
        )
        // short hash
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(short_hash),
        )
}

/// Right detail pane for the selected File History entry.
pub(crate) fn render_fh_detail_pane(
    state: &file_history::FileHistoryState,
    panel_width: f32,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // Clone the entry out so listeners can capture owned data.
    let entry: Option<kagi_git::FileHistoryEntry> = state.selected_entry().cloned();

    let mut pane = div()
        .id("fh-detail-pane")
        .w(theme::scaled_px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .bg(rgb(theme().panel))
        .overflow_y_scroll();

    let Some(entry) = entry else {
        return pane
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("No entry selected.")),
            )
            .into_any_element();
    };

    let line = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(value)),
            )
    };

    let ct = entry.change.change_type;
    let ct_label = file_history::change_type_label(ct).to_string();
    let stat = if entry.change.is_binary {
        "binary".to_string()
    } else {
        format!(
            "+{} \u{2212}{}",
            entry.change.insertions.unwrap_or(0),
            entry.change.deletions.unwrap_or(0)
        )
    };
    let path_after = entry.change.path_after.to_string_lossy().into_owned();
    let path_before = entry
        .change
        .path_before
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    if let Some(c) = entry.commit.as_ref() {
        let full = c.full_hash.clone();
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(c.subject.clone())),
            )
            .child(line("Full Hash", c.full_hash.clone()))
            .child(line("Short Hash", c.short_hash.clone()));

        if let Some(body) = c.body.as_ref() {
            pane = pane.child(line("Message", body.clone()));
        }
        pane = pane
            .child(line(
                "Author",
                format!("{} <{}>", c.author_name, c.author_email),
            ))
            .child(line("Committer", c.committer_name.clone()))
            .child(line("Author Date", c.author_date.clone()))
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path After", path_after));
        if let Some(before) = path_before {
            pane = pane.child(line("Path Before", before));
        }

        // ── Actions ──
        let id_open = CommitId(full.clone());
        let id_graph = CommitId(full.clone());
        let full_for_copy = full.clone();
        let actions = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .mt_2()
            .child(fh_header_button(
                "fh-detail-open",
                "Open Commit",
                move |this, _e, _w, cx| {
                    this.app
                        .update(cx, |app, cx| {
                            app.close_file_history();
                            app.jump_to_commit(&id_open);
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-graph",
                "Show in Graph",
                move |this, _e, _w, cx| {
                    this.app
                        .update(cx, |app, cx| {
                            app.close_file_history();
                            app.jump_to_commit(&id_graph);
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-copy",
                "Copy Hash",
                move |_this, _e, _w, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(full_for_copy.clone()));
                },
                cx,
            ));
        pane = pane.child(actions);
    } else {
        // WIP entry — minimal detail.
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from("Uncommitted changes")),
            )
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path", path_after));
    }

    pane.into_any_element()
}

/// Context menu for a File History commit row (ADR-0089).
pub(crate) fn render_fh_row_menu(
    state: &file_history::FileHistoryState,
    ix: usize,
    pos: gpui::Point<gpui::Pixels>,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // Resolve the entry's data up front (commit hash + path at this commit).
    let (commit_hash, path_at) = {
        let entry = state.history.as_ref().and_then(|h| h.entries.get(ix));
        let commit_hash = entry
            .and_then(|e| e.commit.as_ref())
            .map(|c| c.full_hash.clone());
        let path_at = entry.map(|e| e.change.path_after.to_string_lossy().into_owned());
        (commit_hash, path_at)
    };

    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _w, cx| {
        this.menu = None;
        cx.notify();
    });

    fn item<F>(id: &'static str, label: &'static str, on_click: F) -> gpui::Stateful<gpui::Div>
    where
        F: Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    {
        div()
            .id(id)
            .px_3()
            .py(theme::scaled_px(3.))
            .text_sm()
            .text_color(rgb(theme().text_main))
            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
            .on_click(on_click)
            .child(SharedString::from(label))
    }

    let mut menu = div()
        .absolute()
        .left(pos.x)
        .top(pos.y)
        .w(theme::scaled_px(220.))
        .occlude()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .rounded_md()
        .shadow_lg()
        .py(theme::scaled_px(2.));

    if let Some(hash) = commit_hash.clone() {
        let h1 = hash.clone();
        menu = menu.child(item(
            "fh-menu-copy-hash",
            "Copy Commit Hash",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(h1.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(p) = path_at.clone() {
        menu = menu.child(item(
            "fh-menu-copy-path",
            "Copy File Path at This Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(hash) = commit_hash.clone() {
        let id_open = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-open-commit",
            "Open Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                this.app
                    .update(cx, |app, cx| {
                        app.close_file_history();
                        app.jump_to_commit(&id_open);
                        cx.notify();
                    })
                    .ok();
            }),
        ));
        let id_graph = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-graph",
            "Show Commit in Graph",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                this.app
                    .update(cx, |app, cx| {
                        app.close_file_history();
                        app.jump_to_commit(&id_graph);
                        cx.notify();
                    })
                    .ok();
            }),
        ));
    }

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(menu)
        .into_any_element()
}
