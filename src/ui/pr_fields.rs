//! The PR field picker: the gear on a properties row (ADR-0200 §11).
//!
//! Reviewers, assignees and labels are the three things a reader of a PR most
//! often has to change, and until now kagi could only show them. The picker is
//! a read of what the repository offers plus a toggle list; confirming sends
//! one `gh pr edit` through the write family.

use gpui::{div, prelude::*, px, rgb, Context, SharedString};

use super::i18n::Msg;
use super::modal_shell::{modal_body, modal_card, MODAL_W_SM};
use super::modals::{PrField, PrFieldsModal};
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

impl PrField {
    /// The row label this field is edited from.
    pub(super) fn title(self) -> &'static str {
        match self {
            PrField::Reviewers => Msg::PrRailReviewers.t(),
            PrField::Assignees => Msg::PrRailAssignees.t(),
            PrField::Labels => Msg::PrRailLabels.t(),
        }
    }
}

impl KagiApp {
    /// Open the picker for one field, and start reading what the repository
    /// offers for it.
    ///
    /// The read is only for the *candidates*: what the PR already carries is
    /// listed from the tab's own snapshot, so a value can be removed with no
    /// network at all - which is the difference between a picker that works
    /// offline and one that shows an empty list (see the comment write, whose
    /// `gh repo view` lookup failed exactly that way).
    pub fn open_pr_fields_modal(&mut self, field: PrField, cx: &mut Context<Self>) {
        let Some(pr) = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
            .map(|t| t.pr.clone())
        else {
            return;
        };
        let current = match field {
            PrField::Reviewers => pr.reviewers.clone(),
            PrField::Assignees => pr.assignees.clone(),
            PrField::Labels => pr.labels.iter().map(|l| l.name.clone()).collect(),
        };
        self.pr_fields_generation = self.pr_fields_generation.wrapping_add(1);
        let generation = self.pr_fields_generation;
        self.set_pr_fields_modal(PrFieldsModal {
            generation,
            number: pr.number,
            base_repo: pr.base_repo.clone(),
            field,
            current: current.clone(),
            selected: current,
            candidates: None,
            error: None,
        });
        klog!("pr-fields: open #{} field={:?}", pr.number, field);
        cx.notify();

        let Some(repo) = self.repo_path.clone() else {
            return;
        };
        let base_repo = pr.base_repo.clone();
        cx.spawn(async move |this, acx| {
            let read = acx
                .background_executor()
                .spawn(async move {
                    match field {
                        PrField::Labels => kagi_git::github::repo_labels(&repo, &base_repo)
                            .map(|labels| labels.into_iter().map(|l| l.name).collect::<Vec<_>>()),
                        _ => kagi_git::github::repo_assignable_users(&repo, &base_repo),
                    }
                })
                .await;
            let _ = this.update(acx, |app, cx| {
                // Only the picker that asked for this list may take it: the
                // reader may have cancelled it, or opened another field, while
                // `gh` was out.
                let Some(modal) = app.pr_fields_modal_mut() else {
                    return;
                };
                if modal.generation != generation {
                    return;
                }
                match read {
                    Ok(list) => modal.candidates = Some(list),
                    Err(error) => {
                        modal.candidates = Some(Vec::new());
                        modal.error = Some(SharedString::from(error.to_string()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Toggle one value in the open picker.
    pub fn pr_fields_toggle(&mut self, value: String, cx: &mut Context<Self>) {
        if let Some(modal) = self.pr_fields_modal_mut() {
            if let Some(ix) = modal.selected.iter().position(|v| *v == value) {
                modal.selected.remove(ix);
            } else {
                modal.selected.push(value);
            }
        }
        cx.notify();
    }
}

/// The picker overlay: every value the PR carries or the repository offers,
/// with the selected ones marked. One scroll region, per #454.
pub(crate) fn render_pr_fields_modal(
    modal: PrFieldsModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use gpui_component::Disableable as _;
    // What the PR carries always appears, even when it is not in (or ahead of)
    // the repository's list: a value that cannot be seen cannot be removed.
    let mut rows: Vec<String> = modal.current.clone();
    for value in modal.selected.iter().chain(
        modal
            .candidates
            .as_ref()
            .map(|c| c.iter())
            .unwrap_or_default(),
    ) {
        if !rows.contains(value) {
            rows.push(value.clone());
        }
    }
    let loading = modal.candidates.is_none();
    let changed = {
        let (add, remove) = kagi_domain::github::PrFieldEdit::diff(&modal.current, &modal.selected);
        !add.is_empty() || !remove.is_empty()
    };

    let mut list = div()
        .id("pr-fields-list")
        .flex_1()
        .min_h(px(0.))
        .max_h(theme::scaled_px(320.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (i, value) in rows.iter().enumerate() {
        let picked = modal.selected.contains(value);
        let v = value.clone();
        let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_fields_toggle(v.clone(), cx);
        });
        list = list.child(
            div()
                .id(("pr-fields-row", i))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(toggle)
                .child(
                    div()
                        .w(theme::scaled_px(14.))
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(rgb(if picked {
                            theme().color_success
                        } else {
                            theme().text_muted
                        }))
                        .child(SharedString::from(if picked {
                            "\u{2713}"
                        } else {
                            "\u{00b7}"
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .text_color(rgb(if picked {
                            theme().text_main
                        } else {
                            theme().text_sub
                        }))
                        .child(safe_text(value)),
                ),
        );
    }

    let cancel = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.clear_pr_fields_modal();
        cx.notify();
    });
    let confirm = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.start_pr_edit(cx);
    });

    let card = modal_card(MODAL_W_SM)
        .child(super::modal_renderers::render_modal_title_row(
            SharedString::from(format!(
                "#{} \u{00b7} {}",
                modal.number,
                modal.field.title()
            )),
            None,
        ))
        .child(
            modal_body()
                .child(list)
                // A read that failed says so; it never masquerades as "the
                // repository offers nothing".
                .children(modal.error.clone().map(|error| {
                    div()
                        .text_xs()
                        .text_color(rgb(theme().color_blocker))
                        .child(error)
                }))
                .when(loading, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(Msg::EditorWorkspaceLoading.t())),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    gpui_component::button::Button::new("pr-fields-cancel")
                        .label(Msg::PlanCancel.t())
                        .on_click(cancel),
                )
                .child(
                    gpui_component::button::Button::new("pr-fields-confirm")
                        .label(Msg::PrFieldsApply.t())
                        .disabled(!changed)
                        .on_click(confirm),
                ),
        );
    super::modal_renderers::modal_overlay(card).into_any_element()
}
