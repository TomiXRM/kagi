//! Explicit deletion and directory/file approval of one immutable prepared plan.
use crate::{
    app,
    ui::{self, KagiApp},
};
use gpui::{div, prelude::*, Context, SharedString};
use kagi_domain::conflict_family::{ConflictPathChanges, ConflictRevision};
use ui::i18n::Msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionKind {
    Remove,
    KeepDirectory,
    KeepFile,
}

#[derive(Clone)]
pub struct ConflictResolutionModal {
    pub token: app::PlanToken,
    pub owner: app::Attachment,
    pub revision: ConflictRevision,
    pub intent_token: u64,
    pub changes: ConflictPathChanges,
    pub kind: ResolutionKind,
    pub path: std::path::PathBuf,
}

impl ConflictResolutionModal {
    pub fn title(&self) -> String {
        let (message, suffix) = match self.kind {
            ResolutionKind::Remove => (Msg::ConflictRemoveTitle, ""),
            ResolutionKind::KeepDirectory => (Msg::ConflictKeepDirectoryTitle, "/"),
            ResolutionKind::KeepFile => (Msg::ConflictKeepFileTitle, ""),
        };
        message
            .t()
            .replace("{path}", &format!("{}{suffix}", self.path.display()))
    }

    pub fn confirm_label(&self) -> &'static str {
        match self.kind {
            ResolutionKind::Remove => Msg::ConflictRemoveConfirm,
            ResolutionKind::KeepDirectory => Msg::ConflictKeepDirectoryConfirm,
            ResolutionKind::KeepFile => Msg::ConflictKeepFileConfirm,
        }
        .t()
    }
}

impl KagiApp {
    pub fn cancel_conflict_resolution(&mut self) {
        if self.conflict_resolution_modal().is_some() {
            self.clear_conflict_resolution_modal();
            self.app_sessions.invalidate_plan();
        }
    }

    pub fn confirm_conflict_resolution(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.conflict_resolution_modal().cloned() else {
            return;
        };
        self.flush_conflict_drafts(modal.owner.clone(), "resolution", cx, move |app, cx| {
            let same_modal = app.conflict_resolution_modal().is_some_and(|current| {
                current.intent_token == modal.intent_token
                    && current.owner == modal.owner
                    && current.revision == modal.revision
                    && current.token.revision == modal.token.revision
            });
            let same_pane = app.ui().conflict.as_ref().is_some_and(|pane| {
                let pane = pane.read(cx);
                pane.intent_token == modal.intent_token
                    && pane
                        .mode
                        .as_ref()
                        .is_some_and(|mode| mode.revision == modal.revision)
            });
            if !same_modal || !same_pane || !app.conflict_action_owner_on_screen(&modal.owner) {
                return;
            }
            let policy = ui::blocking_ops::execution_policy();
            match app::approve(
                &mut app.app_sessions,
                modal.token,
                app::Policy::Conflict(policy),
            ) {
                Ok(approved) => {
                    app.clear_conflict_resolution_modal();
                    app.dispatch_job(approved, cx);
                }
                Err(error) => {
                    app.cancel_conflict_resolution();
                    app.push_toast(ui::ToastKind::Error, error.to_string(), cx);
                }
            }
            cx.notify();
        });
    }
}

