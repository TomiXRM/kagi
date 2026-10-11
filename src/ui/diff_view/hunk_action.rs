//! "Stage hunk" / "Unstage hunk" on a Commit Panel diff's hunk headers
//! (#842, Refs #357). The unified and the split view draw a hunk header as
//! one full-width row, so the button sits on it once, for both sides.

use std::path::PathBuf;

use gpui::{prelude::*, SharedString, WeakEntity};
use gpui_component::{
    button::{Button, ButtonVariants},
    Sizable,
};
use kagi_domain::diff::{HunkApproval, HunkRange};

use super::MainDiffSource;
use crate::ui::{theme, KagiApp, Msg};

/// The diff on screen is one side of a Commit Panel file: its hunk headers
/// move a hunk to the other side.
#[derive(Clone)]
pub(crate) struct HunkAction {
    app: WeakEntity<KagiApp>,
    owner: crate::app::SessionId,
    path: PathBuf,
    /// `true`: the staged diff (Unstage hunk); `false`: unstaged (Stage hunk).
    staged: bool,
    approvals: std::sync::Arc<Vec<HunkApproval>>,
}

impl HunkAction {
    /// The action for `source`, when it is a Commit Panel side.
    pub(crate) fn for_source(
        source: &MainDiffSource,
        app: WeakEntity<KagiApp>,
        owner: crate::app::SessionId,
        approvals: std::sync::Arc<Vec<HunkApproval>>,
    ) -> Option<Self> {
        let (path, staged) = match source {
            MainDiffSource::Unstaged { path } => (path.clone(), false),
            MainDiffSource::Staged { path } => (path.clone(), true),
            _ => return None,
        };
        Some(Self {
            app,
            owner,
            path,
            staged,
            approvals,
        })
    }

    /// The button for the hunk header `header` at row `row`; `None` when the
    /// header does not parse as a hunk.
    pub(crate) fn button(&self, row: usize, header: &str) -> Option<gpui::AnyElement> {
        let range = HunkRange::parse(header)?;
        let approved = *self.approvals.iter().find(|h| h.range == range)?;
        let label = if self.staged {
            Msg::DiffUnstageHunk.t()
        } else {
            Msg::DiffStageHunk.t()
        };
        let action = self.clone();
        #[cfg(feature = "gui-e2e")]
        crate::ui::e2e::diff_controls::button(row, gpui::Role::Button, label);
        let button = Button::new(("main-diff-hunk-stage", row))
            .ghost()
            .small()
            // Dense header: retain natural row height, not toolbar Button height.
            .h_auto()
            .px_1()
            .py_0()
            .border_1()
            .border_color(gpui::rgb(theme::theme().selected))
            .label(SharedString::from(label))
            .tooltip(SharedString::from(label))
            .on_key_down(crate::ui::keyboard_nav::stop_activation_keys)
            .map(|button| {
                #[cfg(feature = "gui-e2e")]
                let button = button.on_key_down(move |event, _, _| {
                    if event.keystroke.key == "f19" {
                        crate::ui::e2e::diff_controls::note_focus(row);
                    }
                });
                button
            })
            // The header row starts a text selection on mouse down.
            .on_mouse_down(gpui::MouseButton::Left, |_e, _w, cx| cx.stop_propagation())
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                let HunkAction {
                    app,
                    owner,
                    path,
                    staged,
                    ..
                } = action.clone();
                app.update(cx, |app, cx| {
                    // This write can remove the focused header (or the entire pane).
                    // Do not leave Tab routing on the retired Button handle.
                    if event.is_keyboard() {
                        if let Some(root) = &app.root_focus {
                            window.focus(root, cx);
                        }
                    }
                    app.stage_hunk_from_diff(owner, path, approved, staged, cx)
                })
                .ok();
            });
        #[cfg(feature = "gui-e2e")]
        let button = crate::ui::e2e::measure_control(format!("main-diff-hunk-stage-{row}"), button);
        Some(button.into_any_element())
    }
}
