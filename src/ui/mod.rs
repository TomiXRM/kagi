//! UI module — T008: GPUI commit list
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod commit_list;

use gpui::{
    App, Context, Entity, SharedString, Window,
    div, prelude::*, px, rgb, uniform_list,
};

use crate::git::{Head, RepoSnapshot};
use commit_list::{BadgeKind, CommitRow, build_commit_rows};

// ──────────────────────────────────────────────────────────────
// Catppuccin Mocha palette (subset)
// ──────────────────────────────────────────────────────────────
const BG_BASE: u32 = 0x1e1e2e;
const BG_SURFACE: u32 = 0x313244;
const TEXT_MAIN: u32 = 0xcdd6f4;
const TEXT_SUB: u32 = 0xa6adc8;
const TEXT_MUTED: u32 = 0x585b70;
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
    /// Error or informational message shown instead of the commit list.
    pub error: Option<SharedString>,
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

        eprintln!("[kagi] commit list rows: {}", rows.len());

        KagiApp { header, rows, error: None }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
            header: SharedString::from("kagi"),
            rows: Vec::new(),
            error: Some(SharedString::from(message.into())),
        }
    }
}

impl Render for KagiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header.clone();
        let row_count = self.rows.len();

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

        // ── Normal state: header + commit list ───────────────
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
            // ── Virtualized commit list ──────────────────────
            .child(
                uniform_list(
                    "commit-list",
                    row_count,
                    cx.processor(move |this, range, _window, _cx| {
                        render_rows(&this.rows, range)
                    }),
                )
                .flex_1()
                .h_full()
                .w_full(),
            )
            .into_any()
    }
}

// ──────────────────────────────────────────────────────────────
// Row renderer
// ──────────────────────────────────────────────────────────────

/// Render commit rows for the given range.  Called by `uniform_list`
/// with only the visible subset, so this must be cheap.
fn render_rows(
    rows: &[CommitRow],
    range: std::ops::Range<usize>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            let row = row.clone();
            // Alternate row background for readability.  Parity must come
            // from the absolute row index so stripes stay attached to rows
            // while scrolling.
            let row_bg = if ix % 2 == 0 { BG_BASE } else { 0x1a1a2a };

            div()
                .id(ix)
                .flex()
                .flex_row()
                .w_full()
                .px_3()
                .py(px(3.))
                .bg(rgb(row_bg))
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
