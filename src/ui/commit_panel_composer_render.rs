//! Shared wide/pinned composer rendering over the original CommitPanelView inputs.

use super::commit_panel::CommitPanelView;
use super::commit_panel_controls as controls;
use super::*;
use gpui::Focusable as _;
use gpui::Pixels;
use gpui_component::ActiveTheme as _;

fn body_min_height() -> Pixels {
    theme::scaled_px(16.) + px(theme::rem_size_px() * 0.75 + 2.)
}

fn message_min_height() -> Pixels {
    body_min_height() + px(theme::rem_size_px() * 2.5)
}

impl CommitPanelView {
    fn render_composer_inputs(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let template_toggle = controls::template_toggle(self, cx);
        let icon_row = div()
            .relative()
            .child(controls::icon_row(self, cx))
            .children(super::commit_panel_render::render_coauthor_menu(
                self, window, cx,
            ));
        let input_radius = cx.theme().radius;
        // Retain AutoGrow's cached visual-row measurement without its inner
        // minimum preventing the surrounding composer from shrinking.
        let body_rows = self
            .body_input
            .as_ref()
            .map_or(3, |body| body.read(cx).wrap_row_count().clamp(3, 12));
        let body_height = px(body_rows as f32 * theme::rem_size_px() * 1.25)
            + gpui_component::Size::Medium.input_py() * 2.;
        match (&self.title_input, &self.body_input) {
            (Some(title), Some(body)) => div()
                .flex()
                .flex_col()
                // Subject, gap and action-row floor yield height together.
                .min_h(message_min_height())
                .flex_shrink(1.)
                .overflow_hidden()
                .gap_2()
                .child(
                    Input::new(title)
                        .context_menu(kagi_ui_core::input::context_menu(title, false))
                        .aria_label(Msg::A11yCommitSummary.t())
                        .appearance(true)
                        .bordered(true),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // Tab traverses the named controls; Cmd+] / Cmd+[
                        // retain the editor's explicit indentation actions.
                        .capture_action(cx.listener(
                            |_, _: &gpui_component::input::IndentInline, window, cx| {
                                window.focus_next(cx);
                                cx.stop_propagation();
                            },
                        ))
                        .capture_action(cx.listener(
                            |_, _: &gpui_component::input::OutdentInline, window, cx| {
                                window.focus_prev(cx);
                                cx.stop_propagation();
                            },
                        ))
                        .min_h(body_min_height())
                        .flex_shrink(1.)
                        .rounded(input_radius)
                        .border_1()
                        .border_color(cx.theme().input)
                        .when(body.read(cx).focus_handle(cx).is_focused(window), |el| {
                            el.border_color(cx.theme().ring)
                        })
                        .bg(rgb(theme().bg_base))
                        .child(
                            div()
                                .h(body_height)
                                .min_h(px(0.))
                                .flex_shrink(1.)
                                .overflow_hidden()
                                .relative()
                                .child(super::e2e::measure_inside("cp-body-input"))
                                .child(
                                    Input::new(body)
                                        .context_menu(kagi_ui_core::input::context_menu(
                                            body, false,
                                        ))
                                        .aria_label(Msg::A11yCommitDescription.t())
                                        .h_full()
                                        .appearance(false)
                                        .bordered(false),
                                ),
                        )
                        .relative()
                        .child(super::e2e::measure_inside("cp-body"))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .flex_shrink_0()
                                .px_1()
                                .pb_1()
                                .relative()
                                .child(super::e2e::measure_inside("cp-footer-tools"))
                                .child(template_toggle)
                                .child(icon_row),
                        ),
                )
                .into_any_element(),
            // Headless callers still commit through state.commit_msg.
            _ => div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(commit message input unavailable)"))
                .into_any_element(),
        }
    }

    pub(super) fn render_composer(
        &self,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let msg_inputs = self.render_composer_inputs(window, cx);
        // A body without a subject is not a commit.
        let can_commit = !self.state.staged.is_empty()
            && self
                .title_input
                .as_ref()
                .map(|title| !title.read(cx).value().trim().is_empty())
                .unwrap_or(!self.state.commit_msg.trim().is_empty());
        let commit_btn = controls::commit_button(self, can_commit, cx);
        let has_unstaged_warning = !self.state.unstaged.is_empty() && !self.state.staged.is_empty();
        let has_secret_warning = !self.state.secret_paths.is_empty();
        // Padding, gaps, Commit and optional status lines retain dev's floor.
        let footer_min_height = message_min_height()
            + px(theme::rem_size_px() * 3.25 + 4.)
            + px(theme::rem_size_px() * 1.75)
                * (usize::from(self.smart_status.is_some())
                    + usize::from(has_unstaged_warning)
                    + 3 * usize::from(has_secret_warning)) as f32;
        div()
            .when(compact, |el| el.w_full())
            .flex_shrink(1.)
            .min_h(footer_min_height)
            .overflow_hidden()
            .flex()
            .flex_col()
            .px_3()
            .py_2()
            .gap_2()
            .bg(rgb(theme().surface))
            .child(msg_inputs)
            .relative()
            .child(super::e2e::measure_inside("cp-footer"))
            .children(controls::draft_save_feedback(self, cx))
            // A separate status line cannot push the icon controls sideways.
            .when_some(self.smart_status.clone(), |el, status| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(status)),
                )
            })
            .when(has_unstaged_warning, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().color_warning))
                        .child(SharedString::from(i18n::unstaged_not_included(
                            self.state.unstaged.len(),
                        ))),
                )
            })
            .when(has_secret_warning, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().color_warning))
                        .child(super::e2e::measure_control(
                            "cp-secret-review",
                            Msg::CommitSecretReview.t(),
                        )),
                )
            })
            .child(commit_btn)
    }
}
