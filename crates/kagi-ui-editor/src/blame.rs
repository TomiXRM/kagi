//! Inline blame (issue #350 / ADR-0162): the render half of the Editor
//! Workspace's blame UI — a dim `author · relative-date · summary` label
//! floated at the end of the cursor line. The state half (toggle, lazy
//! load request, seed) lives on `EditorWorkspaceView` in `lib.rs`; the
//! `Backend::blame_file` call itself is bin-owned (this crate never sees
//! the Git backend).

use gpui::prelude::*;
use gpui::{div, px, rgb, Context, SharedString};

use kagi_ui_core::i18n::Msg;
use kagi_ui_core::theme::{self, theme};

use crate::EditorWorkspaceView;

/// The inline-blame label — positioned via the editor's own line layout.
/// Returns `None` whenever there is nothing to draw: toggle off, blame not
/// loaded yet, or the cursor line is scrolled out of view
/// (`range_to_bounds` only resolves offsets in the visible range — that is
/// the clamp).
pub(crate) fn render_inline_blame(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<gpui::AnyElement> {
    use gpui_component::input::RopeExt as _;
    if !view.show_blame {
        return None;
    }
    let blame = view.blame.as_ref()?;
    let st = view.editor.as_ref()?.read(cx);
    let row = st.cursor_position().line as usize;
    // `BlameLine`s are in file order (line 1 first) — index by row. A row
    // past the blamed length (e.g. unsaved lines typed at EOF) draws nothing.
    let line = blame.lines.get(row)?;
    let end = st.text().line_end_offset(row);
    // Window-absolute end-of-line position, from the editor's last frame's
    // layout (a one-frame lag is invisible at render cadence).
    let bounds = st.range_to_bounds(&(end..end))?;
    let line_height = st.line_height().unwrap_or(px(20.));
    let label = inline_blame_label(line, kagi_ui_core::time::now_unix_secs());
    Some(
        gpui::deferred(
            gpui::anchored()
                .position(gpui::point(
                    bounds.origin.x + theme::scaled_px(24.0),
                    bounds.origin.y,
                ))
                .snap_to_window()
                .child(
                    div()
                        .h(line_height)
                        .max_w(theme::scaled_px(560.0))
                        .flex()
                        .items_center()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .overflow_hidden()
                        .child(div().truncate().child(SharedString::from(label))),
                ),
        )
        .into_any_element(),
    )
}

/// Pure text half of [`render_inline_blame`], split out for unit tests.
/// Symbol markers per ADR-0162: `?` (unblamable — the whole label is the
/// explanation, there is no commit to describe) and `*` (ignored — prefixes
/// the normal label).
fn inline_blame_label(line: &kagi_domain::blame::BlameLine, now_secs: i64) -> String {
    if line.unblamable {
        return Msg::BlameUnblamableMarkTip.t().to_string();
    }
    let mark = match line.mark() {
        Some(m) => format!("{m} "),
        None => String::new(),
    };
    format!(
        "{mark}{} · {} · {}",
        line.author,
        kagi_ui_core::time::relative_time(line.time, now_secs),
        line.summary
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blame_line(ignored: bool, unblamable: bool) -> kagi_domain::blame::BlameLine {
        kagi_domain::blame::BlameLine {
            line_no: 1,
            commit: "a".repeat(40),
            short_commit: "aaaaaaa".into(),
            author: "Alice".into(),
            time: 1_000_000,
            summary: "fix the widget".into(),
            ignored,
            unblamable,
        }
    }

    #[test]
    fn inline_blame_label_formats_author_date_summary() {
        let now = 1_000_000 + 2 * 86_400; // 2 days later
        assert_eq!(
            inline_blame_label(&blame_line(false, false), now),
            "Alice · 2d ago · fix the widget"
        );
    }

    #[test]
    fn inline_blame_label_prefixes_ignored_mark() {
        let now = 1_000_000 + 120;
        assert_eq!(
            inline_blame_label(&blame_line(true, false), now),
            "* Alice · 2m ago · fix the widget"
        );
    }

    #[test]
    fn inline_blame_label_unblamable_is_the_tip_text() {
        let label = inline_blame_label(&blame_line(false, true), 0);
        assert!(label.starts_with('?'), "unblamable label keeps the ? mark");
        assert!(!label.contains("Alice"), "no attribution on unblamable");
    }
}
