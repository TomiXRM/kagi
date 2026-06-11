//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod commit_list;
pub mod detail_panel;
pub mod graph_view;

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{
    App, Context, Entity, SharedString, Window,
    div, prelude::*, px, rgb, uniform_list,
};

use crate::git::{ChangeKind, FileDiff, DiffLineKind, FileStatus, Head, RepoSnapshot};
use commit_list::{BadgeKind, CommitRow, build_commit_rows};
use detail_panel::{CommitDetail, build_commit_details};
use graph_view::{graph_canvas, graph_width};

// ──────────────────────────────────────────────────────────────
// Catppuccin Mocha palette (subset)
// ──────────────────────────────────────────────────────────────
const BG_BASE: u32 = 0x1e1e2e;
const BG_SURFACE: u32 = 0x313244;
const BG_SELECTED: u32 = 0x45475a; // surface1 — selected row highlight
const BG_PANEL: u32 = 0x181825;    // mantle — detail panel background
const TEXT_MAIN: u32 = 0xcdd6f4;
const TEXT_SUB: u32 = 0xa6adc8;
const TEXT_MUTED: u32 = 0x585b70;
const TEXT_LABEL: u32 = 0x6c7086; // overlay0 — field labels in detail panel
const COLOR_SHA: u32 = 0x89dceb; // teal
const COLOR_HEAD: u32 = 0xf38ba8; // red  — HEAD / attached branch
const COLOR_BRANCH: u32 = 0x89b4fa; // blue — local branch
const COLOR_REMOTE: u32 = 0xa6e3a1; // green — remote branch
const COLOR_TAG: u32 = 0xfab387; // peach — tag

// Diff display colours
const BG_DIFF_ADDED: u32 = 0x1c3a2a;   // dark green background for added lines
const BG_DIFF_REMOVED: u32 = 0x3a1c1c; // dark red background for removed lines
const COLOR_DIFF_HUNK: u32 = 0x89b4fa; // blue — hunk header

// ──────────────────────────────────────────────────────────────
// FileDiffView — pre-rendered diff rows for the diff panel
// ──────────────────────────────────────────────────────────────

/// A single displayable row in the diff viewer.
#[derive(Clone)]
pub enum DiffRow {
    /// A hunk header line (`@@ -a,b +c,d @@`).
    HunkHeader(SharedString),
    /// A content line (context / added / removed).
    Line {
        kind: DiffLineKind,
        /// The line content as a displayable string (with leading sigil stripped).
        text: SharedString,
    },
    /// Placeholder shown for binary files.
    Binary,
}

/// Pre-computed state for the diff view panel.
#[derive(Clone)]
pub struct FileDiffView {
    /// Display name of the file (path component).
    pub file_name: SharedString,
    /// All displayable rows: hunk headers + content lines.
    pub rows: Vec<DiffRow>,
    /// Row index into the commit's changed-files list (used for the back button).
    pub file_index: usize,
}

