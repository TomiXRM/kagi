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
//! The strip is omitted only while `ConflictView` and its sidebar are shown.
//! Revised owner/HIG placement (#1764) groups Skip and Abort with the operation
//! name/step in the leading sidebar header. Abort is framed Secondary with a red
//! label. Only Continue follows the file actions/Save, separated by a line or gap.
//! A hidden sidebar uses this #704 strip under the main toolbar, not an inline
//! file-header session group; an unmounted `ConflictView` retains it too,
//! including at startup. Hidden sequencer controls keep Skip's armed state
//! visible here. The main toolbar has no conflict-session controls.
//! Localized Repository commands use the observed operation name and dispatch
//! the same paths; Abort remains available without a mounted conflict editor.
//! Abort still dispatches `open_conflict_abort_modal`; admission from
//! `TabViewState::operation`, confirmations, revision/lease guards and execution
//! safety remain unchanged.

use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::{Disableable as _, Sizable as _};

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
        let abort_disabled = !super::commands::is_enabled(self, "repo.conflictAbort", cx);
        let skip = self.ui().conflict.as_ref().and_then(|entity| {
            let view = entity.read(cx);
            view.mode.as_ref().and_then(|mode| {
                mode.session
                    .op
                    .is_sequencer()
                    .then_some(mode.skip_approval.is_some())
            })
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
                            super::button_style::conflict_abort_button(
                                "operation-strip-abort",
                                abort_disabled,
                                cx,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.handle_menu_command("repo.conflictAbort", window, cx)
                                },
                            )),
                        )
                        .into_any_element(),
                    )
                    .when_some(skip, |el, armed| {
                        el.child(super::e2e::measure_control(
                            "operation-strip-skip",
                            super::button_style::KagiButton::styled(
                                "operation-strip-skip",
                                if armed {
                                    Msg::ConflictConfirmSkip.t()
                                } else {
                                    Msg::ConflictSkip.t()
                                },
                                super::button_style::ButtonRole::Secondary,
                                cx,
                            )
                            .small()
                            .compact()
                            .h(theme::scaled_px(20.))
                            .rounded(theme::scaled_px(5.))
                            .text_size(theme::scaled_px(12.))
                            .disabled(!super::commands::is_enabled(self, "repo.conflictSkip", cx))
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.handle_menu_command("repo.conflictSkip", window, cx)
                                },
                            )),
                        ))
                    }),
            )
            .into_any_element(),
        )
    }
}
