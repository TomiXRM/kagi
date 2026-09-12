//! The pull workflow's completion half (ADR-0196 Wave 3).
//!
//! Split from `pull_push.rs` on the lifecycle boundary: that file plans and
//! confirms, this one admits the write, settles the completion and presents the
//! receipt the workflow settled on. Pull is the one family whose job runs up to
//! three recorded children, so "present the result" is not one receipt — the
//! siblings are panel rows and the decisive one is announced.
use crate::ui::blocking_ops::pull_blocking;
use crate::ui::*;

impl KagiApp {
    /// ADR-0196 Wave 3: run the confirmed pull workflow through the
    /// application layer.
    ///
    /// Pull is three writes wearing one confirmation, so the blocking core owns
    /// every receipt it produced and names the one that decided the workflow —
    /// which is not always the last one run. This presents that decisive
    /// receipt and applies the terminal's footer / modal policy; it never
    /// synthesizes an entry of its own. Returns `false` when admission refused
    /// (the refusal is already presented).
    pub(super) fn finish_pull(
        &mut self,
        cx: &mut Context<Self>,
        modal: PullPlanModal,
        repo_path: PathBuf,
    ) -> bool {
        use crate::app::{self, Delivery, FamilyEvidence, LegacyBusy, PullPresentation};
        self.refresh_write_busy();
        let plan = modal.plan.clone();
        let (auto_stash, promised_dirty) = (modal.auto_stash, modal.dirty_digest);
        let (bg_path, bg_plan) = (repo_path.clone(), plan.clone());
        let admitted = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .ok_or(app::AdmissionError::StaleApproval)
            .and_then(|owner| {
                let repo = kagi_git::Backend::open(&repo_path)
                    .and_then(|backend| backend.write_repo_id())
                    .map_err(|error| app::AdmissionError::Identity(error.to_string()))?;
                app::approve_pull(
                    &mut self.app_sessions,
                    app::PullRequest {
                        owner,
                        name: "pull",
                        path: repo_path.clone(),
                        repo,
                        plan: plan.clone(),
                        auto_stash,
                        promised_dirty,
                    },
                )
            })
            .and_then(|approved| {
                app::prepare_pull(
                    &mut self.app_sessions,
                    approved,
                    LegacyBusy(self.busy_op.is_some()),
                    Box::new(move || pull_blocking(&bg_path, &bg_plan, auto_stash, promised_dirty)),
                )
            });
        let job = match app::admit(&mut self.reads, admitted) {
            Ok(job) => job,
            Err(error) => {
                self.report_admission_refusal(error, cx);
                return false;
            }
        };
        self.mark_write_busy("pull");
        let stamp = job.stamp();
        // #289: gpui does not propagate a background panic, so the task can end
        // without a completion. That is not evidence of termination — the write
        // may have happened — so it settles as `Unknown` through the same path
        // instead of clearing the busy mirror under a lease that stays held.
        let abandonment = job.abandonment();
        let task = cx.background_spawn(async move { job.run() });
        cx.spawn(async move |this, acx| {
            let completion = task.fallible().await;
            let _ = this.update(acx, move |app, cx| {
                let completion = match completion {
                    Some(completion) => completion,
                    None => {
                        klog!("op panicked: pull — settled as unknown, lease retained");
                        abandonment.into_completion()
                    }
                };
                let deliveries = app::apply(&mut app.app_sessions, completion);
                app.refresh_write_busy();
                let (completed, rest): (Vec<_>, Vec<_>) = deliveries
                    .into_iter()
                    .partition(|d| matches!(d, Delivery::Completed { .. }));
                let mut failed = false;
                for delivery in completed {
                    let Delivery::Completed { id, report, .. } = delivery else {
                        continue;
                    };
                    let FamilyEvidence::Pull(report) = report.evidence else {
                        continue;
                    };
                    // Settle first, whatever the tab is doing now (#501). A parked
                    // reconcile requirement refuses every later write in this
                    // scope, and the notice is the only way into it — so it is
                    // settlement, not presentation, and it must survive the tab
                    // switch below exactly as the recording-failure notice does
                    // (#702 re-review).
                    for step in &report.steps {
                        app.notice_recording_failure("pull", &step.recording, &repo_path);
                    }
                    app.notice_reconcile_required(id, "pull", &repo_path);
                    let current = app.active_session() == Some(stamp.session)
                        && app.app_sessions.visit(stamp.session) == Some(stamp.visit);
                    if !current {
                        klog!("op result dropped: tab switched during op");
                        continue;
                    }
                    // The workflow's own line comes first, exactly as every
                    // other family emits it — the `footer:` line a receipt
                    // announces belongs after it (#702 Codex review).
                    match &report.terminal.presentation {
                        PullPresentation::Success { summary } => {
                            klog!("async: pull finished — {}", summary)
                        }
                        PullPresentation::Failed { error } => {
                            failed = true;
                            klog!("async: pull failed — {}", error)
                        }
                        PullPresentation::Partial { error } => {
                            klog!("async: pull partially applied — {}", error)
                        }
                    }
                    // Every child receipt belongs in the panel — they are all
                    // durable entries the backend wrote — but only one of them
                    // is the answer to what the user asked for. Walking the
                    // steps in execution order keeps the panel's newest-first
                    // order the same as the durable log's, and announcing
                    // exactly one of them (one toast, one `footer:` line, one
                    // auto-open) keeps a sibling's success from becoming the
                    // workflow's (#702 review P1 / Codex).
                    //
                    // #501: when a step's receipt never reached the oplog, that
                    // is the fact worth announcing — `entry_for_recording`
                    // renders it as "changed but not recorded" — so it takes the
                    // announcement from the decisive receipt rather than
                    // letting a clean one speak for a workflow that was not
                    // fully recorded.
                    let announce_at = report.announce();
                    for (at, step) in report.steps.iter().enumerate() {
                        if at == announce_at {
                            app.present_recorded(&step.recording, cx);
                        } else {
                            app.insert_recorded_row(&step.recording, cx);
                        }
                    }
                    let recording_failed = report.recording_failed();
                    match report.terminal.presentation {
                        PullPresentation::Success { summary } => {
                            // A mutation that happened but was not recorded is
                            // presented as "changed but not recorded" (#501),
                            // and that holds for a *child*'s receipt too: the
                            // success footer must not paper over any of them.
                            if !recording_failed {
                                app.status_footer = FooterStatus::Success(SharedString::from(
                                    format!("pull: {summary}"),
                                ));
                            }
                        }
                        // #493 / ADR-0189: the failure modal survives watcher
                        // reloads and remains until the user dismisses it. The
                        // footer under it is the decisive receipt's, never a
                        // trailing child's success.
                        PullPresentation::Failed { error }
                        | PullPresentation::Partial { error } => {
                            app.reopen_pull_modal(&plan, auto_stash, promised_dirty, error);
                        }
                    }
                    // One completion per admitted write.
                    break;
                }
                for delivery in rest {
                    match delivery {
                        // A failed write reopened its modal with the error; the
                        // reload sweep would clear it. Mark the reads stale and
                        // let the next reload pick them up.
                        Delivery::Invalidate(target) if failed => {
                            for session in app.app_sessions.sessions_for(&target.worktree) {
                                app.reads.invalidate(session);
                            }
                        }
                        other => app.deliver_app_result(other, cx),
                    }
                }
                app.present_app_notice();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    /// Put a receipt in the operation-log panel and nowhere else.
    ///
    /// [`KagiApp::present_recorded`] is *presentation*: it also toasts, moves
    /// the status footer, emits the `footer:` contract line and auto-opens the
    /// panel on a failure. A workflow with several children must do that once,
    /// from the receipt that decided it — the siblings are still durable
    /// receipts the panel should hold, but they are not the answer to what the
    /// user asked for (#702 review P1).
    fn insert_recorded_row(
        &mut self,
        recording: &kagi_git::backend::recording::Recording,
        cx: &mut Context<Self>,
    ) {
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        if let Some(panel) = self.op_log.clone() {
            panel.update(cx, |panel, cx| {
                panel.push(entry);
                panel.collapse();
                cx.notify();
            });
        }
    }

    /// Put the confirmation back with the workflow's own message on it.
    fn reopen_pull_modal(
        &mut self,
        plan: &std::sync::Arc<kagi_git::OperationPlan>,
        auto_stash: bool,
        dirty_digest: Option<kagi_domain::status::WorktreeDigest>,
        error: String,
    ) {
        self.set_pull_modal(PullPlanModal {
            plan: plan.clone(),
            auto_stash,
            error: Some(SharedString::from(error)),
            dirty_digest,
        });
    }
}
