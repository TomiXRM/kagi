//! Per-operation orchestration for `KagiApp`, split out of `ui/mod.rs`
//! (issue #13 Phase 4, P1). Each submodule holds the `open_/cancel_/replan_/
//! confirm_/start_` methods (plus async/finish helpers) for one family of Git
//! operations as additional `impl KagiApp` blocks. Pure physical split —
//! behaviour and signatures are unchanged.

mod app_bridge;
pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod commit;
pub mod conflict;
mod conflict_skip;
pub mod discard;
pub mod editor_fs;
pub mod force_lease;
pub mod history;
pub mod merge;
pub mod modal_state;
pub mod pull_push;
pub mod rebase;
pub mod remote_branch;
pub mod reset;
mod staging_failure;
pub mod stash;
pub mod tag;
pub(crate) mod transport_hold;
pub mod worktree;

use crate::ui::i18n::Msg;
use crate::ui::types::FooterStatus;

/// A Pull confirmation that could not be delivered when its fetch finished,
/// waiting for the tab that asked for it (#625, ADR-0192).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PullConfirmDelivery {
    /// Plan and open the confirmation.
    Confirm,
    /// The pre-Pull fetch failed; show the notice (the oplog entry was already
    /// written when the fetch completed).
    FetchFailed(String),
}
use crate::ui::KagiApp;
use gpui::{Context, SharedString, Task};
use kagi_git::backend::recording::{Recording, RunReport};
use kagi_git::oplog::OpOutcome;
use kagi_git::StateSummary;
use std::path::{Path, PathBuf};

/// What to do with a finished background op, decided from the join result and
/// whether the op's owning tab is still the active one. Pure (no `KagiApp`,
/// no gpui) so both the stale-tab guard (#284) and the panic path (#289) can
/// be unit-tested without a live executor. `busy_op` is cleared by the caller
/// *unconditionally* — that release is not part of this decision.
#[derive(Debug, PartialEq, Eq)]
enum OpDisposition {
    /// Result is valid and still belongs to the active tab — run `on_done`.
    Apply,
    /// Result is valid but the repo/tab changed while the op ran — drop it so
    /// tab A's result can't land in tab B (#284).
    DropStale,
    /// The background future panicked — `Task::fallible` yielded `None` (#289).
    Panicked,
}

/// Does an op result still apply to the currently-active tab? False if either
/// the repo path or the tab-switch generation moved while the op was in flight
/// (the result belongs to a tab the user has since left). Mirrors the sibling
/// async guards in `reload.rs` / `mod.rs` — using both signals is strictly
/// safer than either alone.
/// Whether a state-changing op may start right now. `busy_op` is the single
/// in-flight-op latch; a mutation started while another is running is exactly
/// the concurrent-mutation hazard #283 is about, so every entry point that
/// begins one consults this. Pure so the gate is testable without a Context.
pub(crate) fn op_may_start(busy_op: Option<&'static str>) -> bool {
    busy_op.is_none()
}

fn op_result_applies(
    current_repo: Option<&Path>,
    current_gen: u64,
    owner_repo: Option<&Path>,
    owner_gen: u64,
) -> bool {
    current_gen == owner_gen && current_repo == owner_repo
}

fn classify_op_result(join_ok: bool, still_current: bool) -> OpDisposition {
    match (join_ok, still_current) {
        (false, _) => OpDisposition::Panicked,
        (true, true) => OpDisposition::Apply,
        (true, false) => OpDisposition::DropStale,
    }
}

impl KagiApp {
    /// Run an already-spawned background `task` to completion, then apply the
    /// result on the main thread. This is the mechanical outer shell every
    /// `start_*` execute-op shares:
    ///
    /// `cx.spawn → task.fallible().await → this.update { busy_op = None;
    /// <per-op outcome>; cx.notify() } → detach → cx.notify()`.
    ///
    /// Only the spawn/join boilerplate is shared. The per-op outcome handling
    /// (`record_op`, `reload`, reopen-modal-on-error, `record_history`, …) stays
    /// in `on_done`, byte-identical and in the same order as before — so the
    /// `[kagi]`/`klog!` contract lines and the `plan → … → oplog` ordering are
    /// preserved. T-OPS-DEDUP-001.
    ///
    /// Two safety guards live here so every op inherits them:
    /// * **#284 (stale-tab guard):** the owning tab's `repo_path` +
    ///   `switch_generation` are captured at spawn time and re-checked on
    ///   arrival; a result for a tab the user has left is dropped instead of
    ///   being applied to whatever tab is now active.
    /// * **#289 (guaranteed `busy_op` release):** gpui spawns background futures
    ///   with `propagate_panic = false`, so a panicking op closes its task and a
    ///   plain `task.await` would itself panic ("Task polled after completion"),
    ///   unwinding this closure *before* `busy_op` is cleared and wedging every
    ///   future git op. `Task::fallible().await` yields `None` on panic instead,
    ///   keeping the update closure reachable — and `busy_op` is cleared
    ///   unconditionally at its top, so no outcome (stale, panic, or success)
    ///   can leave it stuck.
    /// Reject a state-changing op if another is in flight (#283 stage 1).
    /// Returns true (and sets the footer) when the caller must bail out.
    pub(crate) fn reject_if_busy(&mut self, cx: &mut Context<Self>) -> bool {
        self.refresh_write_busy();
        if op_may_start(self.busy_op) && !self.app_sessions.has_leases() {
            return false;
        }
        self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
        cx.notify();
        true
    }

