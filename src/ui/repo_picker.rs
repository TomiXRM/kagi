//! The repository picker behind the tab strip's `+` (#923, ADR-0219).
//!
//! A window-global modal, like Remote Browse: it belongs to no tab, and what
//! it produces is a new one. It lists the recently opened repositories and
//! leads to the folder dialog and to Remote Browse.

use std::path::PathBuf;

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, FocusHandle, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;

use super::i18n::Msg;
use super::modal_shell::{modal_body, modal_card, MODAL_W_LG};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// The picker's state: the recent list as it was when it opened, so a row
/// cannot shift under the pointer.
#[derive(Clone, Debug, Default)]
pub struct RepoPickerModal {
    pub recent: Vec<PathBuf>,
}

impl KagiApp {
    /// Open the picker (the tab strip's `+`).
    pub fn open_repo_picker(&mut self, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        let recent = super::tabs::recent_repos();
        klog!("repo-picker: open recent={}", recent.len());
        self.set_repo_picker(RepoPickerModal { recent });
        cx.notify();
    }

    pub fn cancel_repo_picker(&mut self) {
        self.clear_repo_picker();
    }

    /// Open one of the recent repositories: an already open one is switched
    /// to, as everywhere else.
    pub fn repo_picker_open_recent(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clear_repo_picker();
        self.open_repository(path, cx);
        cx.notify();
    }

    /// Hand over to the folder dialog, as `+` used to do directly.
    pub fn repo_picker_open_folder(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.clear_repo_picker();
        self.pick_repository(window, cx);
        cx.notify();
    }

    /// Hand over to Remote Browse, which takes the modal slot.
    pub fn repo_picker_connect_remote(&mut self, cx: &mut Context<Self>) {
        self.clear_repo_picker();
        self.open_remote_browse(cx);
    }
}

pub(crate) fn render_repo_picker(
    modal: RepoPickerModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let mut recent = div().flex().flex_col().gap_1();
    if modal.recent.is_empty() {
        recent = recent.child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::RepoPickerNoRecent.t())),
        );
    }
    for (i, path) in modal.recent.into_iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let full = path.display().to_string();
        let open = cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.repo_picker_open_recent(path.clone(), cx)
        });
        recent = recent.child(super::e2e::measure_control(
            format!("repo-picker-recent-{i}"),
            Button::new(("repo-picker-recent", i))
                .ghost()
                .w_full()
                .h_auto()
                .py_1()
                .justify_start()
                .label(name)
                .on_click(open)
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(safe_text(&full)),
                ),
        ));
    }

    let open_folder = cx
        .listener(|app, _: &gpui::ClickEvent, window, cx| app.repo_picker_open_folder(window, cx));
    let remote = cx.listener(|app, _: &gpui::ClickEvent, _, cx| app.repo_picker_connect_remote(cx));
    let cancel = cx.listener(|app, _: &gpui::ClickEvent, window, cx| {
        app.cancel_repo_picker();
        if let Some(root) = app.root_focus.clone() {
            window.focus(&root, cx);
        }
        cx.notify();
    });

    let card = modal_card(MODAL_W_LG)
        .child(super::modal_renderers::render_modal_title_row(
            SharedString::from(Msg::RepoPickerTitle.t()),
            None,
        ))
        .child(
            modal_body()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(Msg::RepoPickerRecent.t())),
                )
                .child(
                    div()
                        .id("repo-picker-recent-list")
                        .max_h(theme::scaled_px(360.))
                        .overflow_y_scroll()
                        .child(recent),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(super::e2e::measure_control(
                    "repo-picker-open-folder",
                    Button::new("repo-picker-open-folder")
                        .label(Msg::RepoPickerOpenFolder.t())
                        .small()
                        .on_click(open_folder),
                ))
                .child(super::e2e::measure_control(
                    "repo-picker-remote",
                    Button::new("repo-picker-remote")
                        .label(Msg::RepoPickerConnectRemote.t())
                        .small()
                        .on_click(remote),
                ))
                .child(div().flex_1())
                .child(super::e2e::measure_control(
                    "repo-picker-cancel",
                    Button::new("repo-picker-cancel")
                        .label(Msg::PlanCancel.t())
                        .small()
                        .on_click(cancel),
                )),
        );
    // The overlay is absolute: measuring it would anchor it to the probe's
    // own (zero-height) box, so the card is what is measured.
    let overlay = super::modal_renderers::modal_overlay(super::e2e::measure_control(
        "active-modal/repo-picker",
        card,
    ));
    match focus_handle {
        Some(focus) => overlay.track_focus(&focus).into_any_element(),
        None => overlay.into_any_element(),
    }
}