impl FileDiffView {
    /// Build a [`FileDiffView`] from a [`FileDiff`] result.
    pub fn from_file_diff(file_diff: &FileDiff, file_index: usize) -> Self {
        let path = file_diff
            .new_path
            .as_ref()
            .or(file_diff.old_path.as_ref());
        let file_name = SharedString::from(
            path.map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );

        let mut rows: Vec<DiffRow> = Vec::new();

        if file_diff.is_binary {
            rows.push(DiffRow::Binary);
        } else {
            for hunk in &file_diff.hunks {
                // Build hunk header string.
                let (os, oc) = hunk.old_range;
                let (ns, nc) = hunk.new_range;
                let header = SharedString::from(format!(
                    "@@ -{},{} +{},{} @@",
                    os, oc, ns, nc
                ));
                rows.push(DiffRow::HunkHeader(header));

                for line in &hunk.lines {
                    // Strip the trailing newline for display (keep content clean).
                    let raw = line.content.trim_end_matches('\n').trim_end_matches('\r');
                    // Add leading sigil for clarity.
                    let text = match line.kind {
                        DiffLineKind::Added   => SharedString::from(format!("+{}", raw)),
                        DiffLineKind::Removed => SharedString::from(format!("-{}", raw)),
                        DiffLineKind::Context => SharedString::from(format!(" {}", raw)),
                    };
                    rows.push(DiffRow::Line {
                        kind: line.kind.clone(),
                        text,
                    });
                }
            }
        }

        FileDiffView {
            file_name,
            rows,
            file_index,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// KagiApp — root view
// ──────────────────────────────────────────────────────────────

/// Root GPUI view.  Holds all pre-computed display data so the render
/// closure never calls `format!` on hot paths.
pub struct KagiApp {
    /// One-line header text: repo name + HEAD + status summary.
    pub header: SharedString,
    /// Pre-computed commit rows (built once from the snapshot).
    pub rows: Vec<CommitRow>,
    /// Pre-computed detail panel data, parallel to `rows`.
    pub details: Vec<CommitDetail>,
    /// Currently selected row index (None = no selection).
    pub selected: Option<usize>,
    /// Error or informational message shown instead of the commit list.
    pub error: Option<SharedString>,
    /// Absolute path to the repository root; used for on-demand diff fetches.
    pub repo_path: Option<PathBuf>,
    /// Cache of changed-files results keyed by row index.
    /// `None` value means the diff was attempted but failed (show unavailable).
    pub diff_cache: HashMap<usize, Option<Vec<FileStatus>>>,
    /// When `Some`, the detail panel shows the diff for this file instead of
    /// the commit metadata + changed-files list.  Cleared whenever
    /// `selected` changes.
    pub file_diff_view: Option<FileDiffView>,
}

impl KagiApp {
    /// Construct from a successful [`RepoSnapshot`].
    pub fn from_snapshot(repo_name: &str, snap: &RepoSnapshot) -> Self {
        let head_label = match &snap.head {
            Head::Attached { branch, .. } => format!("branch: {branch}"),
            Head::Detached { target } => format!(
                "detached: {}",
                target.get(..8).unwrap_or(target)
            ),
            Head::Unborn { branch } => format!("unborn ({branch})"),
        };

        let status = &snap.status;
        let status_label = if status.is_dirty() {
            let parts: Vec<String> = [
                (!status.staged.is_empty())
                    .then(|| format!("{}S", status.staged.len())),
                (!status.unstaged.is_empty())
                    .then(|| format!("{}M", status.unstaged.len())),
                (!status.untracked.is_empty())
                    .then(|| format!("{}?", status.untracked.len())),
                (!status.conflicted.is_empty())
                    .then(|| format!("{}!", status.conflicted.len())),
            ]
            .into_iter()
            .flatten()
            .collect();
            format!(" [{}]", parts.join(" "))
        } else {
            " [clean]".to_string()
        };

        let header = SharedString::from(format!(
            "{repo_name}  ·  {head_label}{status_label}  ·  {} commits",
            snap.commits.len()
        ));

        let rows = build_commit_rows(snap);
        let details = build_commit_details(snap);

        // T009: log lane count derived from the first row (all rows share the same value).
        let lane_count = rows.first().map(|r| r.lane_count).unwrap_or(0);
        eprintln!("[kagi] graph: lane_count={}", lane_count);
        eprintln!("[kagi] commit list rows: {}", rows.len());

        KagiApp {
            header,
            rows,
            details,
            selected: None,
            error: None,
            repo_path: None,
            diff_cache: HashMap::new(),
            file_diff_view: None,
        }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
            header: SharedString::from("kagi"),
            rows: Vec::new(),
            details: Vec::new(),
            selected: None,
            error: Some(SharedString::from(message.into())),
            repo_path: None,
            diff_cache: HashMap::new(),
            file_diff_view: None,
        }
    }

    /// Select the commit at `index` (or deselect if already selected).
    /// Emits a `[kagi] selected:` log for automated verification.
    /// On first selection of a row, fetches changed files on-demand and caches
    /// the result; subsequent selections of the same row reuse the cache.
    /// Clears any open diff view when the selection changes.
    pub fn select(&mut self, index: usize) {
        // Toggle: clicking the same row again deselects it.
        if self.selected == Some(index) {
            self.selected = None;
            self.file_diff_view = None;
            return;
        }
        self.selected = Some(index);
        // Clear any open file diff when the commit selection changes.
        self.file_diff_view = None;

        if let Some(detail) = self.details.get(index) {
            let parent_count = detail.parent_ids.len();
            eprintln!(
                "[kagi] selected: {} parents={}",
                detail.full_sha.as_ref().get(..8).unwrap_or(&detail.full_sha),
                parent_count,
            );
        }

        // Fetch changed files on-demand (only once per row).
        if !self.diff_cache.contains_key(&index) {
            let files_opt = self.fetch_changed_files(index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
            self.diff_cache.insert(index, files_opt);
        } else {
            // Already cached — just emit the log.
            let n = self
                .diff_cache
                .get(&index)
                .and_then(|v| v.as_ref())
                .map(|v| v.len())
                .unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
        }
    }

    /// Open the diff for the file at `file_index` in the currently selected commit.
    ///
    /// Fetches the diff via [`commit_file_diff`] and stores a pre-rendered
    /// [`FileDiffView`] in `self.file_diff_view`.  No-op if no commit is selected.
    pub fn open_file_diff(&mut self, file_index: usize) {
        use crate::git::{CommitId, commit_file_diff};

        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let detail = match self.details.get(selected) {
            Some(d) => d,
            None => return,
        };
        let files = match self.diff_cache.get(&selected).and_then(|v| v.as_ref()) {
            Some(f) => f,
            None => return,
        };
        let file_status = match files.get(file_index) {
            Some(f) => f,
            None => return,
        };

        let id = CommitId(detail.full_sha.as_ref().to_string());
        let path = file_status.path.clone();

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        match commit_file_diff(&repo, &id, &path) {
            Ok(file_diff) => {
                // Count added / removed lines for the log.
                let added: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                let hunks = file_diff.hunks.len();

                eprintln!(
                    "[kagi] diff: {} hunks={} (+{} -{})",
                    path.display(),
                    hunks,
                    added,
                    removed,
                );

                self.file_diff_view = Some(FileDiffView::from_file_diff(&file_diff, file_index));
            }
            Err(e) => {
                eprintln!("[kagi] diff error: {}", e);
            }
        }
    }

    /// Close the current file diff view and return to the changed-files list.
    pub fn close_file_diff(&mut self) {
        self.file_diff_view = None;
    }

    /// Fetch changed files for the commit at `index`.  Returns `None` on
    /// failure (so the UI can show "(diff unavailable)").
    fn fetch_changed_files(&self, index: usize) -> Option<Vec<FileStatus>> {
        use crate::git::{CommitId, commit_changed_files};

        let repo_path = self.repo_path.as_ref()?;
        let detail = self.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        let repo = git2::Repository::open(repo_path).ok()?;
        commit_changed_files(&repo, &id).ok()
    }
}

impl Render for KagiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header.clone();
        let row_count = self.rows.len();
        let selected = self.selected;

        if let Some(err) = &self.error {
            // ── Error / usage state ──────────────────────────
            let err = err.clone();
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(BG_BASE))
                .child(
                    div()
                        .text_xl()
                        .text_color(rgb(TEXT_MAIN))
                        .child(err),
                )
                .into_any();
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected.and_then(|i| self.details.get(i)).cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> = selected
            .map(|i| self.diff_cache.get(&i).cloned().unwrap_or(None));

        // Clone the file diff view if present.
        let file_diff_view = self.file_diff_view.clone();

        // ── Normal state: header + (list | list + panel) ─────
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG_BASE))
            // ── Header bar ──────────────────────────────────
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .px_3()
                    .py_1()
                    .bg(rgb(BG_SURFACE))
                    .text_color(rgb(TEXT_SUB))
                    .child(header),
            )
            // ── Body row: list (flex_1) + optional panel ─────
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .h_full()
                    // ── Virtualized commit list ──────────────
                    .child(
                        uniform_list(
                            "commit-list",
                            row_count,
                            cx.processor(move |this, range, _window, cx| {
                                render_rows(&this.rows, range, selected, cx)
                            }),
                        )
                        .flex_1()
                        .h_full(),
                    )
                    // ── Detail panel (only when a row is selected) ──
                    .when_some(detail, |el, d| {
                        if let Some(diff_view) = file_diff_view {
                            // ── Diff view mode ──────────────────
                            el.child(render_diff_panel(diff_view, cx))
                        } else {
                            // ── Commit metadata + changed files ─
                            let files = changed_files.clone();
                            let files_for_click = changed_files.clone();
                            el.child(render_detail_panel(d, files.unwrap_or(None), files_for_click.unwrap_or(None), cx))
                        }
                    }),
            )
            .into_any()
    }
}

