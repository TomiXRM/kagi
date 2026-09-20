//! Session-owned source editors. Window-bearing render only creates inputs;
//! subscriptions persist edits through the existing draft storage boundary.
use super::{
    i18n::Msg,
    theme::{self, theme},
    KagiApp,
};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Entity, SharedString, Window};
use gpui_component::input::{Enter, Input, InputEvent, InputState, Paste};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    text::TextView,
    Disableable, Icon, Sizable,
};
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
    pub body_revealed: bool,
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
                        .line_number(false)
                        .placeholder(if number.is_some() {
                            Msg::PrCommentPlaceholder.t()
                        } else {
                            Msg::IssueCompose.t()
                        })
                });
                let title = number.is_none().then(|| {
                    cx.new(|cx| InputState::new(window, cx).placeholder(Msg::IssueComposeEmpty.t()))
                });
                body.update(cx, |st, cx| {
                    st.set_value(editor.draft.body.clone(), window, cx)
                });
                if let Some(title) = &title {
                    title.update(cx, |st, cx| {
                        st.set_value(editor.draft.title.clone(), window, cx)
                    });
                }
                let mut inputs = vec![(body.clone(), false)];
                inputs.extend(title.iter().cloned().map(|input| (input, true)));
                for (input, is_title) in inputs {
                    let repo = repo.clone();
                    cx.subscribe_in(&input, window, move |app, _, event, window, cx| {
                        if number.is_none()
                            && is_title
                            && matches!(
                                event,
                                InputEvent::PressEnter {
                                    secondary: false,
                                    shift: false,
                                }
                            )
                        {
                            // A single-line Input deliberately propagates Enter before
                            // emitting PressEnter. Consume that action before moving focus,
                            // or the same key can reach the newly focused body editor and
                            // insert a newline there.
                            cx.stop_propagation();
                            let body = app
                                .ui
                                .get_mut(&owner)
                                .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                                .and_then(|editor| {
                                    editor.body_revealed = true;
                                    editor.body_input.clone()
                                });
                            if let Some(body) = body {
                                body.update(cx, |state, cx| state.focus(window, cx));
                            }
                            cx.notify();
                            return;
                        }
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
    let disabled = !editor.loaded
        || editor.draft.body.trim().is_empty()
        || state.base_repo.is_none()
        || held
        || app.op_latched();
    // The empty mock is a single prompt. Reuse the existing title entity for
    // it: its first Change naturally expands the body, while Enter explicitly
    // reveals and focuses the existing body entity without putting a newline in
    // the title.
    let empty_create = number.is_none()
        && !editor.focused
        && !editor.preview
        && !editor.body_revealed
        && editor.draft.title.trim().is_empty()
        && editor.draft.body.trim().is_empty();
    let viewer = app.github_login.as_deref().unwrap_or("?");
    let avatar = kagi_ui_core::commit_header::avatar_circle_with_initials(
        40.,
        viewer,
        viewer,
        &app.avatars.images,
    );
    let mut composer = div()
        .id(id)
        .key_context("IssueComposer")
        .w_full()
        .flex()
        .flex_row()
        .gap(theme::scaled_px(14.))
        .px(theme::scaled_px(24.))
        .pt(theme::scaled_px(20.))
        .pb(theme::scaled_px(14.))
        .border_b_1()
        .border_color(rgb(theme().selected))
        .on_action(cx.listener(move |app, _: &FocusIssueEditor, window, cx| {
            app.toggle_issue_focus(number, window, cx)
        }));
    let repo = state
        .base_repo
        .clone()
        .unwrap_or_else(|| Msg::IssueRepoUnavailable.t().into());
    let mut content = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(theme::scaled_px(10.))
        .child(
            div()
                .self_start()
                .h(theme::scaled_px(28.))
                .px(theme::scaled_px(10.))
                .flex()
                .items_center()
                .rounded_full()
                .border_1()
                .border_color(rgb(theme().selected))
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .gap_1()
                .child(Icon::empty().path("icons/folder-open.svg").xsmall())
                .child(repo),
        );
    if number.is_none() {
        let Some(title) = editor.title_input.clone() else {
            return div().into_any_element();
        };
        content = content.child(
            div()
                .h(theme::scaled_px(32.))
                .on_action(cx.listener(|_, action: &Enter, _, cx| {
                    if action.secondary || action.shift {
                        cx.propagate();
                    } else {
                        // InputState emits PressEnter for its subscription, but
                        // its single-line handler also asks the action to bubble.
                        // Stop that plain Enter at the title boundary.
                        cx.stop_propagation();
                    }
                }))
                .child(
                    Input::new(&title)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .h_full()
                        .text_size(theme::scaled_px(20.))
                        .font_weight(gpui::FontWeight::BOLD),
                ),
        );
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
            .child(
                Input::new(&input)
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .h_full()
                    .text_size(theme::scaled_px(15.))
                    .line_height(theme::scaled_px(25.5)),
            )
            .into_any_element()
    };
    if !empty_create {
        content = content.child(body);
    }
    let (mode_icon, mode_tooltip) = if editor.preview {
        ("icons/square-pen.svg", Msg::IssueWrite.t())
    } else {
        ("icons/eye.svg", Msg::IssuePreview.t())
    };
    let submit = Button::new(SharedString::from(format!("{id}-submit")))
        .icon(Icon::empty().path("icons/comment-send.svg"))
        .label(if number.is_some() {
            Msg::IssueReply.t()
        } else {
            Msg::IssueCreate.t()
        });
    // The standard warning variant is filled from Kagi's synchronized warning
    // button tokens. Keep the unavailable state on Button's neutral disabled
    // path instead of leaving an amber tint that looks actionable.
    let submit = if disabled { submit } else { submit.warning() }
        .rounded(px(999.))
        .h(theme::scaled_px(36.))
        .px_3()
        .disabled(disabled)
        .on_click(cx.listener(move |app, _, _, cx| app.start_issue_write(number, cx)));
    content = content.child(
        div()
            .flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(super::e2e::measure_control(
                        if number.is_some() {
                            "issue-reply-mode-toggle"
                        } else {
                            "issue-composer-mode-toggle"
                        },
                        Button::new(SharedString::from(format!("{id}-mode-toggle")))
                            .icon(Icon::empty().path(mode_icon))
                            .small()
                            .tooltip(mode_tooltip)
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if let Some(editor) = app
                                    .ui_mut()
                                    .and_then(|ui| ui.issue_composer.editors.get_mut(&number))
                                {
                                    editor.preview = !editor.preview;
                                }
                                cx.notify();
                            })),
                    )),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new(SharedString::from(format!("{id}-focus")))
                            .icon(Icon::empty().path(if editor.focused {
                                "icons/window-restore.svg"
                            } else {
                                "icons/window-maximize.svg"
                            }))
                            .small()
                            .tooltip_with_action(
                                if editor.focused {
                                    Msg::IssueExitFocus.t()
                                } else {
                                    Msg::IssueFocusEditor.t()
                                },
                                &FocusIssueEditor,
                                Some("IssueComposer"),
                            )
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.toggle_issue_focus(number, window, cx)
                            })),
                    )
                    .child(submit),
            ),
    );
    if let Some(error) = editor.save_error.as_ref() {
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(theme().color_blocker))
                .child(error.clone()),
        );
    }
    if let Some(error) = state.repo_error.as_ref() {
        content = content.child(
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
        content = content.child(div().text_xs().text_color(rgb(theme().text_muted)).child(
            if editor.saving {
                Msg::IssueDraftSaving.t()
            } else {
                Msg::IssueDraftSaved.t()
            },
        ));
    }
    composer = composer.child(avatar).child(content);
    super::e2e::measure_control(id, composer).into_any_element()
}