    pub(crate) fn finish_op_on_main<R, F>(
        &mut self,
        cx: &mut Context<Self>,
        task: Task<R>,
        on_done: F,
    ) where
        R: 'static,
        F: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    {
        self.finish_op_on_main_settled(cx, task, |_, _, _| {}, on_done);
    }

    /// Like [`finish_op_on_main`], with a `settle` half that runs on **every**
    /// arrival — before the stale-tab guard. Anything the execution boundary
    /// already made durable (a receipt, or the notice that appending it failed)
    /// belongs here: DESIGN §4 "operation の終端化は常に先". `on_done` remains
    /// the presentation half and is still dropped when the tab moved (#501).
    pub(crate) fn finish_op_on_main_settled<R, S, F>(
        &mut self,
        cx: &mut Context<Self>,
        task: Task<R>,
        settle: S,
        on_done: F,
    ) where
        R: 'static,
        S: FnOnce(&mut Self, &R, &mut Context<Self>) + 'static,
        F: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    {
        let owner_repo = self.repo_path.clone();
        let owner_gen = self.switch_generation;
        let op_tag = self.busy_op;
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            let _ = this.update(acx, move |app, cx| {
                // Unconditional release (#289): whatever happened to the op,
                // the global op mutex must not stay latched.
                app.busy_op = None;
                app.write_busy_op = None;
                // Settle first, whatever the tab is doing now (#501).
                if let Some(result) = result.as_ref() {
                    settle(app, result, cx);
                }
                let still_current = op_result_applies(
                    app.repo_path.as_deref(),
                    app.switch_generation,
                    owner_repo.as_deref(),
                    owner_gen,
                );
                match classify_op_result(result.is_some(), still_current) {
                    OpDisposition::Apply => {
                        on_done(app, result.expect("Apply implies Some"), cx);
                    }
                    OpDisposition::DropStale => {
                        klog!("op result dropped: tab switched during op");
                    }
                    OpDisposition::Panicked => {
                        let tag = op_tag.unwrap_or("op");
                        klog!("op panicked: {} — busy_op cleared", tag);
                        // ponytail: raw string, not a new i18n Msg — this is an
                        // edge-case recovery footer, matching the raw-format!
                        // footers already used across the op modules.
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "{tag}: operation failed unexpectedly"
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// ADR-0196 Wave 2: settle and present one family's own receipt.
    ///
    /// The settle half notices a failed append on every arrival, so a tab
    /// switch cannot swallow "changed but not recorded". The presentation half
    /// presents the recorded entry — never one re-synthesized from the error
    /// text — and hands `on_done` the localized failure, `None` on success.
    /// An `Err` from the task means the repository would not open: nothing ran
    /// and nothing was recorded, so that one case is still recorded here.
    ///
    /// The `async: <op> finished|failed` contract line is logged here, before
    /// presentation, so its position relative to the footer line is unchanged;
    /// `finished_note` is the family's historical ` — <summary>` suffix.
    pub(crate) fn finish_recorded<F>(
        &mut self,
        cx: &mut Context<Self>,
        task: Task<Result<RunReport, String>>,
        op_name: &'static str,
        op: crate::ui::i18n::Op,
        before: StateSummary,
        repo_path: PathBuf,
        finished_note: Option<String>,
        on_done: F,
    ) where
        F: FnOnce(&mut Self, Option<String>, &mut Context<Self>) + 'static,
    {
        let notice_path = repo_path.clone();
        self.finish_op_on_main_settled(
            cx,
            task,
            move |app, result, _cx| {
                if let Ok(report) = result {
                    app.notice_recording_failure(op_name, &report.recording, &notice_path);
                }
            },
            move |app, result, cx| match result {
                Ok(report) => {
                    let failed = report
                        .result
                        .as_ref()
                        .err()
                        .map(|e| crate::ui::i18n::op_failed(op, e));
                    match &failed {
                        None => klog!(
                            "async: {} finished{}",
                            op_name,
                            finished_note.as_deref().unwrap_or("")
                        ),
                        Some(err_msg) => klog!("async: {} failed — {}", op_name, err_msg),
                    }
                    app.present_recorded(&report.recording, cx);
                    on_done(app, failed, cx);
                }
                Err(err_msg) => {
                    klog!("async: {} failed — {}", op_name, err_msg);
                    app.record_op(
                        op_name,
                        before,
                        OpOutcome::Failed {
                            error: err_msg.clone(),
                        },
                        &repo_path,
                        cx,
                    );
                    on_done(app, Some(err_msg), cx);
                }
            },
        );
    }

    /// Owner-named notice for a record the execution boundary could not append
    /// (#501). Call it from the settle half so a tab switch cannot swallow
    /// "changed but not recorded" — the same delivery the stash family uses.
    pub(crate) fn notice_recording_failure(
        &mut self,
        op: &str,
        recording: &Recording,
        repo: &Path,
    ) {
        if let Recording::Failed { error, .. } = recording {
            self.app_notices
                .push_back(format!("{}: {op}: recording failed: {error}", repo.display()).into());
        }
    }

    /// Present an outcome the execution boundary already recorded. When the
    /// append failed there is no durable trace at all, so a mutation that DID
    /// happen is presented as "changed but not recorded" — never a clean
    /// success toast (#501). A failure that changed nothing stands as it is.
    pub(crate) fn present_recorded(&mut self, recording: &Recording, cx: &mut Context<Self>) {
        // Presentation consumes the attempted receipt, never reconstructs its
        // structured recovery/owner metadata and never retries persistence.
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        self.record_op_impl(entry, cx, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn op_may_start_only_when_no_op_is_in_flight() {
        // #283: the single in-flight-op latch is the concurrent-mutation gate.
        assert!(op_may_start(None), "idle must allow a new op");
        assert!(
            !op_may_start(Some("merge")),
            "an op in flight must block a new one"
        );
    }

    #[test]
    fn applies_when_repo_and_generation_match() {
        let repo = PathBuf::from("/repo/a");
        assert!(op_result_applies(Some(&repo), 7, Some(&repo), 7));
    }

    #[test]
    fn stale_when_generation_advanced() {
        // Same repo, but the user switched tabs (generation bumped) — the
        // result belongs to the tab they left (#284).
        let repo = PathBuf::from("/repo/a");
        assert!(!op_result_applies(Some(&repo), 8, Some(&repo), 7));
    }

    #[test]
    fn stale_when_repo_changed() {
        let now = PathBuf::from("/repo/b");
        let owner = PathBuf::from("/repo/a");
        assert!(!op_result_applies(Some(&now), 7, Some(&owner), 7));
    }

    #[test]
    fn pr_merge_is_blocked_while_another_op_is_in_flight() {
        // #402: a PR merge is a write op — with any op latched (its own tag or
        // a local git op) `reject_if_busy` must refuse it, same as every other
        // start_*. Guards the regression where start_pr_merge never read
        // busy_op.
        assert!(
            !op_may_start(Some("pr-merge")),
            "a pr-merge in flight must block a new op"
        );
        assert!(
            !op_may_start(Some("checkout")),
            "a local op in flight must block a pr-merge"
        );
    }

    #[test]
    fn pr_merge_result_dropped_when_tab_switched() {
        // #402: switching to another repo tab mid-merge must drop the result
        // so pr_mode_close_tab_for / fetch_async can't land on the wrong repo.
        let owner = PathBuf::from("/repo/pr");
        let now = PathBuf::from("/repo/other");
        assert!(
            !op_result_applies(Some(&now), 3, Some(&owner), 3),
            "merge result must not apply after the repo tab changed"
        );
        assert!(
            !op_result_applies(Some(&owner), 4, Some(&owner), 3),
            "merge result must not apply after the tab-switch generation moved"
        );
    }

    #[test]
    fn classify_covers_all_three_dispositions() {
        // Success on the owning tab → apply.
        assert_eq!(classify_op_result(true, true), OpDisposition::Apply);
        // Success but tab moved → drop (#284).
        assert_eq!(classify_op_result(true, false), OpDisposition::DropStale);
        // Panic (fallible None) → recorded as panic, regardless of tab (#289).
        assert_eq!(classify_op_result(false, true), OpDisposition::Panicked);
        assert_eq!(classify_op_result(false, false), OpDisposition::Panicked);
    }
}