pub(crate) fn render(
    modal: ConflictResolutionModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use ui::button_style::{modal_button, modal_button_row, ModalButtonKind};
    use ui::dialog_a11y::{apply_dialog, dialog_a11y, ConfirmStage, DialogHandler};
    use ui::modal_shell::{
        modal_body, modal_card, modal_list_max_h, modal_path_text, MODAL_LIST_ROW_H, MODAL_W_MD,
    };
    let cancel_listener = cx.listener(|app, _: &(), window, cx| {
        app.cancel_conflict_resolution();
        if let Some(focus) = &app.root_focus {
            window.focus(focus, cx);
        }
        cx.notify();
    });
    let confirm_listener = cx.listener(|app, _: &(), window, cx| {
        app.confirm_conflict_resolution(cx);
        if let Some(focus) = &app.root_focus {
            window.focus(focus, cx);
        }
        cx.notify();
    });
    let cancel: DialogHandler = std::rc::Rc::new(move |w, cx| cancel_listener(&(), w, cx));
    let confirm: DialogHandler = std::rc::Rc::new(move |w, cx| confirm_listener(&(), w, cx));
    let title = modal.title();
    let confirm_label = modal.confirm_label();
    let destructive = !modal.changes.remove_worktree.is_empty();
    let spec = dialog_a11y(
        &title,
        Some(confirm_label),
        destructive,
        ConfirmStage::Single,
    );
    let card = apply_dialog(
        "conflict-resolution-card",
        modal_card(MODAL_W_MD).id("conflict-resolution-card"),
        spec,
        Some(confirm.clone()),
        cancel.clone(),
    )
    .child(ui::modal_renderers::render_modal_title_row(
        title.into(),
        None,
    ));
    let mut paths = div().flex_shrink_0().flex().flex_col();
    let mut count = 0;
    for (label, group) in [
        (Msg::ConflictRemoveIndex, &modal.changes.remove_index),
        (Msg::ConflictRemoveWorktree, &modal.changes.remove_worktree),
        (Msg::ConflictKeepPaths, &modal.changes.keep),
    ] {
        let visible = |path: &&std::path::PathBuf| {
            !modal
                .changes
                .moves
                .iter()
                .any(|(source, _)| source == *path)
        };
        let visible_count = group.iter().filter(visible).count();
        if visible_count == 0 {
            continue;
        }
        count += visible_count + 1;
        paths = paths.child(
            div()
                .flex_shrink_0()
                .h(ui::theme::scaled_px(MODAL_LIST_ROW_H))
                .text_sm()
                .child(label.t()),
        );
        for path in group.iter().filter(visible) {
            let suffix = if label == Msg::ConflictRemoveWorktree
                && modal.changes.remove_directories.contains(path)
            {
                "/"
            } else {
                ""
            };
            paths = paths.child(
                div()
                    .flex_shrink_0()
                    .h(ui::theme::scaled_px(MODAL_LIST_ROW_H))
                    .w_full()
                    .flex()
                    .items_center()
                    .child(modal_path_text(SharedString::from(format!(
                        "{}{suffix}",
                        path.display()
                    )))),
            );
        }
    }
    if !modal.changes.moves.is_empty() {
        count += modal.changes.moves.len() + 1;
        paths = paths.child(
            div()
                .flex_shrink_0()
                .h(ui::theme::scaled_px(MODAL_LIST_ROW_H))
                .text_sm()
                .child(Msg::ConflictMovePaths.t()),
        );
        for (source, destination) in &modal.changes.moves {
            paths = paths.child(
                div()
                    .flex_shrink_0()
                    .h(ui::theme::scaled_px(MODAL_LIST_ROW_H))
                    .w_full()
                    .flex()
                    .items_center()
                    .child(modal_path_text(SharedString::from(format!(
                        "{} → {}",
                        source.display(),
                        destination.display()
                    )))),
            );
        }
    }
    let body = modal_body()
        .child(
            div().text_sm().child(
                Msg::ConflictResolutionConsequence
                    .t()
                    .replace("{continue}", Msg::ConflictReviewContinue.t()),
            ),
        )
        .when(destructive, |body| {
            body.child(
                div()
                    .text_sm()
                    .child(Msg::ConflictWorkingFilesConsequence.t()),
            )
        })
        .child(ui::e2e::measure_leaf(
            "conflict-resolution-paths",
            div()
                .id("conflict-resolution-paths")
                .flex_shrink_0()
                .h(modal_list_max_h(count))
                .overflow_y_scroll()
                .child(paths),
        ));
    let buttons = modal_button_row()
        .child(ui::e2e::measure_control(
            "conflict-resolution-cancel",
            modal_button(
                "conflict-resolution-cancel",
                Msg::PlanCancel.t(),
                ModalButtonKind::Cancel,
                None,
                move |_, w, cx| cancel(w, cx),
                cx,
            ),
        ))
        .child(ui::e2e::measure_control(
            "conflict-resolution-confirm",
            modal_button(
                "conflict-resolution-confirm",
                confirm_label,
                if destructive {
                    ModalButtonKind::Destructive
                } else {
                    ModalButtonKind::Primary
                },
                None,
                move |_, w, cx| confirm(w, cx),
                cx,
            ),
        ));
    ui::modal_renderers::modal_overlay(card.child(body).child(buttons).occlude()).into_any_element()
}