// ──────────────────────────────────────────────────────────────
// Row renderer
// ──────────────────────────────────────────────────────────────

/// Render commit rows for the given range.  Called by `uniform_list`
/// with only the visible subset, so this must be cheap.
///
/// `selected` — the currently selected row index (None = no selection).
/// `cx` — the `Context<KagiApp>` from the `cx.processor` closure;
///         used to build `cx.listener(...)` for the on_click handler.
fn render_rows(
    rows: &[CommitRow],
    range: std::ops::Range<usize>,
    selected: Option<usize>,
    cx: &mut Context<KagiApp>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            let row = row.clone();

            // Selected row gets a prominent surface highlight;
            // even/odd stripes apply otherwise.
            let row_bg = if selected == Some(ix) {
                BG_SELECTED
            } else if ix % 2 == 0 {
                BG_BASE
            } else {
                0x1a1a2a
            };

            // ── Graph lane area (T009) ────────────────────────
            // Width is clamped to MAX_LANES lanes; unborn/empty repos
            // get lane_count=0 → graph_w=0 → no canvas rendered.
            let g_w = graph_width(row.lane_count);

            // on_click handler: update KagiApp.selected via cx.listener.
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select(ix);
                cx.notify();
            });

            div()
                .id(ix)
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px_3()
                .h(px(graph_view::ROW_H))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                .when(g_w > 0.0, |el| {
                    el.child(
                        div()
                            .w(px(g_w))
                            .h_full()
                            .flex_shrink_0()
                            .child(
                                graph_canvas(row.lane, row.edges.clone())
                                    .size_full(),
                            ),
                    )
                })
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(COLOR_SHA))
                        .child(row.short_id.clone()),
                )
                .child(render_badges(&row.badges))
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(TEXT_MAIN))
                        .overflow_hidden()
                        .child(row.summary.clone()),
                )
                .child(
                    div()
                        .w(px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_SUB))
                        .overflow_hidden()
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_MUTED))
                        .child(row.date.clone()),
                )
        })
        .collect()
}

