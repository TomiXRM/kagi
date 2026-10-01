//! "Stage hunk" / "Unstage hunk" on a Commit Panel diff's hunk headers
//! (#842, Refs #357). The unified and the split view draw a hunk header as
//! one full-width row, so the button sits on it once, for both sides.

use std::path::PathBuf;

use gpui::{div, prelude::*, rgb, SharedString, WeakEntity};
use kagi_domain::diff::HunkRange;

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
}

impl HunkAction {
    /// The action for `source`, when it is a Commit Panel side.
    pub(crate) fn for_source(
        source: &MainDiffSource,
        app: WeakEntity<KagiApp>,
        owner: crate::app::SessionId,
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
        })
    }

    /// The button for the hunk header `header` at row `row`; `None` when the
    /// header does not parse as a hunk.
    pub(crate) fn button(&self, row: usize, header: &str) -> Option<gpui::AnyElement> {
        let range = HunkRange::parse(header)?;
        let label = if self.staged {
            Msg::DiffUnstageHunk.t()
        } else {
            Msg::DiffStageHunk.t()
        };
        let action = self.clone();
        let button = div()
            .id(("main-diff-hunk-stage", row))
            .flex_shrink_0()
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(theme::theme().selected))
            .text_xs()
            .text_color(rgb(theme::theme().text_sub))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme::theme().selected)))
            // The header row starts a text selection on mouse down.
            .on_mouse_down(gpui::MouseButton::Left, |_e, _w, cx| cx.stop_propagation())
            .on_click(move |_e, _w, cx| {
                cx.stop_propagation();
                let HunkAction {
                    app,
                    owner,
                    path,
                    staged,
                } = action.clone();
                app.update(cx, |app, cx| {
                    app.stage_hunk_from_diff(owner, path, range, staged, cx)
                })
                .ok();
            })
            .child(SharedString::from(label));
        Some(
            crate::ui::e2e::measure_control(format!("main-diff-hunk-stage-{row}"), button)
                .into_any_element(),
        )
    }
}
