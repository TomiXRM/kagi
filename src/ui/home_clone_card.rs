//! The clone card opened from Home (#923, ADR-0219): what will be cloned
//! where, the plan, and Clone — split out of `home_github.rs`, which owns the
//! list and the clone's state and execution.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, FocusHandle, KeyDownEvent, SharedString,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use kagi_ui_core::i18n::{plan_note_text, plan_title_text};

use super::home_github::CloneModal;
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_current_predicted, render_modal_title_row};
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_LG};
use super::render_helpers::safe_text;
use super::theme::theme;
use super::KagiApp;

/// The clone card: the plan's current → predicted box, its warnings and
/// blockers, the destination with "Choose Folder…", and Clone (absent when
/// blocked).
pub(crate) fn render_clone_modal(
    modal: CloneModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let plan = &modal.plan;
    let blocked = !plan.blockers.is_empty();
    let cancel = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
        app.cancel_clone();
        if let Some(root) = app.root_focus.clone() {
            window.focus(&root, cx);
        }
        cx.notify();
    });
    let confirm = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
        app.start_clone(cx);
        if let Some(root) = app.root_focus.clone() {
            window.focus(&root, cx);
        }
        cx.notify();
    });
    let change = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
        app.change_clone_parent(cx);
    });

    let mut body = modal_scroll_body()
        .child(
            div()
                .flex_shrink_0()
                .child(render_current_predicted(plan, None)),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from(Msg::CloneDestination.t())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .font_family(super::MONO_FONT)
                        .text_color(rgb(theme().text_main))
                        .child(safe_text(&modal.request.dest.display().to_string())),
                )
                .child(super::e2e::measure_control(
                    "clone-change-folder",
                    Button::new("clone-change-folder")
                        .ghost()
                        .small()
                        .label(Msg::CloneChooseFolder.t())
                        .on_click(change),
                )),
        );
    for (i, note) in plan.warnings.iter().enumerate() {
        body = body.child(super::plan_card_rows::render_note_row(
            SharedString::from(format!("clone-warning-{i}")),
            false,
            "!",
            theme().color_warning,
            &plan_note_text(note),
            false,
        ));
    }
    for (i, note) in plan.blockers.iter().enumerate() {
        body = body.child(super::plan_card_rows::render_note_row(
            SharedString::from(format!("clone-blocker-{i}")),
            true,
            "\u{2717}",
            theme().color_blocker,
            &plan_note_text(note),
            false,
        ));
    }
    if let Some(command) = plan.equivalent_command.as_deref() {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .font_family(super::MONO_FONT)
                .text_color(rgb(theme().text_muted))
                .child(safe_text(command)),
        );
    }

    let mut buttons =
        div()
            .flex()
            .flex_row()
            .gap_2()
            .justify_end()
            .child(super::e2e::measure_control(
                "clone-cancel",
                Button::new("clone-cancel")
                    .label(Msg::PlanCancel.t())
                    .ghost()
                    .small()
                    .on_click(cancel),
            ));
    if !blocked {
        buttons = buttons.child(super::e2e::measure_control(
            "clone-confirm",
            Button::new("clone-confirm")
                .primary()
                .small()
                .label(Msg::CloneConfirm.t())
                .on_click(confirm),
        ));
    }
    let card = modal_card(MODAL_W_LG)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(plan_title_text(&plan.title)),
            None,
        )))
        .child(body)
        .child(div().flex_shrink_0().child(buttons));
    let esc = cx.listener(|app, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            app.cancel_clone();
            if let Some(root) = app.root_focus.clone() {
                window.focus(&root, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let base = div().on_key_down(esc);
    let card = match focus_handle {
        Some(focus) => base.track_focus(&focus).child(card),
        None => base.child(card),
    };
    modal_overlay(super::e2e::measure_control("active-modal/clone", card)).into_any_element()
}
