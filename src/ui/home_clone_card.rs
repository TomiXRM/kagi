//! The clone card opened from Home (#923, ADR-0219): which repository, where
//! it goes, and Clone. Kagi does not choose the folder: the card asks for it
//! (the system folder dialog), and Clone stays off until a folder is chosen
//! and its plan has no blocker. Split out of `home_github.rs`, which owns the
//! list and the clone's state and execution.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, FocusHandle, KeyDownEvent, SharedString,
};
use kagi_ui_core::i18n::plan_note_text;
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::button_style::{modal_button, ModalButtonKind};
use super::home_github::CloneModal;
use super::i18n::Msg;
use super::modal_renderers::modal_overlay;
use super::render_helpers::safe_text;
use super::theme::theme;
use super::KagiApp;

const CARD_W: f32 = 480.;

#[cfg(feature = "gui-e2e")]
#[path = "e2e/clone_progress.rs"]
pub mod clone_progress;

/// The clone card: the repository at the top, the location it goes to (empty
/// until chosen), what will be created there and any reason it cannot, then
/// Cancel and Clone.
pub(crate) fn render_clone_modal(
    modal: CloneModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let disabled_reason = match modal.target.as_ref() {
        None => Some(SharedString::from(Msg::CloneNoLocation.t())),
        Some(target) => target
            .plan
            .blockers
            .first()
            .map(|blocker| SharedString::from(plan_note_text(blocker))),
    };
    let available = disabled_reason.is_none() && modal.started.is_none();
    let title = Msg::CloneTitle.t().replace("{}", modal.listing.name());
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

    let body = clone_body(&modal, choose, cx);
    let buttons = clone_footer(
        modal.started.is_some(),
        disabled_reason,
        cancel,
        confirm,
        cx,
    );
    let card = super::modal_shell::modal_card(CARD_W)
        .relative()
        .child(super::e2e::measure_inside("clone-card-bounds"))
        .child(body)
        .child(buttons.flex_shrink_0());

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
    let card = super::modal_renderers::input_dialog_frame(
        "clone-card",
        &title,
        available.then_some(Msg::CloneConfirm.t()),
        card,
        focus_handle.as_ref(),
        |frame| frame.on_key_down(esc),
        cx,
    );
    modal_overlay(super::e2e::measure_control("active-modal/clone", card)).into_any_element()
}

fn clone_body(
    modal: &CloneModal,
    choose: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let mut body = super::modal_shell::modal_scroll_body()
        // The pinned Button's outside focus band is 1.5px beyond its 1px
        // border, independent of zoom. Keep it inside the scroll clipping box.
        .px(px(3.))
        .pb(px(3.))
        .relative()
        .child(super::e2e::measure_inside("clone-body"))
        .child(header(modal))
        .child(location(modal, choose, cx))
        .children(modal.started.map(progress));
    if let Some(target) = &modal.target {
        // Warnings remain notes and blockers alerts to assistive technology.
        let notes = target
            .plan
            .warnings
            .iter()
            .map(|n| {
                let (glyph, color) = super::plan_card_rows::note_presentation(n.kind(), &theme());
                (false, glyph, color, n)
            })
            .chain(
                target
                    .plan
                    .blockers
                    .iter()
                    .map(|n| (true, "\u{2717}", theme().color_blocker, n)),
            );
        for (i, (blocker, glyph, color, note)) in notes.enumerate() {
            let id = SharedString::from(format!("clone-note-{i}"));
            body = body.child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .child(super::e2e::measure_inside(id.to_string()))
                    .child(super::plan_card_rows::render_note_row(
                        id,
                        blocker,
                        glyph,
                        color,
                        &plan_note_text(note),
                        true,
                    )),
            );
        }
    }
    body
}

fn clone_footer(
    running: bool,
    disabled_reason: Option<SharedString>,
    cancel: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    confirm: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::Div {
    let buttons = super::button_style::modal_button_row();
    if running {
        // ADR-0219 decision 6: Hide steps aside without stopping the clone.
        buttons.child(super::e2e::measure_control(
            "clone-hide",
            modal_button(
                "clone-hide",
                Msg::CloneHide.t(),
                ModalButtonKind::Secondary,
                None,
                cancel,
                cx,
            ),
        ))
    } else {
        buttons
            .child(super::e2e::measure_control(
                "clone-cancel",
                modal_button(
                    "clone-cancel",
                    Msg::PlanCancel.t(),
                    ModalButtonKind::Cancel,
                    None,
                    cancel,
                    cx,
                ),
            ))
            .child(super::e2e::measure_control(
                "clone-confirm",
                modal_button(
                    "clone-confirm",
                    Msg::CloneConfirm.t(),
                    ModalButtonKind::Primary,
                    disabled_reason,
                    confirm,
                    cx,
                ),
            ))
    }
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
        .child(destination_path(
            "clone-source",
            safe_text(&format!("{} · {}", listing.name_with_owner, listing.host)),
            |el| el.flex_1().min_w(px(0.)),
        ));
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
                    .when(theme().uses_neutral_dark_chrome(), |chip| {
                        chip.border_1()
                            .border_color(rgb(theme().elevated_border(theme().surface)))
                    })
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
        .child(destination_path(
            "clone-title",
            safe_text(&Msg::CloneTitle.t().replace("{}", listing.name())),
            |el| el.text_xl().text_color(rgb(theme().text_main)),
        ))
        .child(sub)
}

/// The resolved destination and its unchanged system parent-picker action.
fn location(
    modal: &CloneModal,
    choose: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> impl IntoElement {
    let chosen = modal
        .target
        .as_ref()
        .map(|target| target.request.dest.display().to_string());
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
        .child(match &chosen {
            Some(path) => destination_path("clone-destination", safe_text(path), |el| {
                el.text_color(rgb(theme().text_main))
            })
            .into_any_element(),
            None => div()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::CloneNoLocation.t()))
                .into_any_element(),
        });
    let created = chosen.as_ref().map(|path| destination_summary(path, cx));
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
                    modal_button(
                        "clone-change-folder",
                        if chosen.is_some() {
                            Msg::CloneChangeFolder.t()
                        } else {
                            Msg::CloneChooseFolder.t()
                        },
                        ModalButtonKind::Secondary,
                        modal
                            .started
                            .map(|_| SharedString::from(Msg::CloneChangeFolderWhileRunning.t())),
                        choose,
                        cx,
                    ),
                )),
        )
        .children(created)
}

