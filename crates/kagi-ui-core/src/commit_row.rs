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
use gpui::{div, px, rgb, Div, SharedString, Stateful};

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
    /// `+N −M`, or `"bin"` for a binary change.
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
        SharedString::from(format!(
            "+{} \u{2212}{}",
            entry.change.insertions.unwrap_or(0),
            entry.change.deletions.unwrap_or(0)
        ))
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
        CommitRowLayout::Table { row_height } => base
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .py_px()
            .h(px(row_height))
            .flex_shrink_0()
            .child(fixed_col(18., model.badge_color, true, model.badge.into()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .truncate()
                    .child(model.subject.clone()),
            )
            .child(fixed_col(
                90.,
                theme().text_sub,
                false,
                model.author.clone(),
            ))
            .child(fixed_col(
                64.,
                theme().text_muted,
                false,
                model.date_relative.clone(),
            ))
            .child(fixed_col(72., theme().text_sub, false, model.stat.clone()))
            .child(fixed_col(
                64.,
                theme().text_muted,
                false,
                model.short_hash.clone(),
            )),
        CommitRowLayout::Card => base
            .flex()
            .flex_row()
            .items_start()
            .px_2()
            .py_1()
            .gap_1()
            .child(fixed_col(14., model.badge_color, true, model.badge.into()))
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

/// One fixed-width, non-shrinking, truncating column.
fn fixed_col(width: f32, color: u32, large: bool, text: SharedString) -> Div {
    let col = div()
        .w(theme::scaled_px(width))
        .flex_shrink_0()
        .text_color(rgb(color))
        .truncate();
    if large {
        col.text_sm().child(text)
    } else {
        col.text_xs().child(text)
    }
}
