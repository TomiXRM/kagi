//! Repository owner-trust prompt (ADR-0160 / issue #310).
//!
//! ADR-0146 hardened the CLI path; git enforces `safe.directory` there. The
//! git2 (libgit2) path does not, so [`kagi_git::Backend::open`] evaluates owner
//! trust itself and marks a foreign-owned, untrusted repo `Untrusted`. Reads
//! stay allowed; every write is refused by `Backend::run` until the user
//! confirms trust here.
//!
//! This is a plain confirm modal (no `OperationPlan`), modelled on the Editor
//! Workspace dirty-guard: Enter/Esc come free from the root
//! `confirm_active_modal` / `cancel_active_modal` plumbing.

use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::button_style::{modal_button, ModalButtonKind};
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_modal_title_row, ModalIcon};
use super::modals::TrustRepoModal;
use super::theme::{self, theme as current_theme};
use super::KagiApp;

impl KagiApp {
    /// If the active tab's repository opened `Untrusted` (foreign owner, not in
    /// `safe.directory` or the trust store), raise the trust prompt. Called
    /// right after a session is opened. No-op when trusted, when there is no
    /// session, or when another modal already owns the slot.
    pub fn prompt_trust_if_untrusted(&mut self) {
        if self.active_modal.is_some() {
            return;
        }
        let Some(session) = self.ui().repo_session.as_ref() else {
            return;
        };
        if session.backend().trust().is_trusted() {
            return;
        }
        let repo_path = session.path().to_path_buf();
        klog!("repo untrusted (foreign owner): {}", repo_path.display());
        self.set_trust_repo_modal(TrustRepoModal {
            repo_path,
            error: None,
        });
    }

    /// Confirm trust: persist the grant (`trusted_repos`) and re-open the
    /// session so both the read backend and the (lazily spawned) write worker
    /// pick up the fresh `Trusted` state — the worker caches trust at spawn, so
    /// a re-open is the clean way to invalidate it.
    pub fn confirm_trust_repo(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.trust_repo_modal().cloned() else {
            return;
        };
        let error = match kagi_git::trust::trust_repo(&modal.repo_path) {
            Ok(()) => {
                klog!("repo trusted: {}", modal.repo_path.display());
                match kagi_git::session::RepoSession::open(&modal.repo_path) {
                    Ok(session) => {
                        if let Some(ui) = self.ui_mut() {
                            ui.repo_session = Some(session);
                        }
                        None
                    }
                    Err(e) => Some(format!(
                        "{}\n\n{e}",
                        Msg::TrustRepoReopenFailed.t().replacen(
                            "{}",
                            &modal.repo_path.display().to_string(),
                            1
                        )
                    )),
                }
            }
            Err(e) => {
                klog!("trust grant failed: {e}");
                Some(format!(
                    "{}\n\n{e}",
                    Msg::TrustRepoSaveFailed.t().replacen(
                        "{}",
                        &modal.repo_path.display().to_string(),
                        1
                    )
                ))
            }
        };
        if let Some(error) = error {
            self.set_trust_repo_modal(TrustRepoModal {
                repo_path: modal.repo_path,
                error: Some(error),
            });
        } else {
            self.clear_trust_repo_modal();
        }
        cx.notify();
    }

    /// Dismiss the trust prompt without granting. The repo stays read-only;
    /// re-opening the repo will prompt again.
    pub fn cancel_trust_repo_modal(&mut self) {
        self.clear_trust_repo_modal();
    }
}

/// Owner-trust confirmation overlay. Not a Git write — no plan card — just a
/// trust-or-cancel gate. Wired to `confirm_trust_repo` / `cancel_trust_repo_modal`.
pub(crate) fn render_trust_repo_modal(
    modal: TrustRepoModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.cancel_trust_repo_modal();
        cx.notify();
    });
    let confirm = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.confirm_trust_repo(cx);
    });

    let body = super::modal_shell::modal_scroll_body()
        .relative()
        .child(super::e2e::measure_inside("trust-repo-body"))
        .child(render_modal_title_row(
            SharedString::from(Msg::TrustRepoTitle.t()),
            Some((
                ModalIcon::Path("icons/user-plus.svg"),
                current_theme().color_warning,
            )),
        ))
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_sub))
                .child(SharedString::from(Msg::TrustRepoBody.t())),
        )
        .child(trust_path(SharedString::from(
            modal.repo_path.display().to_string(),
        )))
        .when_some(modal.error.clone(), |card, error| {
            card.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .id("trust-repo-error")
                            .flex_1()
                            .min_w_0()
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(rgb(current_theme().color_blocker))
                            .child(SharedString::from(error.clone()))
                            .child(super::e2e::measure_control(
                                "trust-repo-error-tail",
                                div().h(theme::scaled_px(1.)),
                            )),
                    )
                    .child(super::modal_copy::modal_copy_button(
                        "trust-repo-copy-error",
                        Msg::ModalCopyAll.t(),
                        error,
                        cx,
                    )),
            )
        });
    let card = super::modal_shell::modal_card(460.)
        .relative()
        .child(super::e2e::measure_inside("trust-repo-card"))
        .child(body)
        .child(
            super::button_style::modal_button_row()
                .flex_shrink_0()
                .child(super::e2e::measure_control(
                    "trust-repo-cancel",
                    modal_button(
                        "trust-repo-cancel",
                        Msg::PlanCancel.t(),
                        ModalButtonKind::Cancel,
                        None,
                        cancel,
                        cx,
                    ),
                ))
                .child(super::e2e::measure_control(
                    "trust-repo-confirm",
                    modal_button(
                        "trust-repo-confirm",
                        if modal.error.is_some() {
                            Msg::TrustRepoRetry.t()
                        } else {
                            Msg::TrustRepoConfirm.t()
                        },
                        ModalButtonKind::Primary,
                        None,
                        confirm,
                        cx,
                    ),
                )),
        );

    modal_overlay(card).into_any_element()
}

fn trust_path(full: SharedString) -> impl IntoElement {
    let tooltip = full.clone();
    div()
        .id("trust-repo-path")
        .role(gpui::Role::Label)
        .aria_label(full.clone())
        .aria_description(full.clone())
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .relative()
        .child(super::e2e::measure_inside("trust-repo-path"))
        .child(full)
        .text_xs()
        .text_color(rgb(current_theme().text_label))
        .hover_tooltip_builder("trust-repo-path-help", move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
}
