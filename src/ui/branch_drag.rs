//! Read-only branch-drop identity and feedback (ADR-0144).
use gpui::{Context, CursorStyle, Div, InteractiveElement, Stateful, Styled};

use super::{theme::theme, BranchDrag, KagiApp};

impl KagiApp {
    /// Compare ref identities, not tip OIDs: distinct branches at the same commit
    /// are still distinct destinations whose merge plan may explain containment.
    pub(crate) fn merge_drag_target_valid(&self, source: &str, target: &str) -> bool {
        let view = self.view();
        let local = |name: &str| view.branches.iter().any(|(branch, _)| branch == name);
        let remote = |name: &str| {
            name.split_once('/').is_some_and(|(remote, branch)| {
                view.remote_branches
                    .iter()
                    .any(|item| item.remote == remote && item.name == branch)
            })
        };
        let target_local = local(target);
        let target_remote = !target_local && remote(target);
        let resolved =
            kagi_domain::refs::merge_into_target_name(target, target_local, target_remote);
        (local(source) || remote(source))
            && (target_local || target_remote)
            && source != target
            && source != resolved
    }
}

/// Every branch target shares the same predicate for highlight, cursor and drop
/// dispatch. No plan or repository read is started by drag-over.
pub(super) fn feedback(
    element: Stateful<Div>,
    target: String,
    cx: &mut Context<KagiApp>,
) -> Stateful<Div> {
    let entity = cx.entity();
    let drop_target = target.clone();
    let move_target = target;
    element
        .can_drop(move |payload, _, cx| {
            payload.downcast_ref::<BranchDrag>().is_some_and(|drag| {
                entity
                    .read(cx)
                    .merge_drag_target_valid(&drag.name, &drop_target)
            })
        })
        .drag_over::<BranchDrag>(|style, _, _, _| {
            style
                .bg(gpui::rgb(theme().ui_selected()))
                .border_color(gpui::rgb(theme().ui_accent()))
        })
        .on_drag_move::<BranchDrag>(cx.listener(
            move |this, event: &gpui::DragMoveEvent<BranchDrag>, window, cx| {
                if event.bounds.contains(&event.event.position) {
                    let cursor = if this.merge_drag_target_valid(&event.drag(cx).name, &move_target)
                    {
                        CursorStyle::OpenHand
                    } else {
                        CursorStyle::OperationNotAllowed
                    };
                    if cx.active_drag_cursor_style() != Some(cursor) {
                        cx.set_active_drag_cursor_style(cursor, window);
                    }
                }
            },
        ))
}
