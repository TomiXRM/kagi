//! Per-operation orchestration for `KagiApp`, split out of `ui/mod.rs`
//! (issue #13 Phase 4, P1). Each submodule holds the `open_/cancel_/replan_/
//! confirm_/start_` methods (plus async/finish helpers) for one family of Git
//! operations as additional `impl KagiApp` blocks. Pure physical split —
//! behaviour and signatures are unchanged.

pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod commit;
pub mod conflict;
pub mod discard;
pub mod editor_fs;
pub mod history;
pub mod modal_state;
pub mod pull_push;
pub mod remote_branch;
pub mod reset;
pub mod stash;
pub mod tag;
pub mod worktree;

use crate::ui::KagiApp;
use gpui::Context;

impl KagiApp {
    /// Run an already-spawned background `task` to completion, then apply the
    /// result on the main thread. This is the mechanical outer shell every
    /// `start_*` execute-op shares:
    ///
    /// `cx.spawn → task.await → this.update { busy_op = None; <per-op outcome>;
    /// cx.notify() } → detach → cx.notify()`.
    ///
    /// Only the spawn/join boilerplate is shared. The per-op outcome handling
    /// (`record_op`, `reload`, reopen-modal-on-error, `record_history`, …) stays
    /// in `on_done`, byte-identical and in the same order as before — so the
    /// `[kagi]`/`klog!` contract lines and the `plan → … → oplog` ordering are
    /// preserved. T-OPS-DEDUP-001.
    fn finish_op_on_main<R, F>(
        &mut self,
        cx: &mut Context<Self>,
        task: impl std::future::Future<Output = R> + 'static,
        on_done: F,
    ) where
        R: 'static,
        F: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    {
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, move |app, cx| {
                app.busy_op = None;
                on_done(app, result, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
