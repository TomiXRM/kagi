//! Feature-only report transport and observation for native worktree inspection.

use std::{cell::RefCell, path::Path};

use kagi_git::worktree_inspection::WorktreeInspection;

use super::super::{sidebar_worktree_row, KagiApp};

thread_local! {
    static REPORT: RefCell<Option<gpui::Task<WorktreeInspection>>> = const { RefCell::new(None) };
}

pub fn queue(task: gpui::Task<WorktreeInspection>) {
    REPORT.with(|slot| {
        assert!(
            slot.borrow_mut().replace(task).is_none(),
            "unconsumed worktree inspection"
        );
    });
}

pub(crate) fn take() -> Option<gpui::Task<WorktreeInspection>> {
    REPORT.with(|slot| slot.borrow_mut().take())
}

pub fn status(app: &KagiApp, path: &Path) -> (bool, Option<u64>, Option<String>) {
    sidebar_worktree_row::inspection_status(app, path)
}
