//! Session-owned source editors. Window-bearing render only creates inputs;
//! subscriptions persist edits through the existing draft storage boundary.
use super::{i18n::Msg, theme::theme, KagiApp};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Entity, SharedString, Window};
use gpui_component::input::{Input, InputEvent, InputState, Paste};
use gpui_component::{button::Button, text::TextView, Disableable, Sizable};
use kagi_domain::issue_composer::{fenced_code_paste, IssueDraft};
use std::collections::HashMap;

gpui::actions!(issues_composer, [FocusIssueEditor]);

#[derive(Default)]
pub(super) struct IssuesComposerState {
    pub base_repo: Option<String>,
    pub repo_error: Option<String>,
    pub repo_loading: bool,
    pub editors: HashMap<Option<u64>, IssueEditor>,
}

#[derive(Default)]
pub(super) struct IssueEditor {
    pub draft: IssueDraft,
    pub repo: Option<std::path::PathBuf>,
    pub storage_version: u64,
    pub body_input: Option<Entity<InputState>>,
    pub title_input: Option<Entity<InputState>>,
    pub loaded: bool,
    pub loading: bool,
    pub sync_inputs: bool,
    pub preview: bool,
    pub focused: bool,
    pub saving: bool,
    pub save_error: Option<String>,
}

impl KagiApp {
    pub(crate) fn sync_issue_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.issues_mode_open() {
            return;
        }
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let selected = self.ui().selected_github_issue;
        for number in [None, selected]
            .into_iter()
            .take(if selected.is_some() { 2 } else { 1 })
        {
            let Some(editor) = self
                .ui
                .get_mut(&owner)
                .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
            else {
                continue;
            };
            if editor.body_input.is_none() {
                let body = cx.new(|cx| {
                    InputState::new(window, cx)
                        .code_editor("markdown")
                        .placeholder(if number.is_some() {
                            Msg::PrCommentPlaceholder.t()
                        } else {
                            Msg::IssueCompose.t()
                        })
                });
                let title = number.is_none().then(|| {
                    cx.new(|cx| {
                        InputState::new(window, cx).placeholder(Msg::IssueTitleOptional.t())
                    })
                });
                body.update(cx, |st, cx| {
                    st.set_value(editor.draft.body.clone(), window, cx)
                });
                if let Some(title) = &title {
                    title.update(cx, |st, cx| {
                        st.set_value(editor.draft.title.clone(), window, cx)
                    });
                }
                let mut inputs = vec![body.clone()];
                inputs.extend(title.iter().cloned());
                for input in inputs {
                    let repo = repo.clone();
                    cx.subscribe(&input, move |app, _, event, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let Some(editor) = app
                            .ui
                            .get_mut(&owner)
                            .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                        else {
                            return;
                        };
                        // Programmatic restore/clear is not a user edit.
                        if editor.sync_inputs {
                            return;
                        }
                        let Some(body) = &editor.body_input else {
                            return;
                        };
                        let title = editor
                            .title_input
                            .as_ref()
                            .map(|title| title.read(cx).value().to_string())
                            .unwrap_or_else(|| editor.draft.title.clone());
                        let changed = editor
                            .draft
                            .update(title, body.read(cx).value().to_string());
                        if changed {
                            app.save_issue_draft_for(owner, repo.clone(), number, cx);
                        }
                        cx.notify();
                    })
                    .detach();
                }
                editor.body_input = Some(body);
                editor.title_input = title;
                editor.sync_inputs = false;
            } else if editor.sync_inputs {
                if let Some(input) = &editor.body_input {
                    input.update(cx, |st, cx| {
                        st.set_value(editor.draft.body.clone(), window, cx)
                    });
                }
                if let Some(input) = &editor.title_input {
                    input.update(cx, |st, cx| {
                        st.set_value(editor.draft.title.clone(), window, cx)
                    });
                }
                editor.sync_inputs = false;
            }
        }
    }

    pub(super) fn toggle_issue_focus(
        &mut self,
        number: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_active_modal() {
            return;
        }
        let Some(ui) = self.ui_mut() else {
            return;
        };
        let Some(was_focused) = ui
            .issue_composer
            .editors
            .get(&number)
            .map(|editor| editor.focused)
        else {
            return;
        };
        let focused = !was_focused;
        if focused {
            for editor in ui.issue_composer.editors.values_mut() {
                editor.focused = false;
            }
        }
        let Some(editor) = ui.issue_composer.editors.get_mut(&number) else {
            return;
        };
        editor.focused = focused;
        if focused {
            editor.preview = false;
        }
        let input = editor.body_input.clone();
        if let Some(input) = input {
            input.update(cx, |st, cx| st.focus(window, cx));
        }
        cx.notify();
    }
}

