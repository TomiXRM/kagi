//! One row of a file's commit history, shared by File History's full-width
//! list and the Editor Workspace's narrow History tab.
//!
//! The two panes render the *same* row data in two genuinely different
//! shapes — a six-column table across the center pane vs. a two-line card in
//! a ~380px sidebar — so the layout is the parameter
//! ([`CommitRowLayout`]) and everything feeding it is shared:
//! [`CommitRowModel`] derives every display string once (badge, subject,
//! author, both date formats, `+ins −del`, short hash), and
//! [`row_background`] owns the zebra/selection rule both lists were
//! copy-pasting.
//!
//! [`render_commit_row`] returns a `Stateful<Div>` rather than a finished
//! element, so each pane attaches its own interactions — File History adds a
//! double-click-to-jump handler and a right-click context menu, the Editor
//! tab only needs a single click — without this module knowing about either
//! entity type or growing callback parameters for handlers only one caller
//! uses.

use gpui::prelude::*;
use gpui::{div, px, relative, rgb, Div, SharedString, Stateful};

use kagi_domain::file_history::{FileHistoryEntry, FileHistoryEntryKind};

use crate::change_badge::entry_badge;
use crate::theme::{self, theme};
use crate::time::relative_time;

/// Which shape [`render_commit_row`] lays the row out in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommitRowLayout {
    /// File History's full-width single-line table row: badge, subject, then
    /// fixed-width author / relative-date / diffstat / short-hash columns.
    /// `row_height` is the caller's zoom-scaled row height.
    Table { row_height: f32 },
    /// The Editor Workspace History tab's compact two-line card: badge in a
    /// narrow leading column, then subject over a `hash · author · date`
    /// meta line. No diffstat column — there's no width for one.
    Card,
}

/// Every display string a commit row needs, derived once from the entry.
///
/// Both date forms are precomputed because the two layouts want different
/// ones (the wide row has space for `"3d ago"`, the narrow card shows a
/// stable `YYYY-MM-DD`) and deriving them is cheaper than branching per
/// layout at every call site.
pub struct CommitRowModel {
    pub badge: &'static str,
    pub badge_color: u32,
    pub subject: SharedString,
    pub author: SharedString,
    /// Relative time (`"3d ago"`), for [`CommitRowLayout::Table`].
    pub date_relative: SharedString,
    /// `YYYY-MM-DD`, for [`CommitRowLayout::Card`].
    pub date_ymd: SharedString,
    /// `+N −M`, `"bin"` for a binary change, or empty when the entry carries
    /// no line counts (the WIP row, built from `git status`; #813).
    pub stat: SharedString,
    pub short_hash: SharedString,
}

/// Derive a row's display data. `now` is the epoch-seconds reference for the
/// relative date (callers pass `time::now_unix_secs()` once per list render,
/// not per row).
pub fn commit_row_model(entry: &FileHistoryEntry, now: i64) -> CommitRowModel {
    let (badge, badge_color) = entry_badge(entry);

    let (subject, author, date_relative, date_ymd, short_hash) =
        if entry.kind == FileHistoryEntryKind::Wip {
            (
                SharedString::from("WIP \u{2014} Uncommitted changes"),
                SharedString::from(""),
                SharedString::from(""),
                SharedString::from(""),
                SharedString::from(""),
            )
        } else if let Some(c) = entry.commit.as_ref() {
            let relative = crate::time_parse::iso_to_epoch(&c.author_date)
                .map(|e| relative_time(e, now))
                .unwrap_or_default();
            // `author_date` is `git log`'s strict-ISO8601 (`%aI`), so the
            // leading 10 bytes are always `YYYY-MM-DD` — no parse needed.
            let ymd = c.author_date.get(0..10).unwrap_or("").to_string();
            (
                SharedString::from(c.subject.clone()),
                SharedString::from(c.author_name.clone()),
                SharedString::from(relative),
                SharedString::from(ymd),
                SharedString::from(c.short_hash.clone()),
            )
        } else {
            (
                SharedString::from("(unknown)"),
                SharedString::from(""),
                SharedString::from(""),
                SharedString::from(""),
                SharedString::from(""),
            )
        };

    let stat = if entry.change.is_binary {
        SharedString::from("bin")
    } else {
        match (entry.change.insertions, entry.change.deletions) {
            (Some(ins), Some(del)) => SharedString::from(format!("+{ins} \u{2212}{del}")),
            // #813: no numstat → say nothing rather than "+0 −0"; the diff
            // pane shows the real working-tree change.
            _ => SharedString::default(),
        }
    };

    CommitRowModel {
        badge,
        badge_color,
        subject,
        author,
        date_relative,
        date_ymd,
        stat,
        short_hash,
    }
}

/// Row background: selection wins, then zebra striping by visible position.
pub fn row_background(is_selected: bool, ix: usize) -> u32 {
    if is_selected {
        theme().selected
    } else if ix % 2 == 1 {
        theme().bg_row_alt
    } else {
        theme().panel
    }
}