// ──────────────────────────────────────────────────────────────
// Detail panel renderer
// ──────────────────────────────────────────────────────────────

/// Render the right-side detail panel showing commit metadata + changed files.
///
/// Each changed-file row is clickable: clicking opens the file diff view.
fn render_detail_panel(
    d: CommitDetail,
    changed_files: Option<Vec<FileStatus>>,
    changed_files_for_click: Option<Vec<FileStatus>>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Helper: one labelled field row.
    let field = |label: &'static str, value: SharedString| {
        div()
            .flex()
            .flex_col()
            .mb_2()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_LABEL))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .text_color(rgb(TEXT_MAIN))
                    .overflow_hidden()
                    .child(value),
            )
    };

    // Parents section: "none" for root commits, short ids otherwise.
    let parents_value = if d.parent_ids.is_empty() {
        SharedString::from("(root commit)")
    } else {
        SharedString::from(d.parent_ids.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("  "))
    };

    // Colour constants for change-kind badges (A/M/D/R/T).
    const COLOR_ADDED:   u32 = 0xa6e3a1; // green
    const COLOR_MODIFIED: u32 = 0xf9e2af; // yellow
    const COLOR_DELETED: u32 = 0xf38ba8; // red
    const COLOR_RENAMED: u32 = 0x89b4fa; // blue
    const COLOR_TYPECHANGE: u32 = 0x585b70; // gray (muted)

    const MAX_FILES: usize = 100;

    // Build the file rows (up to MAX_FILES) and a truncation notice.
    let (file_rows, truncated): (Vec<_>, Option<usize>) = match &changed_files {
        None => (vec![], None),
        Some(files) => {
            let total = files.len();
            let shown = files.iter().take(MAX_FILES).enumerate();
            let rows: Vec<_> = shown
                .map(|(file_index, f)| {
                    let (badge_char, badge_color) = match &f.change {
                        ChangeKind::Added      => ("A", COLOR_ADDED),
                        ChangeKind::Modified   => ("M", COLOR_MODIFIED),
                        ChangeKind::Deleted    => ("D", COLOR_DELETED),
                        ChangeKind::Renamed { .. } => ("R", COLOR_RENAMED),
                        ChangeKind::TypeChange => ("T", COLOR_TYPECHANGE),
                    };

                    // Path display: truncate with leading "…" when longer than
                    // ~40 chars.  Use chars() for correct Unicode counting.
                    const MAX_PATH_CHARS: usize = 40;
                    let path_str = f.path.to_string_lossy();
                    let display_path: String = {
                        let char_count = path_str.chars().count();
                        if char_count > MAX_PATH_CHARS {
                            let skip = char_count - (MAX_PATH_CHARS - 1);
                            let tail: String = path_str.chars().skip(skip).collect();
                            format!("\u{2026}{}", tail)
                        } else {
                            path_str.into_owned()
                        }
                    };

                    // Click handler: open the diff for this file.
                    let click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.open_file_diff(file_index);
                        cx.notify();
                    });

                    div()
                        .id(("file-row", file_index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .mb_px()
                        .on_click(click)
                        .child(
                            div()
                                .w(px(14.))
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(rgb(badge_color))
                                .child(SharedString::from(badge_char)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(rgb(TEXT_MAIN))
                                .overflow_hidden()
                                .child(SharedString::from(display_path)),
                        )
                })
                .collect();
            let truncated = if total > MAX_FILES { Some(total - MAX_FILES) } else { None };
            (rows, truncated)
        }
    };

    // Suppress unused warning for changed_files_for_click (kept for symmetry / future use).
    let _ = changed_files_for_click;

    let files_section = {
        let section_label = match &changed_files {
            None => SharedString::from("Changed files"),
            Some(files) => SharedString::from(format!("Changed files ({})", files.len())),
        };

        let mut section = div()
            .flex()
            .flex_col()
            .mt_2()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_LABEL))
                    .mb_1()
                    .child(section_label),
            );

        if changed_files.is_none() {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from("(diff unavailable)")),
            );
        } else {
            for row in file_rows {
                section = section.child(row);
            }
            if let Some(remaining) = truncated {
                section = section.child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(SharedString::from(format!("\u{2026} and {} more", remaining))),
                );
            }
        }

        section
    };

    div()
        .w(px(360.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_PANEL))
        .px_3()
        .py_2()
        // ── Full SHA ────────────────────────────────────────
        .child(field("SHA", d.full_sha))
        // ── Author ──────────────────────────────────────────
        .child(field("Author", d.author_line))
        // ── Committer (only when different from author) ──────
        .when_some(d.committer_line, |el, c| el.child(field("Committer", c)))
        // ── Parents ─────────────────────────────────────────
        .child(field("Parents", parents_value))
        // ── Message ─────────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("Message")),
                )
                .child(
                    div()
                        .text_color(rgb(TEXT_MAIN))
                        .overflow_hidden()
                        .flex_1()
                        .child(d.full_message),
                ),
        )
        // ── Changed files ─────────────────────────────────
        .child(files_section)
}