pub(super) fn render_composer(
    app: &KagiApp,
    number: Option<u64>,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let state = &app.ui().issue_composer;
    let Some(editor) = state.editors.get(&number) else {
        return div().into_any_element();
    };
    let Some(input) = editor.body_input.clone() else {
        return div().into_any_element();
    };
    let id = if number.is_some() {
        "issue-reply-composer"
    } else {
        "issue-composer"
    };
    let held = app.repo_path.as_ref().is_some_and(|repo| {
        app.transport_holds.contains(
            repo,
            if number.is_some() {
                "issue-comment"
            } else {
                "issue-create"
            },
        )
    });
    let disabled = !editor.loaded || state.base_repo.is_none() || held || app.op_latched();
    let mut composer = div()
        .id(id)
        .key_context("IssueComposer")
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(super::pr_attention::card_border())
        .on_action(cx.listener(move |app, _: &FocusIssueEditor, window, cx| {
            app.toggle_issue_focus(number, window, cx)
        }))
        .child(
            div().text_sm().text_color(rgb(theme().text_muted)).child(
                state
                    .base_repo
                    .clone()
                    .unwrap_or_else(|| Msg::IssueRepoUnavailable.t().into()),
            ),
        );
    if number.is_none() {
        let Some(title) = editor.title_input.clone() else {
            return div().into_any_element();
        };
        composer = composer.child(Input::new(&title).small());
    }
    let body = if editor.preview {
        let markdown = kagi_ui_editor::markdown::pad_inline_code(
            &kagi_domain::message::sanitize_markdown_for_view(&editor.draft.body),
        );
        div()
            .min_h(px(80.))
            .child(
                TextView::markdown(
                    ("issue-preview", number.unwrap_or(0) as usize),
                    SharedString::from(kagi_ui_core::markdown::flatten_html_blocks(&markdown)),
                )
                .selectable(true),
            )
            .into_any_element()
    } else {
        let paste_input = input.clone();
        div()
            .font_family(super::theme::MONO_FONT)
            .h(px(if editor.focused {
                420.
            } else if editor.draft.body.lines().count() > 3 || editor.draft.body.contains("```") {
                220.
            } else {
                100.
            }))
            .capture_action(cx.listener(move |app, _: &Paste, window, cx| {
                if app.has_active_modal() {
                    return;
                }
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return;
                };
                let mut fenced = fenced_code_paste(&text, None);
                if fenced == text {
                    return;
                }
                paste_input.update(cx, |st, cx| {
                    // An opening fence must start on its own line even when
                    // code is pasted immediately after prose.
                    if st.cursor_position().character > 0 {
                        fenced.insert(0, '\n');
                    }
                    st.replace(fenced, window, cx);
                    let pos = st.cursor_position();
                    st.set_cursor_position(pos, window, cx);
                });
                cx.stop_propagation();
            }))
            .child(Input::new(&input).h_full())
            .into_any_element()
    };
    composer = composer.child(body).child(
        div()
            .flex()
            .gap_2()
            .items_center()
            .child(super::e2e::measure_control(
                if number.is_some() {
                    "issue-reply-write"
                } else {
                    "issue-composer-write"
                },
                Button::new(SharedString::from(format!("{id}-write")))
                    .label(Msg::IssueWrite.t())
                    .small()
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if let Some(editor) = app
                            .ui_mut()
                            .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                        {
                            editor.preview = false;
                        }
                        cx.notify();
                    })),
            ))
            .child(super::e2e::measure_control(
                if number.is_some() {
                    "issue-reply-preview"
                } else {
                    "issue-composer-preview"
                },
                Button::new(SharedString::from(format!("{id}-preview")))
                    .label(Msg::IssuePreview.t())
                    .small()
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if let Some(editor) = app
                            .ui_mut()
                            .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                        {
                            editor.preview = true;
                        }
                        cx.notify();
                    })),
            ))
            .child(
                Button::new(SharedString::from(format!("{id}-focus")))
                    .label(if editor.focused {
                        Msg::IssueExitFocus.t()
                    } else {
                        Msg::IssueFocusEditor.t()
                    })
                    .small()
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.toggle_issue_focus(number, window, cx)
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new(SharedString::from(format!("{id}-submit")))
                    .label(if number.is_some() {
                        Msg::IssueReply.t()
                    } else {
                        Msg::IssueCreate.t()
                    })
                    .small()
                    .disabled(disabled)
                    .on_click(cx.listener(move |app, _, _, cx| app.start_issue_write(number, cx))),
            ),
    );
    if let Some(error) = editor.save_error.as_ref() {
        composer = composer.child(
            div()
                .text_xs()
                .text_color(rgb(theme().color_blocker))
                .child(error.clone()),
        );
    }
    if let Some(error) = state.repo_error.as_ref() {
        composer = composer.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(rgb(theme().color_blocker))
                        .child(error.clone()),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-retry-repo")))
                        .label(Msg::PrRefresh.t())
                        .small()
                        .disabled(state.repo_loading)
                        .on_click(cx.listener(|app, _, _, cx| app.refresh_github_issues(cx))),
                ),
        );
    } else if editor.save_error.is_none()
        && (!editor.draft.body.trim().is_empty() || !editor.draft.title.trim().is_empty())
    {
        composer = composer.child(div().text_xs().text_color(rgb(theme().text_muted)).child(
            if editor.saving {
                Msg::IssueDraftSaving.t()
            } else {
                Msg::IssueDraftSaved.t()
            },
        ));
    }
    super::e2e::measure_control(id, composer).into_any_element()
}
