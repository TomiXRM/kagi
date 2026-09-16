//! The sidebar's WORKTREES leaf row.
//!
//! Split out of `sidebar.rs` (a known oversized god-file, see CLAUDE.md) when
//! #733 gave the row its context menu.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};

use super::sidebar::{name_tooltip, SIDEBAR_ROW_H};
use super::theme::{self, theme};
use super::KagiApp;

/// A worktree leaf (✓ marks the current worktree). Right-click opens the shared
/// worktree menu.
///
/// `path` is the working-tree path itself, never `path_label`: the label is
/// display text — lossy for non-UTF-8 paths and control-byte sanitized — so the
/// menu's path actions would target the wrong directory if parsed back from it.
pub(super) fn build_worktree_row(
    name: &str,
    path: &std::path::Path,
    path_label: &str,
    is_current: bool,
    is_main: bool,
    locked: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // issue #356: worktree name/path are remote/filesystem-origin text —
    // neutralize control bytes in the visible label.
    let name_s = kagi_domain::text_safety::sanitize_control_bytes(name);
    let path_s = kagi_domain::text_safety::sanitize_control_bytes(path_label);
    let label = if is_current {
        SharedString::from(format!("\u{2713} {}  {}", name_s, path_s))
    } else {
        SharedString::from(format!("{}  {}", name_s, path_s))
    };
    let full_name = label.clone();
    let text_color = if is_current {
        theme().color_success
    } else {
        theme().text_sub
    };
    let mut row = div()
        .id(SharedString::from(format!("sidebar-worktree-{}", name)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        // w_full: without it the row sizes to its content and runs past the
        // sidebar clip — the trailing lock chip was never visible and the
        // label never ellipsized (other sidebar rows are width-bounded).
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .tooltip(name_tooltip(full_name))
        // min_w(0): without it the flex item sizes to the (long) path label's
        // min-content width and pushes the lock chip past the clipped row edge
        // — the indicator never showed at all (user report).
        .child(div().flex_1().min_w(px(0.)).truncate().child(label));
    if locked {
        // 🔐 reads at a glance where the muted "locked" text was easy to miss
        // next to the path label (user feedback).
        row = row.child(div().flex_shrink_0().text_xs().child("🔐"));
    }
    // The lifecycle items (remove / lock / unlock) are linked worktrees only —
    // the main worktree is never lockable or removable from kagi — but the path
    // items are not, so the main row gets the menu too: from a linked
    // worktree's tab, "Open in new tab" on the main row is the way back (#733).
    // `build_worktree_menu` drops the lifecycle groups when `is_main`.
    let name_for_menu = name.to_string();
    let path_for_menu = path.to_path_buf();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_worktree_menu(
                name_for_menu.clone(),
                locked,
                is_main,
                Some(path_for_menu.clone()),
                event.position,
            );
            cx.stop_propagation();
            cx.notify();
        },
    );
    row.on_mouse_down(gpui::MouseButton::Right, menu_handler)
        .hover(|style| style.bg(rgb(theme().surface)))
        .into_any()
}
