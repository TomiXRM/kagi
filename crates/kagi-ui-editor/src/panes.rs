//! Right-pane History tab + center-pane Diff/Snapshot tabs (T-WS-EDITOR-008).
//!
//! Kept in a sibling module rather than growing `lib.rs` further (CLAUDE.md:
//! prefer a focused sibling module over an oversized file already past the
//! 800-LOC target).

use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, relative, rgb, uniform_list, AnyElement, Context};

use kagi_domain::file_history::{CommitSummary, FileHistoryEntry, FileHistoryEntryKind};
use kagi_ui_core::avatar::AvatarImages;
use kagi_ui_core::commit_header::render_commit_header;
use kagi_ui_core::commit_row::{commit_row_model, render_commit_row, CommitRowLayout};
use kagi_ui_core::i18n::Msg;
use kagi_ui_core::theme::theme;

use crate::named_button::NamedButton;
use gpui_component::{
    button::{Button, ButtonVariants},
    Disableable, Sizable,
};

impl EditorWorkspaceView {
    pub fn details_visible(&self) -> bool {
        self.details_visible
    }

    pub fn toggle_details(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.set_details_visible(!self.details_visible, window, cx);
    }

    /// Changes presentation only: selected revision, document and cached width stay owned here.
    pub fn set_details_visible(
        &mut self,
        visible: bool,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.details_visible == visible {
            return;
        }
        self.details_visible = visible;
        if !visible {
            if self
                .details_focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
            {
                window.focus(&self.root_focus, cx);
            }
            self.hooks.pane_selectors.release_hidden(self, window, cx);
        }
        cx.notify();
    }

    pub fn details_resizing_available(&self) -> bool {
        self.details_visible && !self.details_motion.animating(kagi_ui_core::motion::now())
    }

    pub fn pane_widths_animating(&self) -> bool {
        self.details_motion.animating(kagi_ui_core::motion::now())
    }

    pub(crate) fn prepare_details(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let now = kagi_ui_core::motion::now();
        if self.details_focus.is_none() {
            self.details_focus = Some(cx.focus_handle());
        }
        if self.details_visible && !self.details_motion.animating(now) {
            self.details_width = kagi_ui_core::theme::scaled_px(self.hunks_w)
                * self.pane_width_fraction_with_details(self.pane_bounds(window).size.width, true);
        }
        self.details_motion
            .sync(self.details_visible, now, kagi_ui_core::motion::instant());
        kagi_ui_core::motion::request_frame(window, self.details_motion.animating(now));
    }
}