// ──────────────────────────────────────────────────────────────
// Diff panel renderer
// ──────────────────────────────────────────────────────────────

/// Render the diff view panel for a single file.
///
/// Layout:
/// - `← back` row (click to return to the changed-files list)
/// - File name
/// - Virtualized diff line list (`uniform_list` with id `"diff-list"`)
fn render_diff_panel(view: FileDiffView, cx: &mut Context<KagiApp>) -> impl IntoElement {
    let row_count = view.rows.len();
    let rows = std::sync::Arc::new(view.rows);
    let rows_for_list = rows.clone();

    // "← back" click handler: close the diff view.
    let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.close_file_diff();
        cx.notify();
    });

    div()
        .w(px(560.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_PANEL))
        .px_0()
        .py_0()
        // ── Back row ──────────────────────────────────────────
        .child(
            div()
                .id("diff-back")
                .flex()
                .flex_row()
                .items_center()
                .px_3()
                .py_1()
                .bg(rgb(BG_SURFACE))
                .on_click(back_click)
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_SUB))
                        .child(SharedString::from("\u{2190} back")),
                )
                .child(
                    div()
                        .ml_2()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(TEXT_MAIN))
                        .overflow_hidden()
                        .child(view.file_name),
                ),
        )
        // ── Diff body ──────────────────────────────────────────
        .child(
            uniform_list(
                "diff-list",
                row_count,
                cx.processor(move |_this, range, _window, _cx| {
                    render_diff_rows(&rows_for_list, range)
                }),
            )
            .flex_1()
            .h_full(),
        )
}