/// Render a commit row in `layout`. Attach interactions on the result
/// (`.on_click(..)`, `.on_mouse_down(..)`) — see the module docs.
///
/// `id` namespaces the element id per caller so two lists on screen at once
/// can't collide.
pub fn render_commit_row(
    id: &'static str,
    ix: usize,
    model: &CommitRowModel,
    layout: CommitRowLayout,
    is_selected: bool,
) -> Stateful<Div> {
    let base = div()
        .id((id, ix))
        .w_full()
        .bg(rgb(row_background(is_selected, ix)))
        .cursor_pointer()
        // Hover uses the subtle `surface` tint (like the commit panel / branch
        // list), NOT `selected` — using the selection colour made a hovered row
        // indistinguishable from the selected one, so the row the mouse was left
        // on after a click looked "still selected" while the arrows moved the
        // real selection elsewhere. The selected row keeps its colour on hover.
        .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))));

    match layout {
        CommitRowLayout::Table { row_height } => {
            let row = base
                .flex()
                .flex_row()
                .items_center()
                .px_3()
                .h(px(row_height))
                .flex_shrink_0()
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .line_height(relative(1.2))
                .child(table_col(
                    ix,
                    "badge",
                    16.,
                    model.badge_color,
                    model.badge.into(),
                ))
                .child(
                    table_text(ix, "subject", theme().text_main, model.subject.clone())
                        .flex_1()
                        .min_w(px(0.)),
                )
                .child(
                    table_col(ix, "author", 90., theme().text_sub, model.author.clone())
                        .min_w(px(0.))
                        .flex_shrink(1.),
                )
                .child(table_col(
                    ix,
                    "date",
                    64.,
                    theme().text_muted,
                    model.date_relative.clone(),
                ))
                .child(
                    table_col(ix, "stat", 72., theme().text_sub, model.stat.clone())
                        .min_w(px(0.))
                        .flex_shrink(1.),
                )
                .child(table_col(
                    ix,
                    "hash",
                    64.,
                    theme().text_muted,
                    model.short_hash.clone(),
                ));
            #[cfg(feature = "gui-e2e")]
            let row = row.relative().child(table_probe(ix, "row"));
            row
        }
        CommitRowLayout::Card => base
            .flex()
            .flex_row()
            .items_start()
            .px_2()
            .py_1()
            .gap_1()
            .child(fixed_col(14., model.badge_color, model.badge.into()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().text_main))
                            .truncate()
                            .child(model.subject.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(model.short_hash.clone())
                            .child(model.author.clone())
                            .child(model.date_ymd.clone()),
                    ),
            ),
    }
}

/// Table columns inherit File History's Finder font and leading.
fn table_text(ix: usize, part: &'static str, color: u32, text: SharedString) -> Div {
    let col = div().text_color(rgb(color)).truncate().child(text);
    #[cfg(feature = "gui-e2e")]
    let col = col.relative().child(table_probe(ix, part));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = (ix, part);
    col
}

fn table_col(ix: usize, part: &'static str, width: f32, color: u32, text: SharedString) -> Div {
    table_text(ix, part, color, text)
        .w(theme::scaled_px(width))
        .flex_shrink_0()
}

/// One fixed-width, non-shrinking, truncating column.
fn fixed_col(width: f32, color: u32, text: SharedString) -> Div {
    div()
        .w(theme::scaled_px(width))
        .flex_shrink_0()
        .text_color(rgb(color))
        .truncate()
        .text_sm()
        .child(text)
}

/// Actual table paint geometry for native File History density/containment tests.
/// `"row"` is the inner frame: `RowList` reserves `keyboard_nav::RING` on each
/// edge of the full fixed-height row. Text parts are `"badge"`, `"subject"`,
/// `"author"`, `"date"`, `"stat"` and `"hash"`.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Copy, Debug)]
pub struct TableRowPaint {
    pub bounds: gpui::Bounds<gpui::Pixels>,
    pub mask: gpui::Bounds<gpui::Pixels>,
    pub font_size: gpui::Pixels,
    pub font_weight: gpui::FontWeight,
    pub line_height: gpui::Pixels,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static TABLE_ROW_PAINTS: std::cell::RefCell<std::collections::HashMap<
        (gpui::WindowId, usize, &'static str), TableRowPaint
    >> = std::cell::RefCell::new(Default::default());
}

/// Read a painted File History table row or column, scoped to its native window.
#[cfg(feature = "gui-e2e")]
pub fn table_row_paint(
    window: gpui::WindowId,
    ix: usize,
    part: &'static str,
) -> Option<TableRowPaint> {
    TABLE_ROW_PAINTS.with(|paints| paints.borrow().get(&(window, ix, part)).copied())
}

/// Clear a window before drawing so offscreen rows cannot reuse stale geometry.
#[cfg(feature = "gui-e2e")]
pub fn clear_table_row_paints(window: gpui::WindowId) {
    TABLE_ROW_PAINTS.with(|paints| paints.borrow_mut().retain(|(id, _, _), _| *id != window));
}

/// An absolute inside probe, never a wrapper: native test layout equals production.
#[cfg(feature = "gui-e2e")]
fn table_probe(ix: usize, part: &'static str) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let style = window.text_style();
            let paint = TableRowPaint {
                bounds,
                mask: window.content_mask().bounds,
                font_size: style.font_size.to_pixels(window.rem_size()),
                font_weight: style.font_weight,
                line_height: style.line_height_in_pixels(window.rem_size()),
            };
            TABLE_ROW_PAINTS.with(|paints| {
                paints
                    .borrow_mut()
                    .insert((window.window_handle().window_id(), ix, part), paint);
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
