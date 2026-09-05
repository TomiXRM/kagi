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
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;

use super::button_style::KagiButton;
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
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        if session.backend().trust().is_trusted() {
            return;
        }
        let repo_path = session.path().to_path_buf();
        klog!("repo untrusted (foreign owner): {}", repo_path.display());
        self.set_trust_repo_modal(TrustRepoModal { repo_path });
    }

    /// Confirm trust: persist the grant (`trusted_repos`) and re-open the
    /// session so both the read backend and the (lazily spawned) write worker
    /// pick up the fresh `Trusted` state — the worker caches trust at spawn, so
    /// a re-open is the clean way to invalidate it.
    pub fn confirm_trust_repo(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.trust_repo_modal().cloned() else {
            return;
        };
        match kagi_git::trust::trust_repo(&modal.repo_path) {
            Ok(()) => {
                klog!("repo trusted: {}", modal.repo_path.display());
                self.repo_session = kagi_git::session::RepoSession::open(&modal.repo_path).ok();
            }
            Err(e) => {
                klog!("trust grant failed: {e}");
            }
        }
        self.clear_trust_repo_modal();
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

    let card = div()
        .w(theme::scaled_px(460.))
        // Same popup surface as `modal_shell::modal_card` and the Settings
        // panel (`theme.panel`); `modal` is lighter and read as a different
        // surface class next to them (#454 follow-up).
        .bg(rgb(current_theme().panel))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
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
        .child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(modal.repo_path.display().to_string())),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("trust-repo-cancel")
                        .label(Msg::PlanCancel.t())
                        .ghost()
                        .small()
                        .on_click(cancel),
                )
                .child(
                    KagiButton::accent(
                        "trust-repo-confirm",
                        Msg::TrustRepoConfirm.t(),
                        current_theme().color_success,
                        cx,
                    )
                    .small()
                    .on_click(confirm),
                ),
        );

    modal_overlay(card).into_any_element()
}
