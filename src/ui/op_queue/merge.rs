use super::*;

impl KagiApp {
    /// Freeze the destination name, never the enqueue-time merge prediction.
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
        into.is_some_and(|into| self.enqueue_intent(IntentRequest::Merge { source, into }, cx))
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
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            let _ = this.update(acx, |app, cx| {
                if app.planning == Some("merge-plan") {
                    app.planning = None;
                    app.status_footer = super::super::types::FooterStatus::Idle("".into());
                }
                app.op_queue.planning = false;
                match result {
                    Some(Ok(modal)) => {
                        klog!(
                            "queue: plan {} blockers={} warnings={}",
                            app.queued_label(id),
                            modal.plan.blockers.len(),
                            modal.plan.warnings.len()
                        );
                        app.op_queue.plans.insert(id, QueuedPlan::Merge(modal));
                        app.drive_queue(QueueEvent::PlanCompleted(id), cx);
                    }
                    Some(Err(error)) => {
                        klog!("queue: plan error {}: {}", app.queued_label(id), error);
                        app.report_plan_failure(i18n::Op::Merge, error);
                        app.drive_queue(QueueEvent::PlanError(id), cx);
                    }
                    None => {
                        app.report_plan_failure(i18n::Op::Merge, "merge plan failed unexpectedly");
                        app.drive_queue(QueueEvent::PlanError(id), cx);
                    }
                }
                app.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
            });
        })
        .detach();
    }
}
