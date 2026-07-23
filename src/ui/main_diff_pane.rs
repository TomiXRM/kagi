//! ADR-0121 B2: the full-width main diff (T-UI-003) as a fat entity
//! (ADR-0117 template), registered as the `CenterPane::Diff` [`super::workspace::WorkspaceItem`].
//!
//! What moved in here from `KagiApp`:
//! - the [`MainDiffView`] itself (was `KagiApp.main_diff: Option<MainDiffView>`;
//!   the field is now `Option<Entity<MainDiffPane>>`),
//! - the diff-list scroll state (was `KagiApp.main_diff_scroll_handle` — the
//!   `ListState` now lives and dies with the pane),
//! - the off-thread highlight swap-in (was `KagiApp.pending_diff_highlight` +
//!   `apply_pending_highlights`): the spawn's `this.update` now targets the
//!   pane entity, so a result that arrives after the diff was closed is
//!   dropped by the dead-weak-handle guard instead of a render-time check.
//!
//! The File History / Editor Workspace *embedded* diff panes are untouched:
//! they keep their own `MainDiffView` + `ListState` fields and render via
//! `render_helpers::render_diff_list` directly, exactly as before.

use gpui::{prelude::*, Context, Entity, ListState, WeakEntity, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::Sizable as _;

use super::diff_view::{MainDiffSource, MainDiffView, RowHighlights};
use super::render_helpers::{new_diff_list_state, render_diff_list};
use super::KagiApp;

/// Fat entity for the standalone (center-slot) main diff.
pub struct MainDiffPane {
    /// The diff currently shown. Replaced in place on j/k file steps and on
    /// re-opens while the pane is up, so the `ListState` keeps the same
    /// lifecycle the old persistent `KagiApp.main_diff_scroll_handle` had
    /// (reset-to-top only when the row count changes — see
    /// `render_helpers::render_diff_list`).
    pub view: MainDiffView,
    /// T-UI-003 / T-DIFF-WRAP-001: `ListState` (variable-height) for the
    /// "main-diff-list" — see `render_helpers::render_diff_list` for the
    /// item-count sync/reset lifecycle.
    scroll: ListState,
    /// ADR-0117: parent handle for the header buttons (Back / History). Only
    /// upgraded from event listeners — never from the render path (re-entrancy).
    app: WeakEntity<KagiApp>,
}

impl MainDiffPane {
    pub fn new(view: MainDiffView, app: WeakEntity<KagiApp>) -> Self {
        Self {
            view,
            scroll: new_diff_list_state(),
            app,
        }
    }

    /// ADR-0109: apply an off-thread highlight result if the pane still shows
    /// the same commit file it was requested for (stale results — the user
    /// stepped to another file first — are discarded, as before).
    pub(crate) fn apply_highlights(&mut self, row: usize, file: usize, highlights: RowHighlights) {
        match self.view.source {
            MainDiffSource::Commit {
                row_index,
                file_index,
            } if row_index == row && file_index == file => {}
            _ => return,
        }
        for (row_i, row_highlights) in highlights {
            if let Some(super::diff_view::DiffRow::Line { highlights: hl, .. }) =
                self.view.rows.get_mut(row_i)
            {
                *hl = row_highlights;
            }
        }
    }
}

impl Render for MainDiffPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Standalone header buttons (moved from `render_main_diff_view`'s
        // `standalone: true` arm). "← Back" closes the diff; "History" opens
        // File History for the shown file (导线 #3).
        let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.app
                .update(cx, |app, cx| {
                    app.close_main_diff();
                    cx.notify();
                })
                .ok();
        });
        let history_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.app
                .update(cx, |app, cx| {
                    app.open_file_history_from_main_diff(cx);
                    cx.notify();
                })
                .ok();
        });
        let leading = Button::new("main-diff-back")
            .label("\u{2190} Back")
            .ghost()
            .small()
            .on_click(back_click)
            .into_any_element();
        let trailing = Button::new("main-diff-history")
            .label("History")
            .ghost()
            .small()
            .flex_shrink_0()
            .on_click(history_click)
            .into_any_element();

        render_diff_list::<MainDiffPane>(
            self.view.clone(),
            Some(leading),
            Some(trailing),
            self.scroll.clone(),
            cx,
        )
    }
}

impl KagiApp {
    /// Show `view` in the main diff pane: update the live pane in place (j/k
    /// steps, re-opens) or create the entity on first open. Returns the pane
    /// so callers can chain a highlight spawn onto it.
    pub(crate) fn show_main_diff(
        &mut self,
        view: MainDiffView,
        cx: &mut Context<Self>,
    ) -> Entity<MainDiffPane> {
        match self.main_diff.clone() {
            Some(pane) => {
                pane.update(cx, |p, cx| {
                    p.view = view;
                    cx.notify();
                });
                pane
            }
            None => {
                let weak = cx.weak_entity();
                let pane = cx.new(|_| MainDiffPane::new(view, weak));
                self.main_diff = Some(pane.clone());
                pane
            }
        }
    }
}
