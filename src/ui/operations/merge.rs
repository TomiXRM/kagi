//! Merge planning, drag destinations, and confirmed execution.

use crate::ui::blocking_ops::*;
use crate::ui::*;

/// Translate a remote-tracking drop target to the local branch name which
/// `plan_merge_into_branch` would operate on. The exact local name wins, just
/// as it does in the backend, because a branch named `origin/feature` is valid.
fn worktree_target_name(target: &str, branches: &[(String, bool)], remotes: &[String]) -> String {
    if branches.iter().any(|(branch, _)| branch == target) {
        return target.to_string();
    }
    if remotes.iter().any(|remote| remote == target) {
        return default_tracking_branch_name(target);
    }
    target.to_string()
}

impl KagiApp {
    pub fn open_merge_modal(
        &mut self,
        target: String,
        expected_branch: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.reject_if_busy(cx) {
            return;
        }
        self.clear_merge_modal();
        let Some(owner) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .filter(|owner| owner.worktree.is_some())
        else {
            return;
        };
        self.planning = Some("merge-plan");
        self.status_footer = FooterStatus::Busy(SharedString::from("Planning merge…"));
        klog!("async: merge plan started for {}", target);
        let bg_owner = owner.clone();
        let bg_target = target.clone();
        let task = cx.background_spawn(async move {
            let repo = open_merge_backend(&bg_owner)?;
            let (plan, kind) = repo
                .plan_merge_branch(&bg_target)
                .map_err(|e| e.to_string())?;
            // The destination may still be loading its first read. Its plan,
            // not the active view's cached branch list, identifies its HEAD.
            let into_branch = match &plan.title {
                kagi_git::ops::PlanTitle::Merge(kagi_domain::plan_note::MergeTitle::Into {
                    current: Some(branch),
                    ..
                }) => branch.clone(),
                _ => "HEAD".to_string(),
            };
            if expected_branch
                .as_ref()
                .is_some_and(|expected| expected != &into_branch)
            {
                return Err(Msg::MergeDestinationChanged.t().to_string());
            }
            Ok((plan, kind, into_branch))
        });
        self.finish_op_on_main(cx, task, move |app, result, _cx| {
            if app
                .active_session()
                .and_then(|id| app.app_sessions.attachment(id))
                .as_ref()
                != Some(&owner)
            {
                return;
            }
            match result {
                Ok((plan, kind, into_branch)) => {
                    klog!(
                        "plan: merge {} blockers={} warnings={} preview_files={} kind={:?}",
                        target,
                        plan.blockers.len(),
                        plan.warnings.len(),
                        plan.preview_files.len(),
                        kind
                    );
                    app.status_footer = FooterStatus::Idle(SharedString::from(""));
                    app.set_merge_modal(MergePlanModal {
                        owner,
                        target,
                        into_branch,
                        plan: std::sync::Arc::new(plan),
                        kind,
                        off_branch: false,
                        error: None,
                    });
                }
                Err(e) => app.report_plan_failure(i18n::Op::Merge, e),
            }
        });
    }

    pub fn cancel_merge_modal(&mut self) {
        self.clear_merge_modal();
    }