/// Render a range of diff rows for the `"diff-list"` uniform_list.
fn render_diff_rows(
    rows: &[DiffRow],
    range: std::ops::Range<usize>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(i, row)| match row {
            DiffRow::HunkHeader(header) => {
                div()
                    .id(("diff-hunk", i))
                    .w_full()
                    .px_2()
                    .py_px()
                    .bg(rgb(BG_SURFACE))
                    .text_sm()
                    .text_color(rgb(COLOR_DIFF_HUNK))
                    .overflow_hidden()
                    .child(header.clone())
                    .into_any()
            }
            DiffRow::Line { kind, text } => {
                let bg = match kind {
                    DiffLineKind::Added   => BG_DIFF_ADDED,
                    DiffLineKind::Removed => BG_DIFF_REMOVED,
                    DiffLineKind::Context => BG_BASE,
                };
                let text_color = match kind {
                    DiffLineKind::Added   => 0xa6e3a1u32, // green
                    DiffLineKind::Removed => 0xf38ba8u32, // red
                    DiffLineKind::Context => TEXT_MAIN,
                };
                div()
                    .id(("diff-line", i))
                    .w_full()
                    .px_2()
                    .py_px()
                    .bg(rgb(bg))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .overflow_hidden()
                    .child(text.clone())
                    .into_any()
            }
            DiffRow::Binary => {
                div()
                    .id(("diff-binary", i))
                    .w_full()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from("Binary file (no diff)"))
                    .into_any()
            }
        })
        .collect()
}

/// Render the badge chips for one commit row as a horizontal flex container.
fn render_badges(badges: &[commit_list::RefBadge]) -> impl IntoElement {
    let mut row = div().flex().flex_row().gap_1().flex_shrink_0().mr_2();
    for badge in badges {
        let color = match badge.kind {
            BadgeKind::HeadBranch => COLOR_HEAD,
            BadgeKind::Branch => COLOR_BRANCH,
            BadgeKind::Remote => COLOR_REMOTE,
            BadgeKind::Tag => COLOR_TAG,
        };
        let chip = div()
            .px_1()
            .rounded_sm()
            .bg(rgb(color))
            .text_color(rgb(BG_BASE))
            .text_sm()
            .child(badge.label.clone());
        row = row.child(chip);
    }
    row
}

// ──────────────────────────────────────────────────────────────
// Application entry point helper
// ──────────────────────────────────────────────────────────────

/// Open the GPUI window and start the event loop.
pub fn run_app(app_state: KagiApp) {
    use gpui::{Application, Bounds, WindowBounds, WindowOptions, size};

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1024.), px(768.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                let entity: Entity<KagiApp> = cx.new(|_| app_state);
                entity
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
