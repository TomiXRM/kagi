//! The header operation strip (#704).
//!
//! One line under the toolbar, present for exactly as long as the repository
//! is in the middle of something: the operation's name, how far it has got,
//! and the way out. It is rendered from `TabViewState::operation`, so it shows
//! from the tab's first accepted read — including on a repository opened at
//! launch that was left `MERGING` by someone else — and it does not go away
//! when the conflict editor does. ADR-0135 removed the old permanent banner;
//! this is not that banner back, it is the operation's own row and it exists
//! only while an operation does.
//!
//! The one place it is not drawn is while the conflict editor is mounted
//! (owner ruling on top of #707): there the dashboard's own Abort sits with
//! Continue / Next conflict and dispatches `open_conflict_abort_modal` — this
//! module's action, unchanged. Admission never moves: it is
//! `TabViewState::operation` in both placements, so an abort is reachable
//! whether or not a `ConflictView` exists, which is what #704 was about.

use gpui::{div, prelude::*, rgb, Context, SharedString};

use super::i18n::Msg;
use super::theme::{self, theme};
use super::KagiApp;

impl KagiApp {
    /// `None` when nothing is in progress — the strip has no idle state.
    pub(super) fn render_operation_strip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let operation = self.view().operation.clone()?;
        // Domain words (merge / rebase / cherry-pick / revert) stay English
        // per ADR-0048; only the prose around them is localized.
        let label = match operation.step {
            Some((step, total)) => format!(
                "{} {step}/{total} · {}",
                operation.kind().slug(),
                Msg::OperationInProgress.t()
            ),
            None => format!(
                "{} · {}",
                operation.kind().slug(),
                Msg::OperationInProgress.t()
            ),
        };
        let abort = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            if let Some(owner) = this.detect_owner() {
                this.open_conflict_abort_modal(owner, cx);
            }
            cx.notify();
        });
        Some(
            super::e2e::measure_control(
                "operation-strip",
                div()
                    .id("operation-strip")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .py(theme::scaled_px(4.0))
                    .bg(rgb(theme().surface))
                    .border_b_1()
                    .border_color(rgb(theme().color_warning))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(label)),
                    )
                    .child(div().flex_1())
                    .child(
                        super::e2e::measure_control(
                            "operation-strip-abort",
                            div()
                                .id("operation-strip-abort")
                                .px_2()
                                .py(theme::scaled_px(2.0))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(theme().color_blocker))
                                .text_xs()
                                .text_color(rgb(theme().color_blocker))
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|style| style.bg(rgb(theme().selected)))
                                .child(SharedString::from(Msg::ConflictAbort.t()))
                                .on_click(abort),
                        )
                        .into_any_element(),
                    ),
            )
            .into_any_element(),
        )
    }
}
