//! The field picker: the gear on a PR properties row (ADR-0200 §11), and the
//! label / assignee buttons of the New Issue composer (#866).
//!
//! Reviewers, assignees and labels are the three things a reader of a PR most
//! often has to change, and until now kagi could only show them. The picker is
//! a read of what the repository offers plus a toggle list; confirming sends
//! one `gh pr edit` through the write family. For a New Issue, confirming only
//! stores the selection in the composer; its Create button is the write.

use gpui::{div, prelude::*, px, rgb, Context, Focusable as _, SharedString};

use super::i18n::Msg;
use super::modal_shell::{modal_body, modal_card, MODAL_W_SM};
use super::modals::{FieldTarget, PrField, PrFieldsModal};
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
        let Some(owner) = self.active_session() else {
            return;
        };
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
        self.open_fields_picker(
            FieldTarget::Pr {
                number: pr.number,
                owner,
            },
            pr.base_repo.clone(),
            field,
            current,
        );
        klog!("pr-fields: open #{} field={:?}", pr.number, field);
        cx.notify();
        self.read_field_candidates(field, pr.base_repo.clone(), cx);
    }

    /// Show the picker for `target`, starting from `current`. The candidates
    /// arrive from [`Self::read_field_candidates`].
    pub(super) fn open_fields_picker(
        &mut self,
        target: FieldTarget,
        base_repo: String,
        field: PrField,
        current: Vec<String>,
    ) {
        self.pr_fields_generation = self.pr_fields_generation.wrapping_add(1);
        self.set_pr_fields_modal(PrFieldsModal {
            generation: self.pr_fields_generation,
            target,
            base_repo,
            field,
            current: current.clone(),
            selected: current,
            candidates: None,
            error: None,
        });
    }

    /// Read what the repository offers for the open picker's field.
    pub(super) fn read_field_candidates(
        &mut self,
        field: PrField,
        base_repo: String,
        cx: &mut Context<Self>,
    ) {
        let generation = self.pr_fields_generation;
        let Some(repo) = self.repo_path.clone() else {
            return;
        };
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

    /// The picker's Apply: a PR edit is a write; a New Issue selection is
    /// stored for the composer's Create.
    pub fn confirm_pr_fields(&mut self, cx: &mut Context<Self>) {
        match self.pr_fields_modal().map(|modal| modal.target) {
            Some(FieldTarget::Pr { .. }) => self.start_pr_edit(cx),
            Some(FieldTarget::NewIssue { .. }) => self.apply_issue_fields(cx),
            None => {}
        }
    }

    /// Build the picker's filter box while the picker is open and drop it
    /// when it closes. Runs on the window-bearing render pass:
    /// `InputState::new` needs a `&mut Window`, and the overlay renderer has
    /// none. Focused on creation, so typing starts at once.
    pub(crate) fn sync_pr_fields_input(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.pr_fields_modal().is_some();
        match (open, self.pr_fields_input.is_some()) {
            (true, false) => {
                let input = cx.new(|cx| {
                    gpui_component::input::InputState::new(window, cx)
                        .placeholder(Msg::PrFieldsFilterPlaceholder.t())
                });
                cx.subscribe(&input, |_this, _input, event, cx| {
                    if matches!(event, gpui_component::input::InputEvent::Change) {
                        cx.notify();
                    }
                })
                .detach();
                input.update(cx, |st, cx| st.focus(window, cx));
                self.pr_fields_input = Some(input);
            }
            (false, true) => {
                // #755: the filter is leaving the dispatch tree. Do not steal
                // focus if a replacement modal already focused its own input.
                if self
                    .pr_fields_input
                    .as_ref()
                    .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
                {
                    if let Some(root) = self.root_focus.as_ref() {
                        window.focus(root, cx);
                    }
                }
                self.pr_fields_input = None;
            }
            _ => {}
        }
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
    app: &KagiApp,
    modal: PrFieldsModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use gpui_component::button::ButtonVariants as _;
    use gpui_component::{Disableable as _, Selectable as _};
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
    // Fuzzy filter (the command palette's matcher): a login is remembered by
    // a few letters more often than by its spelling. Selected values always
    // stay listed, so a filter can never hide what is about to be sent.
    let query = app
        .pr_fields_input
        .as_ref()
        .map(|input| input.read(cx).value().to_string())
        .unwrap_or_default();
    let rows: Vec<String> = if query.trim().is_empty() {
        rows
    } else {
        let mut scored: Vec<(i64, String)> = rows
            .into_iter()
            .filter_map(|value| {
                if modal.selected.contains(&value) {
                    return Some((i64::MAX, value));
                }
                super::command_palette::fuzzy_match(&query, &value).map(|score| (score, value))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, value)| value).collect()
    };
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
    for value in rows.iter() {
        let picked = modal.selected.contains(value);
        let v = value.clone();
        let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_fields_toggle(v.clone(), cx);
        });
        // A Button per row (#904 review): `Role::Button` with the pick as
        // `aria_selected`, a tab stop, and Space toggles it like a click
        // (#354). Enter stays the modal's Apply, as everywhere else.
        list = list.child(
            // Keyed by the value, not the row index: picking a row re-sorts it
            // to the top, and an index id would hand its focus to another
            // candidate (#904 review).
            gpui_component::button::Button::new(SharedString::from(format!(
                "pr-fields-row-{value}"
            )))
            .ghost()
            .compact()
            .w_full()
            .h_auto()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .selected(picked)
            .on_click(toggle)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
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
            ),
        );
    }

    let cancel = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.clear_pr_fields_modal();
        cx.notify();
    });
    let confirm = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.confirm_pr_fields(cx);
    });

    let card = modal_card(MODAL_W_SM)
        .child(super::modal_renderers::render_modal_title_row(
            SharedString::from(match modal.target {
                FieldTarget::Pr { number, .. } => {
                    format!("#{} \u{00b7} {}", number, modal.field.title())
                }
                FieldTarget::NewIssue { .. } => format!(
                    "{} \u{00b7} {}",
                    Msg::IssueNewFieldsTitle.t(),
                    modal.field.title()
                ),
            }),
            None,
        ))
        .child(
            modal_body()
                .children(
                    app.pr_fields_input
                        .as_ref()
                        .map(gpui_component::input::Input::new),
                )
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
                .child(super::e2e::measure_control(
                    "pr-fields-cancel",
                    gpui_component::button::Button::new("pr-fields-cancel")
                        .label(Msg::PlanCancel.t())
                        .on_click(cancel),
                ))
                .child(super::e2e::measure_control(
                    "pr-fields-confirm",
                    gpui_component::button::Button::new("pr-fields-confirm")
                        .label(Msg::PrFieldsApply.t())
                        .disabled(!changed)
                        .on_click(confirm),
                )),
        );
    super::modal_renderers::modal_overlay(card).into_any_element()
}
