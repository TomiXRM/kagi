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
    /// receipt and applies the terminal's footer / notice policy; it never
    /// synthesizes an entry of its own. Returns `false` when admission refused
    /// (the refusal is already presented).
    pub(super) fn finish_pull(
        &mut self,
        cx: &mut Context<Self>,
        modal: PullPlanModal,
        repo_path: PathBuf,
    ) -> bool {
        use crate::app::{self, Delivery, FamilyEvidence, PullPresentation};
        let progress_identity = modal
            .progress_request
            .map(crate::ui::busy::BusyIdentity::Planning)
            .or_else(|| modal.fetch_owner.map(crate::ui::busy::BusyIdentity::pull));
        let progress_hidden = self.busy_request_hidden(progress_identity);
        self.refresh_write_busy();
        // The lease answers for every other writer; the UI latch is what a
        // planning task in flight is refused by (ADR-0196 Wave 3).
        let latched = self.op_latched();
        let plan = modal.plan.clone();
        let (auto_stash, promised_dirty) = (modal.auto_stash, modal.dirty_digest);
        let result_started = modal
            .fetch_owner
            .map(|(_, _, started)| started)
            .unwrap_or_else(|| cx.background_executor().now());
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
                if latched {
                    return Err(app::AdmissionError::Busy);
                }
                app::prepare_pull(
                    &mut self.app_sessions,
                    approved,
                    Box::new(move || pull_blocking(&bg_path, &bg_plan, auto_stash, promised_dirty)),
                )
            });
        let job = match app::admit(&mut self.reads, admitted) {
            Ok(job) => job,
            Err(error) => {
                self.report_write_admission_refusal("pull", &repo_path, error, cx);
                return false;
            }
        };
        self.mark_write_busy("pull", cx);
        self.continue_busy_request(progress_identity, progress_hidden);
        // Admission consumes the decision. Keep the card on refusal, but never
        // leave a runnable confirmation above an admitted background writer.
        self.clear_pull_modal();
        self.status_footer = FooterStatus::Busy(crate::ui::operations::display_text(
            SharedString::from(Msg::BusyPull.t()),
        ));
        klog!("async: pull started");
        cx.notify();
        let stamp = job.stamp();
        // Pull has a remote effect and no verify path: untracked (ADR-0204 決定 4).
        self.queue_untracked_write(Some(stamp.session), cx);
        // #289: gpui does not propagate a background panic, so the task can end
        // without a completion. That is not evidence of termination — the write
        // may have happened — so it settles as `Unknown` through the same path
        // instead of clearing the busy mirror under a lease that stays held.
        let abandonment = job.abandonment();
        #[cfg(feature = "gui-e2e")]
        let hold = crate::ui::op_queue::take_run_hold();
        let task = cx.background_spawn(async move {
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            job.run()
        });
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
                        app.notice_recording_failure("pull", &step.recording, &repo_path, None);
                    }
                    app.notice_reconcile_required(id, "pull", &repo_path);
                    let announce = app.operation_result_toast_needed(
                        cx.background_executor()
                            .now()
                            .saturating_duration_since(result_started)
                            >= std::time::Duration::from_secs(2),
                    );
                    let current = app.active_session() == Some(stamp.session)
                        && app.app_sessions.visit(stamp.session) == Some(stamp.visit);
                    if !current {
                        let announce_at = report.announce();
                        for (at, step) in report.steps.iter().enumerate() {
                            if at == announce_at {
                                app.present_pull_recorded(&report, true, true, cx);
                            } else {
                                app.insert_recorded_row(&step.recording, cx);
                            }
                        }
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
                            app.present_pull_recorded(&report, announce, false, cx);
                        } else {
                            app.insert_recorded_row(&step.recording, cx);
                        }
                    }
                    if let PullPresentation::Failed { error }
                    | PullPresentation::Partial { error } = &report.terminal.presentation
                    {
                        let decisive = &report.steps[announce_at].recording;
                        app.enqueue_run_outcome_notice(id, decisive, Some(error), None);
                    }
                    // One completion per admitted write.
                    break;
                }
                for delivery in rest {
                    match delivery {
                        // A failed write stays durable in Operation Log. Mark
                        // the reads stale and let the next reload pick them up.
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

    /// Pull remains the requested operation even when stash pop settles last.
    fn present_pull_recorded(
        &mut self,
        report: &crate::app::PullReport,
        announce: bool,
        background: bool,
        cx: &mut Context<Self>,
    ) {
        let recording = &report.steps[report.announce()].recording;
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        if report.recording_failed() {
            if background {
                self.insert_recorded_row(recording, cx);
            } else {
                self.record_op_result(
                    entry,
                    cx,
                    false,
                    None,
                    None,
                    announce,
                    true,
                    Some("pull"),
                    true,
                    None,
                    None,
                    None,
                    None,
                );
            }
            return;
        }
        let text = match &report.decisive().recording.entry().outcome {
            kagi_git::oplog::OpOutcome::Unknown { evidence, .. } => {
                i18n::op::user_text::operations::unknown_preview("pull", evidence)
            }
            _ => match &report.terminal.presentation {
                crate::app::PullPresentation::Success { summary } => {
                    i18n::op::user_text::operations::pull_summary(summary)
                }
                crate::app::PullPresentation::Failed { error } => {
                    i18n::op::user_text::operations::failure_preview("pull", error)
                }
                crate::app::PullPresentation::Partial { error } => {
                    i18n::op::user_text::operations::partial_preview("pull", error)
                }
            },
        };
        let kind = if matches!(
            report.terminal.presentation,
            crate::app::PullPresentation::Success { .. }
        ) {
            ToastKind::Success
        } else {
            ToastKind::Error
        };
        if background {
            let _ = self.push_operation_result(
                &entry,
                "pull",
                crate::ui::toast_stack::ToastSpec::operation(&entry, kind, text.into()),
                cx,
            );
            self.insert_recorded_row(recording, cx);
        } else {
            self.record_op_result(
                entry,
                cx,
                false,
                None,
                Some((kind, text.into())),
                announce,
                true,
                Some("pull"),
                false,
                None,
                None,
                None,
                None,
            );
        }
    }

    /// Put a receipt in the operation-log panel and nowhere else.
    ///
    /// [`KagiApp::present_recorded`] presents the result in a footer or toast
    /// and emits the `footer:` contract line. A workflow with several children
    /// must do that once, from the receipt that decided it. The siblings are
    /// durable receipts the panel should hold, but they are not the answer to what the
    /// user asked for (#702 review P1).
    pub(in crate::ui::operations) fn insert_recorded_row(
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

    /// Put a background owner's receipt in the window-global Operation Log and
    /// announce its result without changing the active tab's footer/panes.
    pub(in crate::ui::operations) fn present_recorded_background(
        &mut self,
        recording: &kagi_git::backend::recording::Recording,
        success_text: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        ) {
            self.insert_recorded_row(recording, cx);
            return;
        }
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        let success = matches!(entry.outcome, kagi_git::oplog::OpOutcome::Success { .. });
        let shown = success_text.filter(|_| success).unwrap_or_else(|| {
            crate::ui::operations::outcome_text(&entry.op, &entry.outcome).into()
        });
        let _ = self.push_operation_result(
            &entry,
            &entry.op,
            crate::ui::toast_stack::ToastSpec::operation(
                &entry,
                if success {
                    ToastKind::Success
                } else {
                    ToastKind::Error
                },
                shown,
            ),
            cx,
        );
        self.insert_recorded_row(recording, cx);
    }
}
