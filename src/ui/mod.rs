//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list
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

use crate::git::{ChangeKind, FileStatus, Head, RepoSnapshot};
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
        }
    }

    /// Select the commit at `index` (or deselect if already selected).
    /// Emits a `[kagi] selected:` log for automated verification.
    /// On first selection of a row, fetches changed files on-demand and caches
    /// the result; subsequent selections of the same row reuse the cache.
    pub fn select(&mut self, index: usize) {
        // Toggle: clicking the same row again deselects it.
        if self.selected == Some(index) {
            self.selected = None;
            return;
        }
        self.selected = Some(index);

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
                        // Pair the CommitDetail with the changed-files list.
                        // changed_files is Some(...) when a row is selected.
                        let files = changed_files.clone();
                        el.child(render_detail_panel(d, files.unwrap_or(None)))
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
            // cx.listener signature:
            //   fn listener<E: ?Sized>(
            //       &self,
            //       f: impl Fn(&mut T, &E, &mut Window, &mut Context<T>) + 'static,
            //   ) -> impl Fn(&E, &mut Window, &mut App) + 'static
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select(ix);
                // State changed — request a re-render so the highlight and
                // detail panel actually update on screen.
                cx.notify();
            });

            div()
                .id(ix)
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px_3()
                // Fixed row height with NO vertical padding: the graph canvas
                // must span the full row so Pass edges connect seamlessly
                // across row boundaries.
                .h(px(graph_view::ROW_H))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                // Graph lane canvas — wrapped in a sized div so we can call
                // .w() / .h() on the container (canvas returns opaque type).
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
                // Short SHA (monospace-ish, muted teal)
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(COLOR_SHA))
                        .child(row.short_id.clone()),
                )
                // Badge chips
                .child(render_badges(&row.badges))
                // Summary (fills remaining space)
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(TEXT_MAIN))
                        .overflow_hidden()
                        .child(row.summary.clone()),
                )
                // Author
                .child(
                    div()
                        .w(px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_SUB))
                        .overflow_hidden()
                        .child(row.author.clone()),
                )
                // Date
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

/// Render the 360 px right-side detail panel for the selected commit.
///
/// `changed_files` is `None` when the diff has not been loaded or failed
/// (shows "(diff unavailable)"), or `Some(Vec<FileStatus>)` with the list.
fn render_detail_panel(d: CommitDetail, changed_files: Option<Vec<FileStatus>>) -> impl IntoElement {
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

    let mut panel = div()
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
        .child(field("Author", d.author_line));

    // Committer (only when different from author)
    if let Some(committer) = d.committer_line {
        panel = panel.child(field("Committer", committer));
    }

    // ── Changed files section ────────────────────────────────────────────────
    // Colour constants for change-kind badges (A/M/D/R/T).
    const COLOR_ADDED:   u32 = 0xa6e3a1; // green
    const COLOR_MODIFIED: u32 = 0xf9e2af; // yellow
    const COLOR_DELETED: u32 = 0xf38ba8; // red
    const COLOR_RENAMED: u32 = 0x89b4fa; // blue
    const COLOR_TYPECHANGE: u32 = 0x585b70; // gray (muted)

    const MAX_FILES: usize = 100;

    // Build the file rows (up to MAX_FILES) and a truncation notice.
    let (file_rows, truncated): (Vec<_>, Option<usize>) = match &changed_files {
        None => {
            // Diff unavailable: single notice row.
            (vec![], None)
        }
        Some(files) => {
            let total = files.len();
            let shown = files.iter().take(MAX_FILES);
            let rows: Vec<_> = shown
                .map(|f| {
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
                            // Keep the last MAX_PATH_CHARS-1 chars (to leave room for "…").
                            let skip = char_count - (MAX_PATH_CHARS - 1);
                            let tail: String = path_str.chars().skip(skip).collect();
                            format!("\u{2026}{}", tail)
                        } else {
                            path_str.into_owned()
                        }
                    };

                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .mb_px()
                        .child(
                            // Kind badge: single letter in a small colored chip.
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
            // Diff unavailable.
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

    panel
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
