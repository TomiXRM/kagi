//! Editor Workspace tree context-menu modal renderers (T-WS-EDITOR-007):
//! the Rename/New File/New Folder name-input prompt, and the Delete
//! (Trash) confirm. Both mirror the shape of the existing simple modals
//! (`modal_renderers_create.rs` for the input+confirm card,
//! `modal_renderers_destructive.rs` for the danger-styled confirm) — neither
//! carries an `OperationPlan` (these are plain `std::fs` ops, not Git
//! writes, ADR-0120 §4 scoping).

use super::i18n::Msg;
use super::modal_renderers::modal_overlay;
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, FocusHandle, KeyDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::Sizable as _;

/// Render the Rename / New File / New Folder name-input prompt.
pub(crate) fn render_editor_fs_prompt_modal(
    modal: EditorFsPromptModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let (title, confirm_label): (&'static str, &'static str) = match modal.kind {
        EditorFsPromptKind::Rename => (
            Msg::EditorFsPromptRenameTitle.t(),
            Msg::EditorFsPromptRenameTitle.t(),
        ),
        EditorFsPromptKind::NewFile => (
            Msg::EditorFsPromptNewFileTitle.t(),
            Msg::EditorFsPromptCreateButton.t(),
        ),
        EditorFsPromptKind::NewDir => (
            Msg::EditorFsPromptNewFolderTitle.t(),
            Msg::EditorFsPromptCreateButton.t(),
        ),
    };
    let name_is_valid = !modal.input.trim().is_empty();

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_editor_fs_prompt();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_editor_fs_prompt(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let mut card = div()
        .w(theme::scaled_px(420.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(title)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from(Msg::EditorFsPromptNameLabel.t())),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("editor-fs-prompt-cancel")
            .label(Msg::EditorWorkspaceCancel.t())
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if name_is_valid {
        button_row = button_row.child(
            Button::new("editor-fs-prompt-confirm")
                .label(confirm_label)
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }
    card = card.child(button_row);

    // Real text input handles its own focus/keys; Escape bubbles up to this
    // wrapper and cancels (same convention as create-branch/create-worktree).
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_editor_fs_prompt();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let focusable_card = {
        let base = div().on_key_down(esc_cancel);
        if let Some(ref fh) = focus_handle {
            base.track_focus(fh).child(card)
        } else {
            base.child(card)
        }
    };

    modal_overlay(focusable_card)
}

/// Render the Delete (Trash) confirmation. No `OperationPlan`/text input —
/// single explicit confirm click (a Trash move is recoverable; see
/// `KagiApp::confirm_editor_delete`'s doc comment for why this skips
/// `DiscardModal`'s two-stage arm).
pub(crate) fn render_editor_delete_confirm_modal(
    modal: EditorDeleteConfirmModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let title = if modal.is_dir {
        Msg::EditorDeleteConfirmTitleFolder.t()
    } else {
        Msg::EditorDeleteConfirmTitleFile.t()
    };
    let name = modal
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| modal.path.to_string_lossy().into_owned());

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_editor_delete_confirm();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_editor_delete(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_editor_delete_confirm();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });

    let mut card = div()
        .w(theme::scaled_px(420.))
        .bg(rgb(current_theme().modal))
        .border_1()
        .border_color(rgb(current_theme().color_blocker))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(current_theme().color_blocker))
                .text_xl()
                .child(SharedString::from(title)),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_main))
                .overflow_hidden()
                .child(SharedString::from(name)),
        );

    if let Some(count) = modal.file_count {
        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .child(SharedString::from(
                    super::i18n::editor_delete_file_count_note(count, modal.truncated),
                )),
        );
    }

    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .child(SharedString::from(Msg::EditorDeleteConfirmTrashNote.t())),
    );

    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            Button::new("editor-delete-cancel")
                .label(Msg::EditorWorkspaceCancel.t())
                .ghost()
                .small()
                .on_click(cancel_handler),
        )
        .child(
            crate::ui::button_style::KagiButton::accent(
                "editor-delete-confirm",
                Msg::EditorDeleteConfirmButton.t(),
                current_theme().color_blocker,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    card = card.child(button_row);

    modal_overlay(card.occlude())
        .on_key_down(esc_cancel)
        .into_any_element()
}