fn destination_summary(path: &str, cx: &gpui::App) -> impl IntoElement {
    let summary = safe_text(&Msg::CloneWillCreate.t().replace("{}", path));
    let copy = path.to_owned();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(destination_path(
            "clone-destination-summary",
            summary,
            |el| {
                el.flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
            },
        ))
        .child(super::e2e::measure_control(
            "clone-copy-location",
            modal_button(
                "clone-copy-location",
                Msg::CloneCopyLocation.t(),
                ModalButtonKind::Secondary,
                None,
                move |_, _, cx| {
                    #[cfg(feature = "gui-e2e")]
                    CLONE_COPY_ACTIVATIONS.with(|count| count.set(count.get() + 1));
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                    cx.stop_propagation();
                },
                cx,
            ),
        ))
}

/// Full text reaches tooltip and AX; only the native text paint is shortened.
fn destination_path(
    id: &'static str,
    full: SharedString,
    configure: impl FnOnce(gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div>,
) -> impl IntoElement {
    let tooltip = full.clone();
    #[cfg(feature = "gui-e2e")]
    if id == "clone-destination" {
        record_clone_destination(Some(&full), Some(&full), Some(&tooltip));
    }
    #[cfg(feature = "gui-e2e")]
    if id == "clone-destination-summary" {
        CLONE_DESTINATION.with(|slot| {
            if let Some(presentation) = slot.borrow_mut().as_mut() {
                presentation.summary = Some(full.to_string());
            }
        });
    }
    configure(
        div()
            .id(id)
            .role(gpui::Role::Label)
            .aria_label(full.clone())
            .aria_description(full.clone())
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle()
            .relative()
            .child(super::e2e::measure_inside(id))
            .child(full),
    )
    .hover_tooltip_builder((id, 0usize), move |window, cx| {
        gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
    })
}

/// The running clone: a spinner with elapsed time. Redrawn every second by
/// `tick_clone_card`.
fn progress(started: std::time::Instant) -> impl IntoElement {
    let secs = started.elapsed().as_secs();
    let elapsed = format!("{}:{:02}", secs / 60, secs % 60);
    // Keep the probe tied to the same values applied to the rendered label.
    let role = gpui::Role::Status;
    let name = SharedString::from(Msg::clone_running_status(secs));
    let visible = SharedString::from(Msg::CloneRunning.t().replace("{}", &elapsed));
    #[cfg(feature = "gui-e2e")]
    clone_progress::record(Some(role), Some(&name), &visible, secs);
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
                    theme().ui_accent(),
                    "clone-progress-spinner",
                ))
                .child(
                    div()
                        .id("clone-progress-status")
                        .role(role)
                        .aria_label(name)
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .relative()
                        .child(super::e2e::measure_inside("clone-progress-status"))
                        .child(visible),
                ),
        )
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloneDestinationPresentation {
    pub displayed: Option<String>,
    pub accessibility: Option<String>,
    pub tooltip: Option<String>,
    pub summary: Option<String>,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CLONE_DESTINATION: std::cell::RefCell<Option<CloneDestinationPresentation>> =
        const { std::cell::RefCell::new(None) };
    static CLONE_COPY_ACTIVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(feature = "gui-e2e")]
fn record_clone_destination(
    displayed: Option<&str>,
    accessibility: Option<&str>,
    tooltip: Option<&str>,
) {
    CLONE_DESTINATION.with(|slot| {
        *slot.borrow_mut() = Some(CloneDestinationPresentation {
            displayed: displayed.map(str::to_owned),
            accessibility: accessibility.map(str::to_owned),
            tooltip: tooltip.map(str::to_owned),
            summary: None,
        });
    });
}

#[cfg(feature = "gui-e2e")]
pub fn clone_destination_presentation() -> Option<CloneDestinationPresentation> {
    CLONE_DESTINATION.with(|slot| slot.borrow().clone())
}

#[cfg(feature = "gui-e2e")]
pub fn take_clone_copy_activations() -> usize {
    CLONE_COPY_ACTIVATIONS.with(|count| count.replace(0))
}
