//! Labels and assignees for a New Issue, and who it is posted as (#866).
//!
//! The picker is the PR field picker ([`super::pr_fields`]) aimed at the
//! composer: confirming stores the selection in the composer's tab state, and
//! the composer's Create is the write that sends it (`gh issue create
//! --label … --assignee …`, refused before `gh` runs when the repository
//! cannot take a value).

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use kagi_domain::github::IssueCreateFields;

use super::i18n::Msg;
use super::modals::{FieldTarget, PrField};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

impl KagiApp {
    /// The `gh` login an Issue write from the active tab posts as: the one
    /// on its repository's host. `None` until that host's login has been
    /// read — the composer then claims nobody (#904 review).
    pub(super) fn issue_host_login(&self) -> Option<&str> {
        let repo = self.ui().issue_composer.base_repo.as_deref()?;
        self.github_host_logins
            .get(&kagi_git::github::repo_host(repo))
            .map(String::as_str)
    }

    /// Read the `gh` login on the host of the Issues repository, once per
    /// host (#904 review). `gh issue create -R <host>/<owner>/<repo>` posts as
    /// that host's identity, which on an Enterprise server is not the
    /// window-global github.com login. A failed read claims nobody and is
    /// asked again on the next Issues read.
    pub(super) fn ensure_issue_host_login(&mut self, base_repo: &str, cx: &mut Context<Self>) {
        let host = kagi_git::github::repo_host(base_repo);
        if !self.github_host_login_requests.insert(host.clone()) {
            return;
        }
        cx.spawn(async move |this, acx| {
            let ask = host.clone();
            let login = acx
                .background_executor()
                .spawn(async move { kagi_git::github::current_login(ask.as_deref()) })
                .await;
            let _ = this.update(acx, |app, cx| {
                match login {
                    Some(login) => {
                        app.github_host_logins.insert(host, login);
                    }
                    None => {
                        app.github_host_login_requests.remove(&host);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the picker for the New Issue composer's labels or assignees,
    /// starting from what the composer already holds.
    pub fn open_issue_fields_modal(&mut self, field: PrField, cx: &mut Context<Self>) {
        if field == PrField::Reviewers || self.has_active_modal() {
            return;
        }
        let Some(owner) = self.active_session() else {
            return;
        };
        let Some(state) = self.ui.get(&owner).map(|ui| &ui.issue_composer) else {
            return;
        };
        // The candidates come from the frozen Issues repository; without it
        // there is nothing to pick from and nothing to create in.
        let Some(base_repo) = state.base_repo.clone() else {
            return;
        };
        // Not before the saved draft has loaded: its picks would replace these,
        // or a save from here would clear the draft on disk (#903).
        let Some(editor) = state.editors.get(&None).filter(|editor| editor.loaded) else {
            return;
        };
        let current = match field {
            PrField::Labels => editor.fields.labels.clone(),
            _ => editor.fields.assignees.clone(),
        };
        self.open_fields_picker(
            FieldTarget::NewIssue { owner },
            base_repo.clone(),
            field,
            current,
        );
        klog!("issue-fields: open field={:?}", field);
        cx.notify();
        self.read_field_candidates(field, base_repo, cx);
    }

    /// The picker's Apply for a New Issue: store the selection, with the
    /// draft (#903), in the composer of the session that opened the picker —
    /// not the active one, which a tab switch may have changed (#904 review).
    /// A closed owner takes nothing. Nothing is sent until the composer's
    /// Create.
    pub(super) fn apply_issue_fields(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.pr_fields_modal().cloned() else {
            return;
        };
        let FieldTarget::NewIssue { owner } = modal.target else {
            return;
        };
        self.clear_pr_fields_modal();
        let Some(editor) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.issue_composer.editors.get_mut(&None))
        else {
            return;
        };
        match modal.field {
            PrField::Labels => editor.fields.labels = modal.selected,
            PrField::Assignees => editor.fields.assignees = modal.selected,
            PrField::Reviewers => {}
        }
        if let Some(repo) = editor.repo.clone() {
            self.save_issue_draft_for(owner, repo, None, cx);
        }
        cx.notify();
    }
}

/// The New Issue composer's fields row: labels and assignees, each its own
/// picker's way in, and the read-only author.
pub(super) fn render_issue_fields(
    app: &KagiApp,
    fields: &IssueCreateFields,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let field = |field: PrField, open_id: &'static str, value_id: &'static str| {
        let items = match field {
            PrField::Labels => &fields.labels,
            _ => &fields.assignees,
        };
        let value: AnyElement = if items.is_empty() {
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrRailNone.t()))
                .into_any_element()
        } else {
            let mut pills = div().flex().flex_row().flex_wrap().gap_1();
            for item in items {
                pills = pills.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(theme().selected))
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(safe_text(item)),
                );
            }
            pills.into_any_element()
        };
        // A Button, not a clickable div (#904 review): `Role::Button` named
        // by the field, a tab stop, and Enter / Space open the picker like a
        // click (#354). The chips are its content.
        let control = Button::new(open_id)
            .ghost()
            .compact()
            .small()
            .h_auto()
            .min_h(theme::scaled_px(24.))
            .min_w(px(0.))
            .label(field.title())
            .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                app.open_issue_fields_modal(field, cx)
            }))
            .child(super::e2e::measure_control(value_id, value));
        super::e2e::measure_control(open_id, control)
    };
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(theme::scaled_px(12.))
        .child(field(
            PrField::Labels,
            "issue-field-open-labels",
            "issue-field-value-labels",
        ))
        .child(field(
            PrField::Assignees,
            "issue-field-open-assignees",
            "issue-field-value-assignees",
        ))
        .child(div().flex_1())
        // `gh issue create` posts as the authenticated user on the
        // repository's host; there is no choice to offer, so the author is
        // shown, not edited. Until that host's login has been read, nobody is
        // claimed.
        .children(app.issue_host_login().map(|login| {
            super::e2e::measure_control(
                "issue-composer-posted-as",
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(
                        Msg::IssuePostedAs.t().replace("{}", login),
                    )),
            )
        }))
        .into_any_element()
}
