use super::commit_panel::CommitPanelView;
use super::*;
use crate::ui::button_style::{ButtonRole, KagiButton};
use gpui_component::Disableable as _;

impl CommitPanelView {
    pub(super) fn render_composer_commit(&self, cx: &mut Context<Self>) -> gpui::Div {
        let can_commit = !self.state.staged.is_empty()
            && self
                .title_input
                .as_ref()
                .map(|e| !e.read(cx).value().trim().is_empty())
                .unwrap_or(!self.state.commit_msg.trim().is_empty());
        // ── Commit button ─────────────────────────────────────────
        // No destination branch in the label: ADR-0134 put it here as "one
        // place to look", but a real branch name overflows the button. The
        // header shows the branch at all times and has room for a long one.
        let commit_label = SharedString::from(i18n::commit_button());
        let commit_click = cx.listener(
            |view: &mut CommitPanelView, _event: &gpui::ClickEvent, window, cx| {
                view.defer_open_commit_plan_modal(window, cx);
            },
        );
        // One Button in both states (`.disabled`), so enabling it does not change
        // the footer's height, and the disabled form still reads as a button
        // rather than blending into the footer background.
        let commit_btn = KagiButton::styled("cp-commit-btn", commit_label, ButtonRole::Primary, cx)
            .small()
            .mt_1()
            .w_full()
            .disabled(!can_commit)
            .when(can_commit, |b| b.on_click(commit_click))
            .into_any_element();
        div()
            .relative()
            .flex_shrink_0()
            .child(commit_btn)
            .child(super::e2e::measure_inside("cp-commit-action"))
    }
    pub(super) fn render_composer_icon(
        &self,
        id: &'static str,
        path: &'static str,
        tip: String,
        enabled: bool,
        accent: u32,
    ) -> gpui::Stateful<gpui::Div> {
        let mut b = div()
            .id(id)
            .p_1()
            .rounded_sm()
            .flex()
            .items_center()
            .justify_center()
            .relative()
            .child(super::e2e::measure_inside(id))
            .tooltip(move |_w, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(_w, cx))
            .child(
                gpui::svg()
                    .path(path)
                    .w(theme::scaled_px(16.0))
                    .h(theme::scaled_px(16.0))
                    .text_color(rgb(if enabled { accent } else { theme().text_muted })),
            );
        if enabled {
            b = b.hover(|s| s.bg(rgb(theme().selected)).cursor_pointer());
        }
        b
    }
    pub(super) fn render_composer_suggest(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let smart = &self.smart_snapshot;
        let smart_ui = (self.smart_generating, &self.smart_status);
        let staged_empty = self.state.staged.is_empty();
        // Sparkles — generate a commit message. Uses the local LLM when it is
        // usable (green), otherwise the rule-based draft.
        let llm_on = smart.llm_offered();
        let suggest_enabled = !staged_empty && !smart_ui.0;
        let suggest_color = if llm_on {
            theme().color_success
        } else {
            theme().accent_text_on(theme().panel)
        };
        let suggest_btn: gpui::AnyElement = if smart_ui.0 {
            // Spin the same icon rather than swapping in a text spinner, so
            // the row does not change width mid-generation.
            use gpui::AnimationExt as _;
            let spinner_base = gpui::svg()
                .path("icons/loader-circle.svg")
                .w(theme::scaled_px(16.0))
                .h(theme::scaled_px(16.0))
                .text_color(rgb(suggest_color));
            // Reduce motion: static loader glyph, no per-frame rotation tick.
            let spinner: gpui::AnyElement = if theme::reduce_motion() {
                spinner_base.into_any_element()
            } else {
                spinner_base
                    .with_animation(
                        "cp-smart-spinner",
                        gpui::Animation::new(Duration::from_millis(900)).repeat(),
                        |svg, delta| {
                            svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                delta * std::f32::consts::TAU,
                            )))
                        },
                    )
                    .into_any_element()
            };
            div()
                .id("cp-smart-suggest")
                .p_1()
                .flex()
                .items_center()
                .justify_center()
                .child(spinner)
                .into_any_element()
        } else {
            let mut b = self.render_composer_icon(
                "cp-smart-suggest",
                "icons/sparkles.svg",
                Msg::GenerateMessage.t().to_string(),
                suggest_enabled,
                suggest_color,
            );
            if suggest_enabled {
                b = b.on_click(cx.listener(
                    move |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        if llm_on {
                            view.defer_smart_generate(window, cx);
                        } else {
                            view.defer_smart_suggest(window, cx);
                        }
                    },
                ));
            }
            b.into_any_element()
        };
        suggest_btn
    }
    pub(super) fn render_composer_icons(&self, cx: &mut Context<Self>) -> gpui::Div {
        let suggest_btn = self.render_composer_suggest(cx);
        let coauthor_menu = if self.coauthor_menu_visible() {
            self.coauthor_menu.as_deref()
        } else {
            None
        };
        // Person+ — co-author picker.
        let coauthor_btn = self
            .render_composer_icon(
                "cp-coauthor",
                "icons/user-plus.svg",
                Msg::AddCoAuthor.t().to_string(),
                true,
                theme().text_main,
            )
            .on_click(cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                    view.toggle_coauthor_menu(cx);
                },
            ));

        // Undo — amend the previous commit (the plan still blocks pushed
        // commits; this only opens the modal).
        let amend_btn = self
            .render_composer_icon(
                "cp-amend-btn",
                "icons/undo-2.svg",
                Msg::AmendLastCommit.t().to_string(),
                true,
                theme().color_warning,
            )
            .on_click(cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                    view.defer_amend(window, cx);
                },
            ));

        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(suggest_btn)
            .child(coauthor_btn)
            // #476 slice 3: amend resolves the panel's repository, so it is
            // offered on a worktree panel too — the confirm names it.
            .child(amend_btn);

        div()
            .relative()
            .child(row)
            .children(super::commit_panel_render::render_coauthor_menu(
                coauthor_menu,
                cx,
            ))
    }
    pub(super) fn render_composer_template(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // ── commit.template toggle (bottom-left of the body box) ──────
        // Only offered when the user actually has a `commit.template`; without
        // one the control would toggle nothing.
        let template_toggle: gpui::AnyElement = if self.commit_template.is_some() {
            let on = self.template_active(cx);
            div()
                .id("cp-template-toggle")
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .text_color(rgb(if on {
                    theme().accent_text_on(theme().panel)
                } else {
                    theme().text_muted
                }))
                .hover(|s| {
                    let s = s.bg(rgb(theme().selected)).cursor_pointer();
                    if on {
                        s.text_color(rgb(theme().accent_text_on(theme().selected)))
                    } else {
                        s
                    }
                })
                .tooltip(|w, cx| {
                    gpui_component::tooltip::Tooltip::new(Msg::ToggleCommitTemplate.t())
                        .build(w, cx)
                })
                .on_click(cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.toggle_template(window, cx);
                    },
                ))
                .child(SharedString::from(Msg::Template.t()))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        template_toggle
    }
    pub(super) fn render_composer_inputs(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let title_input = &self.title_input;
        let body_input = &self.body_input;
        let input_radius = gpui_component::ActiveTheme::theme(&**cx).radius;
        let template_toggle = self.render_composer_template(cx);
        let icon_row = self.render_composer_icons(cx);
        // Subject + body (ADR-0134), with template/co-author trailers in the body.
        // Real icon rows inside its box cannot overlap a long body.
        // AutoGrow's inner minimum prevents flex shrinking at high zoom. Keep
        // its cached visual-row measurement, without imposing that minimum.
        let body_rows = body_input
            .as_ref()
            .map_or(3, |body| body.read(cx).wrap_row_count().clamp(3, 12));
        let body_height = px(body_rows as f32 * theme::rem_size_px() * 1.25)
            + gpui_component::Size::Medium.input_py() * 2.;
        let msg_inputs: gpui::AnyElement = match (&title_input, &body_input) {
            (Some(title), Some(body)) => div()
                .flex()
                .flex_col()
                .min_h(px(0.))
                .flex_shrink(1.)
                .overflow_hidden()
                .gap_2()
                .child(Input::new(title).appearance(true).bordered(true))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // Icon, its p_1, row pb_1, and the body's two borders.
                        .min_h(theme::scaled_px(16.) + px(theme::rem_size_px() * 0.75 + 2.))
                        .flex_shrink(1.)
                        // Same radius the `Input` gives itself, so the two boxes
                        // do not have visibly different corners.
                        .rounded(input_radius)
                        .border_1()
                        .border_color(rgb(theme().text_muted))
                        .bg(rgb(theme().bg_base))
                        .child(
                            div()
                                .h(body_height)
                                .min_h(px(0.))
                                .flex_shrink(1.)
                                .overflow_hidden()
                                .child(Input::new(body).h_full().appearance(false).bordered(false)),
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
                                .child(template_toggle)
                                .child(icon_row),
                        ),
                )
                .into_any_element(),
            // No window yet (headless); the message still flows through
            // `state.commit_msg`, so this is a placeholder, not a failure.
            _ => div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(commit message input unavailable)"))
                .into_any_element(),
        };
        msg_inputs
    }
    pub(super) fn render_composer(&self, compact: bool, cx: &mut Context<Self>) -> gpui::Div {
        let msg_inputs = self.render_composer_inputs(cx);
        let commit_btn = self.render_composer_commit(cx);
        let has_unstaged_warning = !self.state.unstaged.is_empty() && !self.state.staged.is_empty();
        div()
            .when(compact, |el| el.w_full().h_full())
            .flex_shrink(1.)
            .min_h(px(0.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .px_3()
            .py_2()
            .gap_2()
            .bg(rgb(theme().surface))
            // Subject + body (the icon actions live inside the body box)
            .child(msg_inputs)
            // Transient smart-commit status. Its own full-width line:
            // inside the right-aligned icon group it pushed the icons
            // sideways as the text appeared (user report).
            .when_some(self.smart_status.clone(), |el, status| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(status)),
                )
            })
            // Unstaged warning
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
            .child(commit_btn)
    }
}
