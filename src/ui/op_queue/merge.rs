use super::*;
use kagi_domain::plan_note::{CommonNote, PlanNote, PlanOp};

impl KagiApp {
    /// Freeze the destination name, never the enqueue-time merge prediction.
    /// A refusal is shown here: nothing may be running to explain it.
    pub(crate) fn enqueue_merge(
        &mut self,
        source: String,
        into: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let into = into.or_else(|| {
            self.ui().repo_session.as_ref().and_then(|session| {
                match session.backend().head_state().ok()? {
                    kagi_git::Head::Attached { branch, .. } => Some(branch),
                    _ => None,
                }
            })
        });
        let refusal = match into {
            None => {
                kagi_ui_core::i18n::plan_note_text(&PlanNote::Common(CommonNote::HeadDetached {
                    op: PlanOp::Merge,
                }))
            }
            Some(into) => match self.enqueue_intent(IntentRequest::Merge { source, into }, cx) {
                Ok(()) => return true,
                // `run_queue_effects` already reported the full queue.
                Err(Some(EnqueueError::CapacityRejected)) => return false,
                Err(_) => i18n::Msg::OpInProgress.t().to_string(),
            },
        };
        self.status_footer = super::super::types::FooterStatus::Idle(refusal.clone().into());
        self.push_toast(super::super::ToastKind::Error, refusal, cx);
        false
    }

    /// Record a blocked queued merge as refused, as the direct path does.
    pub(super) fn refuse_blocked_queued_merge(
        &mut self,
        modal: &MergePlanModal,
        cx: &mut Context<Self>,
    ) {
        klog!("refused: merge plan has blockers, not executing");
        self.record_refused(
            "merge",
            modal.plan.current.clone(),
            &modal.plan.blockers,
            &modal.owner.path,
            cx,
        );
    }

    pub(super) fn plan_queued_merge(
        &mut self,
        id: IntentId,
        intent: &crate::app::QueuedIntent,
        cx: &mut Context<Self>,
    ) {
        let IntentRequest::Merge { source, into } = &intent.request else {
            unreachable!("only merge intents use the merge planner")
        };
        let owner = self
            .app_sessions
            .attachment(intent.owner)
            .expect("checked owner");
        let (source, into) = (source.clone(), into.clone());
        self.planning = Some("merge-plan");
        // This is the queue's own plan job, not an external PlanSlotTaken.
        self.op_queue.planning = true;
        klog!("async: merge plan started for {}", source);
        let task = cx.background_spawn(async move {
            let repo = super::super::blocking_ops::open_merge_backend(&owner)?;
            let off_branch = repo.current_branch_name().as_deref() != Some(into.as_str());
            let (plan, kind) = if off_branch {
                let (plan, _) = repo
                    .plan_merge_into_branch(&source, &into)
                    .map_err(|error| error.to_string())?;
                (plan, kagi_git::MergeKind::MergeCommit)
            } else {
                repo.plan_merge_branch(&source)
                    .map_err(|error| error.to_string())?
            };
            Ok::<_, String>(MergePlanModal {
                owner,
                target: source,
                into_branch: into,
                plan: Arc::new(plan),
                kind,
                off_branch,
                error: None,
                queued: Some(id),
            })
        });
        #[cfg(feature = "gui-e2e")]
        let hold = PLAN_HOLD.with(|slot| slot.borrow_mut().take());
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            let _ = this.update(acx, |app, cx| {
                if app.planning == Some("merge-plan") {
                    app.planning = None;
                    app.status_footer = super::super::types::FooterStatus::Idle("".into());
                }
                app.op_queue.planning = false;
                app.finish_queued_merge_plan(id, result, cx);
                app.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
            });
        })
        .detach();
    }

    /// A departed or closed owner requeues (or drops) the head and invalidates
    /// its plan: the result then belongs to no one on screen.
    fn finish_queued_merge_plan(
        &mut self,
        id: IntentId,
        result: Option<Result<MergePlanModal, String>>,
        cx: &mut Context<Self>,
    ) {
        // A tab switch or close since the last tick is not yet known to the
        // queue; without this, the result would open on the tab now on screen.
        self.sync_queue(cx);
        let current = self.op_queue.queue.intent(id).is_some_and(|intent| {
            intent.state == IntentState::Planning
                && self.app_sessions.attachment(intent.owner).is_some()
        });
        if !current {
            klog!("queue: plan discarded {}", self.queued_label(id));
            return;
        }
        match result {
            Some(Ok(modal)) => {
                klog!(
                    "queue: plan {} blockers={} warnings={}",
                    self.queued_label(id),
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                self.op_queue.plans.insert(id, QueuedPlan::Merge(modal));
                self.drive_queue(QueueEvent::PlanCompleted(id), cx);
            }
            Some(Err(error)) => {
                klog!("queue: plan error {}: {}", self.queued_label(id), error);
                self.report_plan_failure(i18n::Op::Merge, error);
                self.drive_queue(QueueEvent::PlanError(id), cx);
            }
            None => {
                self.report_plan_failure(i18n::Op::Merge, "merge plan failed unexpectedly");
                self.drive_queue(QueueEvent::PlanError(id), cx);
            }
        }
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static PLAN_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next queued merge plan's result until `hold` completes.
    pub fn hold_next_queued_merge_plan_for_e2e(hold: gpui::Task<()>) {
        PLAN_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
}
