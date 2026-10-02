//! The clone card opened from Home (#923, ADR-0219): which repository, where
//! it goes, and Clone. Kagi does not choose the folder: the card asks for it
//! (the system folder dialog), and Clone stays off until a folder is chosen
//! and its plan has no blocker. Split out of `home_github.rs`, which owns the
//! list and the clone's state and execution.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, FocusHandle, KeyDownEvent, SharedString,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Disableable as _;
use kagi_ui_core::i18n::plan_note_text;

use super::home_github::CloneModal;
use super::i18n::Msg;
use super::modal_renderers::modal_overlay;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

const CARD_W: f32 = 480.;

/// The clone card: the repository at the top, the location it goes to (empty
/// until chosen), what will be created there and any reason it cannot, then
/// Cancel and Clone.
pub(crate) fn render_clone_modal(
    modal: CloneModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let ready = modal
        .target
        .as_ref()
        .is_some_and(|target| target.plan.blockers.is_empty());
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
    let choose = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
        app.change_clone_parent(cx);
    });

    let mut body = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(header(&modal))
        .child(location(&modal, choose))
        .children(modal.started.map(|started| progress(&modal, started)));
    if let Some(target) = &modal.target {
        // The shared tinted note rows: warnings read as notes, blockers as
        // alerts to assistive technology (#354).
        let notes = target
            .plan
            .warnings
            .iter()
            .map(|n| (false, "!", theme().color_warning, n))
            .chain(
                target
                    .plan
                    .blockers
                    .iter()
                    .map(|n| (true, "\u{2717}", theme().color_blocker, n)),
            );
        for (i, (blocker, glyph, color, note)) in notes.enumerate() {
            body = body.child(super::plan_card_rows::render_note_row(
                SharedString::from(format!("clone-note-{i}")),
                blocker,
                glyph,
                color,
                &plan_note_text(note),
                true,
            ));
        }
    }
    let buttons = div().flex().flex_row().justify_end().gap_2();
    let buttons = if modal.started.is_some() {
        // No cancel in v1 (ADR-0219 decision 6): the card can only step
        // aside; the clone keeps going and its result arrives as usual.
        buttons.child(super::e2e::measure_control(
            "clone-hide",
            Button::new("clone-hide")
                .label(Msg::CloneHide.t())
                .ghost()
                .on_click(cancel),
        ))
    } else {
        buttons
            .child(super::e2e::measure_control(
                "clone-cancel",
                Button::new("clone-cancel")
                    .label(Msg::PlanCancel.t())
                    .ghost()
                    .on_click(cancel),
            ))
            .child(super::e2e::measure_control(
                "clone-confirm",
                Button::new("clone-confirm")
                    .primary()
                    .label(Msg::CloneConfirm.t())
                    .disabled(!ready)
                    .on_click(confirm),
            ))
    };
    let card = div()
        .w(theme::scaled_px(CARD_W))
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .rounded_xl()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .shadow_lg()
        .child(body)
        .child(buttons);

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

/// The repository's name, and under it `owner/repo · host` with its
/// Private / Fork marks.
fn header(modal: &CloneModal) -> impl IntoElement {
    let listing = &modal.listing;
    let mut sub = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(format!(
            "{} · {}",
            listing.name_with_owner, listing.host
        )));
    for (on, label) in [
        (listing.is_private, Msg::HomeGithubPrivate.t()),
        (listing.is_fork, Msg::HomeGithubFork.t()),
    ] {
        if on {
            sub = sub.child(
                div()
                    .px_2()
                    .rounded_md()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(label)),
            );
        }
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xl()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(
                    Msg::CloneTitle.t().replace("{}", listing.name()),
                )),
        )
        .child(sub)
}

/// The folder the clone goes into: a field showing the chosen folder (or
/// asking for one) with Choose…, and the folder that will be created.
fn location(
    modal: &CloneModal,
    choose: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let chosen = modal.target.as_ref().and_then(|target| {
        target
            .request
            .dest
            .parent()
            .map(|p| p.display().to_string())
    });
    let field = div()
        .flex_1()
        .min_w(px(0.))
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgb(theme().bg_base))
        .border_1()
        .border_color(rgb(theme().surface))
        .text_sm()
        .truncate()
        .child(match &chosen {
            Some(path) => div()
                .text_color(rgb(theme().text_main))
                .child(safe_text(path)),
            None => div()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::CloneNoLocation.t())),
        });
    let created = modal.target.as_ref().map(|target| {
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .truncate()
            .child(safe_text(
                &Msg::CloneWillCreate
                    .t()
                    .replace("{}", &target.request.dest.display().to_string()),
            ))
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::CloneLocation.t())),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field)
                .child(super::e2e::measure_control(
                    "clone-change-folder",
                    Button::new("clone-change-folder")
                        .outline()
                        .label(Msg::CloneChooseFolder.t())
                        .disabled(modal.started.is_some())
                        .on_click(choose),
                )),
        )
        .children(created)
}

/// The running clone: a spinner with the elapsed time, and where it is
/// downloading from. Redrawn every second by `tick_clone_card`.
fn progress(modal: &CloneModal, started: std::time::Instant) -> impl IntoElement {
    let secs = started.elapsed().as_secs();
    let elapsed = format!("{}:{:02}", secs / 60, secs % 60);
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(super::render_overlay::sync_spinner(
                    16.,
                    theme().color_branch,
                    "clone-progress-spinner",
                ))
                .child(div().text_sm().text_color(rgb(theme().text_main)).child(
                    SharedString::from(Msg::CloneRunning.t().replace("{}", &elapsed)),
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(
                    Msg::CloneRunningHint.t().replace("{}", &modal.listing.host),
                )),
        )
}
