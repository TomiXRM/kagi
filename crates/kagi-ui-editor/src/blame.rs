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
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct BlameProbe {
    pub row: usize,
    pub label: String,
    pub aria_label: Option<String>,
    pub tooltip: Option<String>,
    pub tooltip_shown: Option<String>,
    pub bounds: gpui::Bounds<gpui::Pixels>,
}

impl EditorWorkspaceView {
    #[cfg(feature = "gui-e2e")]
    pub fn blame_request_for_e2e(&self) -> u64 {
        self.file_req
    }
    #[cfg(feature = "gui-e2e")]
    pub fn blame_map_pending_for_e2e(&self) -> bool {
        self.blame_map_task.is_some()
    }
    #[cfg(feature = "gui-e2e")]
    pub fn blame_map_computations_for_e2e(&self) -> u64 {
        self.blame_map_computations
    }
    pub(crate) fn invalidate_blame_map(&mut self) {
        self.blame_map_revision = self.blame_map_revision.wrapping_add(1);
        self.blame_map_task = None;
        self.blame_line_map.clear();
    }
    pub(crate) fn refresh_blame_map(&mut self, cx: &mut Context<Self>) {
        if !self.show_blame || self.blame_head.is_none() {
            self.invalidate_blame_map();
            return;
        }
        let buffer = self
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).value())
            .unwrap_or_else(|| self.content.clone().unwrap_or_default().into());
        self.update_blame_map(buffer, cx);
    }
    pub(crate) fn update_blame_map(&mut self, buffer: SharedString, cx: &mut Context<Self>) {
        self.invalidate_blame_map();
        if !self.show_blame {
            return;
        }
        let Some(head) = self.blame_head.clone() else {
            return;
        };
        let revision = self.blame_map_revision;
        #[cfg(feature = "gui-e2e")]
        {
            self.blame_map_computations += 1;
        }
        let task =
            cx.background_spawn(async move { kagi_domain::blame::buffer_line_map(&head, &buffer) });
        self.blame_map_task = Some(cx.spawn(async move |view, cx| {
            let map = task.await;
            let _ = view.update(cx, |view, cx| {
                if revision != view.blame_map_revision || !view.show_blame {
                    return;
                }
                view.blame_line_map = map;
                view.blame_map_task = None;
                cx.notify();
            });
        }));
    }
}
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
    #[cfg(feature = "gui-e2e")]
    let previous = view.blame_probe.borrow_mut().take();
    if !view.show_blame {
        return None;
    }
    let blame = view.blame.as_ref()?;
    let st = view.editor.as_ref()?.read(cx);
    let row = st.cursor_position().line as usize;
    let original = *view.blame_line_map.get(row)?;
    let line = original.and_then(|row| blame.lines.get(row));
    let end = st.text().line_end_offset(row);
    // Window-absolute end-of-line position, from the editor's last frame's
    // layout (a one-frame lag is invisible at render cadence).
    let bounds = st.range_to_bounds(&(end..end))?;
    let line_height = st.line_height().unwrap_or(px(20.));
    let label: SharedString = match line {
        Some(line) => inline_blame_label(line, kagi_ui_core::time::now_unix_secs()).into(),
        None => Msg::BlameUncommitted.t().into(),
    };
    #[cfg(feature = "gui-e2e")]
    view.blame_probe.replace(Some(BlameProbe {
        row,
        label: label.to_string(),
        aria_label: Some(label.to_string()),
        tooltip: Some(label.to_string()),
        tooltip_shown: previous
            .filter(|p| p.row == row && p.label == label.as_ref())
            .and_then(|p| p.tooltip_shown),
        bounds: Default::default(),
    }));
    Some(
        gpui::deferred(
            gpui::anchored()
                .position(gpui::point(
                    bounds.origin.x + theme::scaled_px(24.0),
                    bounds.origin.y,
                ))
                .snap_to_window()
                .child(blame_cell(view, label, line_height, cx)),
        )
        .into_any_element(),
    )
}

fn blame_cell(
    _view: &EditorWorkspaceView,
    label: SharedString,
    line_height: gpui::Pixels,
    _cx: &mut Context<EditorWorkspaceView>,
) -> impl IntoElement {
    div()
        .id("inline-blame-cell")
        .relative()
        .role(gpui::Role::Label)
        .aria_label(label.clone())
        .tooltip({
            let full = label.clone();
            #[cfg(feature = "gui-e2e")]
            let owner = _cx.entity().downgrade();
            move |window, cx| {
                #[cfg(feature = "gui-e2e")]
                let _ = owner.update(cx, |view, _| {
                    if let Some(probe) = view.blame_probe.borrow_mut().as_mut() {
                        probe.tooltip_shown = Some(full.to_string());
                    }
                });
                gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
            }
        })
        .h(line_height)
        .max_w(theme::scaled_px(560.0))
        .flex()
        .items_center()
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .overflow_hidden()
        .when(cfg!(feature = "gui-e2e"), |cell| {
            #[cfg(feature = "gui-e2e")]
            let cell = cell.child(measure_blame(_view.blame_probe.clone()));
            cell
        })
        .child(
            div()
                .min_w(px(0.))
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .child(label),
        )
}

#[cfg(feature = "gui-e2e")]
fn measure_blame(probe: std::rc::Rc<std::cell::RefCell<Option<BlameProbe>>>) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, _, _| {
            if let Some(probe) = probe.borrow_mut().as_mut() {
                probe.bounds = bounds;
            }
        },
    )
    .absolute()
    .size_full()
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
        kagi_domain::text_safety::sanitize_control_bytes(&line.author),
        kagi_ui_core::time::relative_time(line.time, now_secs),
        kagi_domain::text_safety::sanitize_control_bytes(&line.summary)
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