/// Quiet, always-available header restoration control, including empty/error states.
pub(crate) fn render_details_toggle(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    use gpui_component::Selectable as _;
    use kagi_ui_core::tooltip::HoverTooltip as _;
    let name: gpui::SharedString = if view.details_visible {
        Msg::EditorDetailsHide.t()
    } else {
        Msg::EditorDetailsShow.t()
    }
    .into();
    let button = Button::new("editor-details-toggle")
        .ghost()
        .small()
        .compact()
        .flex_shrink_0()
        .icon(gpui_component::IconName::PanelRight)
        .selected(view.details_visible)
        .on_click(cx.listener(|view, _, window, cx| view.toggle_details(window, cx)));
    let button = NamedButton(button)
        .role(gpui::Role::Button)
        .aria_label(name.clone())
        .aria_toggled(if view.details_visible {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .hover_tooltip(("editor-details-toggle-help", 0usize), name)
        .into_any_element();
    #[cfg(feature = "gui-e2e")]
    let button = view.hooks.pane_selectors.measure_toggle(button);
    button
}

/// Keep the real content at its settled width while only the outer boxes retract.
pub(crate) fn render_details(
    view: &EditorWorkspaceView,
    width: gpui::Pixels,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<AnyElement> {
    use kagi_ui_core::{
        divider::{DividerDrag, DividerGhost, DividerKind},
        motion::{self, Anchor, Axis},
        theme,
    };
    let sample = view
        .details_motion
        .sample(view.details_width, motion::now());
    if !sample.retain_content {
        return None;
    }
    let width = if sample.done && view.details_visible {
        width
    } else {
        view.details_width
    };
    let divider = div()
        .id("ews-divider-hunks")
        .size_full()
        .bg(rgb(theme().panel))
        .flex()
        .justify_center()
        .child(div().w(gpui::px(1.)).h_full().bg(rgb(theme().surface)))
        .when(view.details_resizing_available(), |divider| {
            divider
                .hover(|style| style.bg(rgb(theme().ui_accent())))
                .cursor_col_resize()
                .on_drag(
                    DividerDrag {
                        kind: DividerKind::EditorHunks,
                    },
                    |_, _, _, cx| cx.new(|_| DividerGhost),
                )
        });
    let divider = motion::clip(
        "editor-details-divider",
        Axis::Horizontal,
        Anchor::Start,
        theme::scaled_px(4.),
        sample.fraction,
        divider,
    )
    .when(cfg!(feature = "gui-e2e"), |divider| {
        #[cfg(feature = "gui-e2e")]
        let divider = divider
            .child(view.hooks.pane_selectors.measure("editor-details-divider"))
            .child(crate::tab_probes::measure_divider(&view.tab_probes, false));
        divider
    });
    let pane = motion::clip(
        "editor-details-pane",
        Axis::Horizontal,
        Anchor::Start,
        width,
        sample.fraction,
        crate::render_hunks_pane(view, width, cx),
    )
    .when(!view.details_visible, |pane| {
        // Exit-only drawing cannot receive pointer input or acquire focus.
        pane.child(div().absolute().size_full().occlude())
    })
    .when(cfg!(feature = "gui-e2e"), |pane| {
        #[cfg(feature = "gui-e2e")]
        let pane = pane.child(view.hooks.pane_selectors.measure("editor-details-pane"));
        pane
    });
    Some(
        div()
            .h_full()
            .flex()
            .flex_shrink_0()
            .children([divider.into_any_element(), pane.into_any_element()])
            .into_any_element(),
    )
}

/// Bound untrusted stderr before sanitizing, then bound the escaped preview.
/// The complete backend cause remains on the file owner.
fn history_error_preview(error: &str) -> String {
    const CHARS: usize = 480;
    let mut lines = 1;
    let mut end = error.len();
    for (count, (offset, ch)) in error.char_indices().enumerate() {
        if count == CHARS || (ch == '\n' && lines == 3) {
            end = offset;
            break;
        }
        if ch == '\n' {
            lines += 1;
        }
    }
    let mut preview = kagi_domain::text_safety::sanitize_control_bytes(&error[..end]);
    let escaped_end = preview.char_indices().nth(CHARS).map(|(offset, _)| offset);
    if let Some(offset) = escaped_end {
        preview.truncate(offset);
    }
    if end < error.len() || escaped_end.is_some() {
        preview.push('…');
    }
    preview
}

#[cfg(feature = "gui-e2e")]
fn measure_history_error(cx: &mut Context<EditorWorkspaceView>, retry: bool) -> AnyElement {
    let entity = cx.entity().downgrade();
    gpui::canvas(
        move |bounds, _, cx| {
            let _ = entity.update(cx, |view, _| {
                if retry {
                    view.history_retry_bounds = Some(bounds);
                } else {
                    view.history_error_bounds = Some(bounds);
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// Fixed top:bottom ratio for the History pane's commit-detail header vs.
/// the commit list (user request — "3:7"; not a drag-resizable
/// `InspectorSplit`-style divider, at least not in this v1).
const HISTORY_HEADER_RATIO: f32 = 0.3;

use crate::{placeholder_text, with_vertical_scrollbar, EditorWorkspaceView};

/// Right pane's Diff/History selector, using the host's shared keyboard owner.
pub(crate) fn render_right_pane_tabs(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    view.hooks.pane_selectors.render(view, false, cx)
}

/// Center pane's Diff/Snapshot selector for the selected historical commit.
pub(crate) fn render_middle_pane_tabs(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    view.hooks.pane_selectors.render(view, true, cx)
}

/// Right pane's History tab body: a virtualized, clickable commit list for
/// the open file (`view.history`, ADR-0089's data layer via `seed_history`).
pub(crate) fn render_history_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    #[cfg(feature = "gui-e2e")]
    view.history_presentation.borrow_mut().clear();
    if view.history_loading() {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    }
    if let Some(error) = &view.history_error {
        let heading = Msg::EditorHistoryFailed.t();
        let retry = Msg::EditorHistoryRetry.t();
        let error = history_error_preview(error);
        #[cfg(feature = "gui-e2e")]
        {
            *view.history_presentation.borrow_mut() = format!("{heading}\n{error}\n{retry}");
        }
        let button = Button::new("ews-history-retry")
            .ghost()
            .small()
            .label(retry)
            .disabled(!view.details_visible())
            .on_click(cx.listener(|view, _, window, cx| {
                if view.details_visible() {
                    view.retry_history(cx);
                    window.focus(&view.root_focus, cx);
                }
            }));
        #[cfg(feature = "gui-e2e")]
        let button = button.on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| {
            if event.keystroke.key == "f19" {
                view.history_retry_focused = true;
                cx.stop_propagation();
            }
        }));
        return div()
            .id("ews-history-error")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_3()
            .text_sm()
            .text_color(rgb(theme().text_main))
            .child(div().flex_shrink_0().child(heading))
            .child(
                div()
                    .id("ews-history-cause")
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .overflow_y_scroll()
                    .child(div().w_full().child(error)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .relative()
                    .child(
                        NamedButton(button)
                            .role(gpui::Role::Button)
                            .aria_label(retry),
                    )
                    .when(cfg!(feature = "gui-e2e"), |el| {
                        #[cfg(feature = "gui-e2e")]
                        let el = el.child(measure_history_error(cx, true));
                        el
                    }),
            )
            .relative()
            .when(cfg!(feature = "gui-e2e"), |el| {
                #[cfg(feature = "gui-e2e")]
                let el = el.child(measure_history_error(cx, false));
                el
            })
            .into_any_element();
    }
    let Some(history) = view.history.as_ref() else {
        #[cfg(feature = "gui-e2e")]
        {
            *view.history_presentation.borrow_mut() = Msg::EditorHistoryEmpty.t().into();
        }
        return placeholder_text(Msg::EditorHistoryEmpty.t()).into_any_element();
    };
    if history.entries.is_empty() {
        #[cfg(feature = "gui-e2e")]
        {
            *view.history_presentation.borrow_mut() = Msg::EditorHistoryEmpty.t().into();
        }
        return placeholder_text(Msg::EditorHistoryEmpty.t()).into_any_element();
    }

    let selected_commit = view
        .selected_history_commit
        .as_deref()
        .and_then(|hash| history.entry_by_hash(hash))
        .and_then(|e| e.commit.as_ref());
    let header = render_history_header(selected_commit, &view.avatars);

    let history = Arc::clone(history);
    let row_count = history.entries.len();
    let scroll_handle = view.history_scroll.clone();
    let scrollbar_handle = scroll_handle.clone();
    let selected = view.selected_history_commit.clone();
    // One clock read per list render, not per row.
    let now = kagi_ui_core::time::now_unix_secs();
    let list = with_vertical_scrollbar(
        "ews-history-scroll",
        &scrollbar_handle,
        uniform_list(
            "ews-history-list",
            row_count,
            cx.processor(move |this, range: Range<usize>, window, cx| {
                range
                    .filter_map(|i| {
                        render_history_row(
                            this,
                            i,
                            history.entries.get(i)?,
                            &selected,
                            now,
                            window.is_window_active(),
                            cx,
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h(px(0.)),
        false,
    );
    // Fixed 3:7 ratio (user request), not a drag divider — both boxes use
    // `flex_basis(relative(_)) + flex_shrink(1.)` with no explicit
    // `flex_grow`, the same recipe `inspector.rs`'s `InspectorSplit` uses to
    // avoid the wrapped-text-feeds-back-into-layout jitter that plain
    // `overflow_y_scroll` directly on a flex-sized box causes.
    let list_box = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(1.0 - HISTORY_HEADER_RATIO))
        .flex_shrink(1.)
        .child(list);
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(header)
        .child(list_box)
        .into_any_element()
}

/// Top of the History pane: the selected commit's subject/meta/body via the
/// shared `kagi_ui_core::commit_header::render_commit_header` — also used by
/// File History's detail pane (see that function's doc comment for why Graph
/// mode's Inspector isn't folded in too). This wrapper supplies only the
/// Editor-specific layout: a fixed 30% box with its own inner scroll (same
/// double-div anti-jitter technique `inspector.rs`'s `InspectorSplit` uses —
/// `overflow_hidden` on the sized outer box, `overflow_y_scroll` on an
/// unconstrained inner child, so wrapped text can't feed back into the flex
/// solve).
fn render_history_header(commit: Option<&CommitSummary>, avatars: &AvatarImages) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(HISTORY_HEADER_RATIO))
        .flex_shrink(1.)
        .overflow_hidden()
        .px_2()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .id("ews-history-header-scroll")
                .size_full()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(render_commit_header(commit, avatars)),
        )
        .into_any_element()
}

/// One row in the History list, rendered through the shared
/// `kagi_ui_core::commit_row` in its `Card` layout — the same model and
/// styling File History's full-width `Table` rows use, differing only in
/// shape. Only the click handler is local: this list drives the center
/// pane's Diff/Snapshot tabs, where File History instead opens its own
/// inline diff (and adds a double-click jump + context menu).
///
/// `include_wip: false` on the backing request (see `start_editor_history_load`
/// in the bin) means every entry here is a real, selectable commit — the WIP
/// row is never duplicated into this list (the Diff tab already covers it).
fn render_history_row(
    _view: &EditorWorkspaceView,
    ix: usize,
    entry: &FileHistoryEntry,
    selected: &Option<String>,
    now: i64,
    window_active: bool,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<AnyElement> {
    debug_assert_ne!(entry.kind, FileHistoryEntryKind::Wip);
    let commit = entry.commit.as_ref()?;
    let is_selected = selected.as_deref() == Some(commit.full_hash.as_str());
    let hash = commit.full_hash.clone();
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        if this.details_visible() {
            this.select_history_commit(hash.clone(), window, cx);
        }
    });

    let model = commit_row_model(entry, now);
    Some(
        render_commit_row(
            "ews-history-row",
            ix,
            &model,
            CommitRowLayout::Card,
            is_selected,
            window_active,
        )
        .on_click(click)
        .into_any_element(),
    )
}

/// Immutable selectable historical text, distinct from the live buffer.
pub(crate) fn render_snapshot_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    if view.snapshot_loading() {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    }
    // Bug report: this used to also show "Loading…" here, forever — but a
    // finished load that found nothing (e.g. this commit predates a rename
    // and the path didn't exist yet) is a distinct, resolved state, not a
    // stuck spinner.
    let Some(snapshot) = view.snapshot.as_ref() else {
        return placeholder_text(Msg::EditorSnapshotUnavailable.t()).into_any_element();
    };
    if snapshot.is_binary {
        return placeholder_text(Msg::EditorWorkspaceBinary.t()).into_any_element();
    }
    if snapshot.content.is_none() {
        return placeholder_text(Msg::EditorWorkspaceDeleted.t()).into_any_element();
    }
    div()
        .id("ews-snapshot")
        .key_context("EditorSnapshot")
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .relative()
        .when(cfg!(feature = "gui-e2e"), |el| {
            #[cfg(feature = "gui-e2e")]
            {
                let view = cx.entity().downgrade();
                return el.child(
                    gpui::canvas(
                        move |bounds, _, cx| {
                            _ = view.update(cx, |view, _| view.snapshot_bounds = Some(bounds));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                );
            }
            #[cfg(not(feature = "gui-e2e"))]
            el
        })
        .child(crate::snapshot::text(view, cx))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::history_error_preview;

    #[test]
    fn history_error_preview_escapes_controls_and_bounds_untrusted_output() {
        assert_eq!(
            history_error_preview("cause\x1b\x07日本語"),
            "cause\\x1B\\x07日本語"
        );
        assert_eq!(
            history_error_preview("one\ntwo\nthree\nfour"),
            "one\ntwo\nthree…"
        );
        let long = history_error_preview(&"日".repeat(1_000));
        assert_eq!(long.chars().count(), 481);
        assert!(long.ends_with('…'));
        let controls = history_error_preview(&"\x1b".repeat(1_000));
        assert_eq!(controls.chars().count(), 481);
        assert!(!controls.contains('\x1b'));
    }
}