    /// T-DNDMERGE-001 / ADR-0079 layer 2: the single entry point a branch
    /// drag-and-drop dispatches to.  `source` is the dragged branch (the merge
    /// source = the branch merged INTO HEAD) — a local branch name, or a
    /// remote-tracking ref like `origin/feature` for an upstream-only branch,
    /// which the planner resolves directly (no local branch is created).  This
    /// validates the obvious rejections (busy / not a branch / dropping the
    /// current branch onto itself) and, on success, delegates to the merge
    /// pipeline via
    /// [`open_merge_modal`] — it never touches git directly (the safety
    /// thesis: drop is a trigger; `plan_merge_branch` remains authoritative for
    /// dirty-WT / ff / conflict prediction).
    pub fn start_merge_from_drag(&mut self, source: String, cx: &mut Context<Self>) {
        let remotes: Vec<String> = self
            .view()
            .remote_branches
            .iter()
            .map(|rb| format!("{}/{}", rb.remote, rb.name))
            .collect();
        match validate_merge_from_drag(&source, &self.view().branches, &remotes, self.op_latched())
        {
            Ok(()) => {
                klog!("drag-merge: start merge from drag — source={}", source);
                self.open_merge_modal(source, None, cx);
            }
            Err(reason) => {
                klog!("drag-merge: rejected — {}", reason);
                // Footer alone was invisible (user report: "nothing happened");
                // a toast makes the refusal explicit without a modal round-trip.
                self.push_toast(ToastKind::Error, reason.clone(), cx);
                self.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
        }
        cx.notify();
    }

    /// Plan merging `source` into `target`, where `target` is a branch that is
    /// **not** checked out (ADR-0144). Reuses the merge modal: its confirm
    /// label already reads `Merge <source> into <target>`.
    pub fn open_merge_into_modal(
        &mut self,
        source: String,
        target: String,
        cx: &mut Context<Self>,
    ) {
        if self.reject_if_busy(cx) {
            return;
        }
        self.clear_merge_modal();
        let Some(owner) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .filter(|owner| owner.worktree.is_some())
        else {
            return;
        };
        self.planning = Some("merge-plan");
        self.status_footer = FooterStatus::Busy(SharedString::from("Planning merge…"));
        klog!(
            "async: merge-into plan started for {} -> {}",
            source,
            target
        );
        let bg_owner = owner.clone();
        let (bg_source, bg_target) = (source.clone(), target.clone());
        let task = cx.background_spawn(async move {
            let repo = open_merge_backend(&bg_owner)?;
            repo.plan_merge_into_branch(&bg_source, &bg_target)
                .map_err(|e| e.to_string())
        });
        self.finish_op_on_main(cx, task, move |app, result, _cx| {
            if app
                .active_session()
                .and_then(|id| app.app_sessions.attachment(id))
                .as_ref()
                != Some(&owner)
            {
                return;
            }
            match result {
                Ok((plan, _kind)) => {
                    klog!(
                        "plan: merge-into {} -> {} blockers={} warnings={}",
                        source,
                        target,
                        plan.blockers.len(),
                        plan.warnings.len()
                    );
                    app.status_footer = FooterStatus::Idle(SharedString::from(""));
                    app.set_merge_modal(MergePlanModal {
                        owner,
                        target: source,
                        into_branch: target,
                        plan: std::sync::Arc::new(plan),
                        kind: kagi_git::MergeKind::MergeCommit,
                        off_branch: true,
                        error: None,
                    });
                }
                Err(e) => app.report_plan_failure(i18n::Op::Merge, e),
            }
        });
    }

    /// Drop of `source` onto a branch row that is **not** the current branch
    /// (ADR-0144).
    pub fn start_merge_into_from_drag(
        &mut self,
        source: String,
        target: String,
        cx: &mut Context<Self>,
    ) {
        let remotes: Vec<String> = self
            .view()
            .remote_branches
            .iter()
            .map(|rb| format!("{}/{}", rb.remote, rb.name))
            .collect();
        match crate::ui::validate_merge_into_from_drag(
            &source,
            &target,
            &self.view().branches,
            &remotes,
            self.op_latched(),
        ) {
            Ok(()) => {
                klog!("drag-merge: into {} from {}", target, source);
                // The worktree inventory contains local branch names. Keep the
                // original remote ref for the off-branch planner, which may
                // create its local destination, but resolve it before finding
                // a worktree that already owns that destination.
                let worktree_target =
                    worktree_target_name(&target, &self.view().branches, &remotes);
                if let Some(path) = self
                    .view()
                    .worktrees
                    .iter()
                    .find(|worktree| {
                        !worktree.is_current
                            && worktree.branch.as_deref() == Some(worktree_target.as_str())
                    })
                    .map(|worktree| worktree.path.clone())
                {
                    self.open_merge_in_worktree(source, worktree_target, path, cx);
                } else {
                    self.open_merge_into_modal(source, target, cx);
                }
            }
            Err(reason) => {
                klog!("drag-merge: rejected — {}", reason);
                self.push_toast(ToastKind::Error, reason.clone(), cx);
                self.status_footer = FooterStatus::Idle(SharedString::from(reason));
                cx.notify();
            }
        }
    }

    pub(crate) fn open_merge_in_worktree(
        &mut self,
        source: String,
        target: String,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.reject_if_busy(cx) {
            return;
        }
        let Some(origin) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .filter(|owner| owner.worktree.is_some())
        else {
            return;
        };
        if self.editor_workspace_any_dirty(cx) {
            self.open_editor_dirty_guard(
                EditorPendingIntent::MergeInWorktree {
                    source,
                    target,
                    path,
                    owner: origin,
                },
                cx,
            );
            return;
        }
        if !self.open_repository(path, cx) {
            return;
        }
        let destination = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id));
        // A stale locator must never turn a drop into a merge in an unrelated
        // repository, or in the origin when navigation was refused.
        if destination.as_ref().is_none_or(|owner| {
            owner.session == origin.session
                || owner.worktree.as_ref().map(|id| &id.repo)
                    != origin.worktree.as_ref().map(|id| &id.repo)
        }) {
            self.report_plan_failure(i18n::Op::Merge, Msg::MergeDestinationChanged.t());
            cx.notify();
            return;
        }
        self.open_merge_modal(source, Some(target), cx);
    }

    pub fn start_merge(&mut self, cx: &mut Context<Self>) {
        if self.reject_if_busy(cx) {
            return;
        }
        let modal = match self.merge_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .as_ref()
            != Some(&modal.owner)
        {
            self.clear_merge_modal();
            cx.notify();
            return;
        }
        let repo_path = modal.owner.path.clone();
        if !modal.plan.blockers.is_empty() {
            klog!("refused: merge plan has blockers, not executing");
            self.record_op(
                "merge",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_merge_modal();
            cx.notify();
            return;
        }

        self.clear_merge_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyMerge.t()));
        klog!("async: merge started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let kind = modal.kind.clone();
        let bg_owner = modal.owner.clone();
        let history_target = modal.target.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the merge (main thread).
        let history_before = self.head_branch_and_sha();
        let off_branch = modal.off_branch;
        let bg_into = modal.into_branch.clone();
        let (note_source, note_into) = (modal.target.clone(), modal.into_branch.clone());
        self.finish_run(
            cx,
            "merge",
            i18n::Op::Merge,
            modal.plan.clone(),
            repo_path,
            move || {
                if off_branch {
                    crate::ui::blocking_ops::merge_into_branch_blocking(
                        &bg_owner, &plan, &target, &bg_into,
                    )
                } else {
                    merge_blocking(&bg_owner, &plan, &target, &kind)
                }
            },
            move |outcome| {
                Some(format!(
                    "finished — {}",
                    crate::ui::blocking_ops::merge_summary(
                        off_branch,
                        &note_source,
                        &note_into,
                        outcome
                    )
                ))
            },
            move |app, done, _cx| match done {
                Ok(_) => {
                    // Record for undo/redo only when the merge actually moved
                    // the branch ref (clean merge / fast-forward). A merge
                    // left in conflict has not moved HEAD, so before==after
                    // and record_history is a no-op.
                    if let (Some((branch, before)), Some((_, after_sha))) =
                        (history_before.clone(), app.head_branch_and_sha())
                    {
                        app.record_history(
                            kagi_git::OperationKind::Merge,
                            &branch,
                            before,
                            after_sha,
                            format!("merge {}", history_target),
                        );
                    }
                    // reload() resets the conflict-mode detection guard and
                    // re-runs detect_conflict_mode(); a merge that left
                    // conflict markers (MergeKind::Conflicts) therefore enters
                    // Conflict Mode here. Non-conflict merges stay Normal.
                }
                Err(failure) => {
                    app.set_merge_modal(MergePlanModal {
                        owner: modal.owner.clone(),
                        target: modal.target.clone(),
                        into_branch: modal.into_branch.clone(),
                        plan: modal.plan.clone(),
                        kind: modal.kind.clone(),
                        off_branch: modal.off_branch,
                        error: Some(SharedString::from(failure.message)),
                    });
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::worktree_target_name;

    #[test]
    fn remote_target_uses_its_checked_out_local_branch_name() {
        assert_eq!(
            worktree_target_name(
                "origin/feature",
                &[("feature".to_string(), false)],
                &["origin/feature".to_string()],
            ),
            "feature"
        );
    }

    #[test]
    fn exact_local_target_keeps_backend_local_precedence() {
        assert_eq!(
            worktree_target_name(
                "origin/feature",
                &[("origin/feature".to_string(), false)],
                &["origin/feature".to_string()],
            ),
            "origin/feature"
        );
    }
}
