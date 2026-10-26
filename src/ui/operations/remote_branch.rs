//! Remote-branch operations: delete-remote-branch (branch-menu "Advanced /
//! Dangerous" group) and fetch-remote-branch (branch-menu "Sync" group).
//!
//! Delete-remote-branch: two-stage confirm (mirrors `operations/discard.rs`'s
//! `confirm_armed` pattern) — the first click only arms the button; the
//! second click runs the delete on a background thread and reloads.
//!
//! Fetch-remote-branch: no modal at all, mirroring the repo-level `Fetch`
//! command (`commands.rs::fetch_async`) — fetch is inherently safe (it only
//! updates a remote-tracking ref, never merges/moves the current branch), so
//! it fires directly from the menu click.

use super::RunPresentation;
use crate::ui::blocking_ops::*;
use crate::ui::*;

#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_BRANCH_FETCH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_PR_FETCH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn panic_next_branch_fetch_for_e2e() {
        PANIC_NEXT_BRANCH_FETCH.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn panic_next_pr_fetch_for_e2e() {
        PANIC_NEXT_PR_FETCH.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

struct LoadedPrLocal {
    base: CommitId,
    base_tip: CommitId,
    head: CommitId,
    commits: Vec<kagi_git::Commit>,
    files: Vec<kagi_git::FileStatus>,
    /// #351: each file's head-side blob, for its viewed mark.
    head_blobs: Vec<String>,
    diff: Option<crate::ui::diff_view::MainDiffView>,
}

impl KagiApp {
    /// Open the delete-remote-branch modal for `remote_branch` (e.g.
    /// `"origin/feature/x"`).
    pub fn open_delete_remote_branch_modal(&mut self, remote_branch: impl Into<String>) {
        let remote_branch = remote_branch.into();
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
                    SharedString::from("delete-remote-branch: repo session unavailable"),
                ));
                return;
            }
        };
        match repo.plan_delete_remote_branch(&remote_branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: delete-remote-branch {} blockers={}",
                    remote_branch,
                    plan.blockers.len()
                );
                self.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                    remote_branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
                    SharedString::from(format!("delete-remote-branch plan error: {}", e)),
                ));
            }
        }
    }

    pub fn cancel_delete_remote_branch_modal(&mut self) {
        self.clear_delete_remote_branch_modal();
    }

    /// Two-stage confirm (mirrors `start_discard`): the first click only
    /// arms the button; the second click runs the delete on a background
    /// thread (write latch) and reloads.
    pub fn start_delete_remote_branch(&mut self, cx: &mut Context<Self>) {
        let modal = match self.delete_remote_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };

        if !modal.confirm_armed {
            self.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                confirm_armed: true,
                ..modal
            });
            klog!("delete-remote-branch: armed (second confirm required — destructive)");
            cx.notify();
            return;
        }

        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: delete-remote-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_refused(
                "delete-remote-branch",
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_delete_remote_branch_modal();
            cx.notify();
            return;
        }

        self.clear_delete_remote_branch_modal();
        self.status_footer = FooterStatus::Busy(crate::ui::operations::display_text(
            SharedString::from(format!("Deleting remote branch '{}'…", modal.remote_branch)),
        ));
        klog!("async: delete-remote-branch started");

        let plan = modal.plan.clone();
        let remote_branch = modal.remote_branch.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_remote_branch = remote_branch.clone();
        self.finish_run(
            cx,
            "delete-remote-branch",
            i18n::Op::Delete,
            plan.clone(),
            repo_path,
            move || delete_remote_branch_blocking(&bg_path, &bg_plan, &bg_remote_branch),
            |_| None,
            move |done| match done {
                Ok(_) => RunPresentation::status(FooterStatus::Success(
                    i18n::op::user_text::operations::deleted_branch(&remote_branch, true).into(),
                )),
                Err(_) => RunPresentation::none(),
            },
        );
    }
}

impl KagiApp {
    /// Fetch a single remote branch's refspec (e.g. `"origin/feature/x"`).
    /// No modal, no plan — fetch never merges or moves the current branch, so
    /// it fires directly, mirroring the repo-level `Fetch` command.
    pub fn fetch_remote_branch_async(&mut self, remote_branch: String, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(lease) = self.reserve_write("fetch", &repo_path, cx) else {
            return;
        };
        let owner = self.active_session();
        let visit = owner.and_then(|owner| self.app_sessions.visit(owner));
        let abandonment = lease.abandonment();
        let supervision = abandonment.supervision();
        klog!("fetch-remote-branch: start {}", remote_branch);
        let bg_path = repo_path.clone();
        let bg_remote_branch = remote_branch.clone();
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_BRANCH_FETCH.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // TestDispatcher propagates an uncaught worker panic rather than
                // returning a failed task. Catch only the injected panic, then
                // deliver the same absent completion as Task::fallible.
                let _ = std::panic::catch_unwind(|| panic!("injected branch fetch task panic"));
                return None;
            }
            let result = crate::ui::blocking_ops::open_backend(&bg_path);
            let open_failed = result.is_err();
            let result = result.and_then(|backend| backend.fetch_remote_branch(&bg_remote_branch));
            lease.complete_git(&result);
            Some((result, open_failed))
        });
        cx.spawn(async move |this, acx| {
            let (result, open_failed) = task
                .fallible()
                .await
                .flatten()
                .unwrap_or_else(|| (Err(abandonment.into_unknown()), false));
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                let current = owner.is_some_and(|owner| {
                    app.active_session() == Some(owner) && app.app_sessions.visit(owner) == visit
                });
                let auth_summary = match &result {
                    Err(kagi_git::GitError::FetchAuthentication { remote, .. }) => {
                        Some(crate::ui::commands::fetch_auth_summary(remote))
                    }
                    _ => None,
                };
                let mut recording_failed = false;
                if let Err(error) = &result {
                    let outcome = if let kagi_git::GitError::TerminationUnknown(reason) = error {
                        kagi_git::oplog::OpOutcome::Unknown {
                            after: kagi_git::StateSummary {
                                head: "unknown".into(),
                                dirty: "unknown".into(),
                            },
                            evidence: reason.to_string(),
                        }
                    } else {
                        let detail = if let Some(summary) = &auth_summary {
                            summary.clone()
                        } else if open_failed {
                            i18n::op::op_failed_contract(i18n::Op::RepoOpen, error)
                        } else {
                            error.to_string()
                        };
                        kagi_git::oplog::OpOutcome::Failed { error: detail }
                    };
                    let recording = app.record_ref_fetch_failure(
                        "fetch-remote-branch",
                        owner,
                        visit,
                        kagi_git::StateSummary {
                            head: format!("remote branch {remote_branch}"),
                            dirty: "unchanged".into(),
                        },
                        outcome,
                        None,
                        auth_summary.as_ref().map(|_| error.to_string()),
                        &repo_path,
                        cx,
                    );
                    recording_failed = matches!(
                        recording,
                        kagi_git::backend::recording::Recording::Failed { .. }
                    );
                }
                for (id, op, path) in app.app_sessions.drain_unaccounted() {
                    app.notice_reconcile_required(id, op, &path);
                }
                app.present_app_notice();
                let result = result.map_err(|e| {
                    if open_failed {
                        i18n::op::op_failed_contract(i18n::Op::RepoOpen, e)
                    } else {
                        e.to_string()
                    }
                });
                match &result {
                    Ok(_) => klog!("fetch-remote-branch: ok {}", remote_branch),
                    Err(e) => klog!("fetch-remote-branch: failed {} — {}", remote_branch, e),
                }
                if !current {
                    cx.notify();
                    return;
                }
                if recording_failed {
                    app.status_footer = FooterStatus::Idle("".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(outcome) => {
                        if outcome.changed {
                            app.reload(cx);
                        }
                        app.status_footer =
                            FooterStatus::Success(crate::ui::operations::display_text(
                                SharedString::from(format!("Fetched {}", remote_branch)),
                            ));
                        app.push_toast(
                            ToastKind::Success,
                            format!("Fetched {}", remote_branch),
                            cx,
                        );
                    }
                    Err(e) => {
                        let msg = auth_summary
                            .unwrap_or_else(|| i18n::op::op_failed_contract(i18n::Op::Fetch, e));
                        app.status_footer = FooterStatus::Failed(
                            crate::ui::operations::display_text(SharedString::from(msg.clone())),
                        );
                        app.push_toast(ToastKind::Error, msg, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch local refs for an opened PR. Automatic list refreshes pass `silent`
    /// so repository contention is not presented as a user attempt. The owner,
    /// PR head, and intent remain frozen through pending retries.
    pub(crate) fn fetch_pr_for_open(
        &mut self,
        pr: kagi_domain::github::PullRequest,
        silent: bool,
        cx: &mut Context<Self>,
    ) {
        let (Some(owner), Some(repo_path)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let Some(visit) = self.app_sessions.visit(owner) else {
            return;
        };
        let already_loading = self
            .pr_mode()
            .and_then(|mode| {
                mode.tabs
                    .iter()
                    .find(|tab| tab.pr.number == pr.number && tab.pr.base_repo == pr.base_repo)
            })
            .is_some_and(|tab| tab.local_refs_loading);
        if already_loading {
            return;
        }
        let generation = if let Some(tab) = self.pr_mode_mut().and_then(|mode| {
            mode.tabs
                .iter_mut()
                .find(|tab| tab.pr.number == pr.number && tab.pr.base_repo == pr.base_repo)
        }) {
            tab.local_refs_loading = true;
            tab.local_refs_visit = Some(visit);
            tab.local_refs_generation = tab.local_refs_generation.wrapping_add(1);
            tab.local_refs_generation
        } else {
            return;
        };
        self.start_pr_ref_fetch(owner, visit, repo_path, pr, generation, silent, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn start_pr_ref_fetch(
        &mut self,
        owner: crate::app::SessionId,
        visit: u64,
        repo_path: std::path::PathBuf,
        pr: kagi_domain::github::PullRequest,
        generation: u64,
        silent: bool,
        cx: &mut Context<Self>,
    ) {
        if self.app_sessions.visit(owner) != Some(visit)
            || !self.ui.get(&owner).is_some_and(|ui| {
                ui.pr_mode.as_ref().is_some_and(|mode| {
                    mode.tabs.iter().any(|tab| {
                        tab.pr.number == pr.number
                            && tab.pr.base_repo == pr.base_repo
                            && tab.pr.head_sha == pr.head_sha
                            && tab.local_refs_loading
                            && tab.local_refs_generation == generation
                    })
                })
            })
        {
            return;
        }
        self.refresh_write_busy();
        if self.op_latched() {
            let retry_repo = repo_path.clone();
            let retry_pr = pr.clone();
            cx.spawn(async move |this, acx| {
                acx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(acx, |app, cx| {
                    app.start_pr_ref_fetch(
                        owner, visit, retry_repo, retry_pr, generation, silent, cx,
                    );
                });
            })
            .detach();
            return;
        }
        let lease = if silent {
            self.reserve_write_for("fetch", "fetch", &repo_path, None, cx)
        } else {
            self.reserve_write_for("fetch", "fetch-pr", &repo_path, Some(owner), cx)
        };
        let Some(lease) = lease else {
            self.finish_pr_ref_load(owner, visit, generation, &pr, None, cx);
            // No worker owns this generation. Leave it eligible for the next
            // list tick, without making execution failures retry immediately.
            if silent {
                if let Some(tab) = self.ui.get_mut(&owner).and_then(|ui| {
                    ui.pr_mode.as_mut()?.tabs.iter_mut().find(|tab| {
                        tab.pr.number == pr.number
                            && tab.pr.base_repo == pr.base_repo
                            && tab.local_refs_generation == generation
                    })
                }) {
                    tab.local_refs_visit = None;
                }
            }
            return;
        };
        let abandonment = lease.abandonment();
        let supervision = abandonment.supervision();
        klog!("pr-mode: fetch #{} start", pr.number);
        let base_repo = pr.base_repo.clone();
        let base_branch = pr.base.clone();
        let head_sha = pr.head_sha.clone();
        let number = pr.number;
        let recorded_repo = repo_path.clone();
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_PR_FETCH.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = std::panic::catch_unwind(|| panic!("injected PR fetch task panic"));
                return None;
            }
            // #885: the refs this job moved are observed, not assumed (its
            // refspecs target refs/remotes and refs/kagi/pr, but a configured
            // fetch refspec may also write local branches). #907 review: only
            // the fetch is observed; the analysis after it runs outside, so a
            // branch moved by someone else meanwhile is not recorded as this
            // job's. Not opened: nothing ran, nothing moved.
            let (result, ref_moves) = match kagi_git::Backend::open(&repo_path) {
                Err(error) => (Err(error), Some(Vec::new())),
                Ok(backend) => backend.observe_then(
                    |backend| backend.fetch_pr_refs(&base_repo, number, &base_branch, &head_sha),
                    |backend, (outcome, base_tip, head)| {
                        let base = backend
                            .merge_base(&base_tip, &head)
                            .unwrap_or_else(|_| base_tip.clone());
                        let commits = backend.commits_between(
                            &base,
                            &head,
                            crate::ui::pr_mode::COMMIT_LIMIT,
                        )?;
                        let files = backend.compare_commits(&base, &head)?;
                        let paths: Vec<_> = files.iter().map(|file| file.path.clone()).collect();
                        let head_blobs = backend.blob_ids_at(&head, &paths)?;
                        let diff = files.first().and_then(|file| {
                            backend
                                .compare_file_diff(&base, &head, &file.path)
                                .ok()
                                .map(|raw| {
                                    crate::ui::diff_view::build_main_diff_view(
                                        &raw,
                                        &file.path,
                                        0,
                                        MainDiffSource::Compare {
                                            base: base.clone(),
                                            target: CompareTarget::Commit(head.clone()),
                                            file_index: 0,
                                        },
                                    )
                                })
                        });
                        Ok((
                            outcome,
                            LoadedPrLocal {
                                base,
                                base_tip,
                                head,
                                commits,
                                files,
                                head_blobs,
                                diff,
                            },
                        ))
                    },
                ),
            };
            lease.complete_git(&result);
            Some((result, ref_moves))
        });
        cx.spawn(async move |this, acx| {
            let (result, ref_moves) = task
                .fallible()
                .await
                .flatten()
                .unwrap_or_else(|| (Err(abandonment.into_unknown()), None));
            let _ = this.update(acx, |app, cx| match result {
                Ok((outcome, local)) => {
                    app.refresh_write_busy();
                    let commit_count = local.commits.len();
                    let file_count = local.files.len();
                    let applied =
                        app.finish_pr_ref_load(owner, visit, generation, &pr, Some(local), cx);
                    if applied && app.active_session() == Some(owner) && outcome.changed {
                        app.reload(cx);
                    }
                    if !applied {
                        app.retry_stale_pr_ref_load(owner, visit, &pr, silent, cx);
                    }
                    klog!("pr-mode: fetch #{} ok", pr.number);
                    klog!(
                        "pr-mode: open #{} commits={} files={}",
                        pr.number,
                        commit_count,
                        file_count
                    );
                }
                Err(error) => {
                    app.refresh_write_busy();
                    let applied = app.finish_pr_ref_load(owner, visit, generation, &pr, None, cx);
                    for (id, op, path) in app.app_sessions.drain_unaccounted() {
                        app.notice_reconcile_required(id, op, &path);
                    }
                    app.present_app_notice();
                    klog!("pr-mode: fetch #{} failed: {}", pr.number, error);
                    let detail =
                        format!("{} — PR #{}: {}", recorded_repo.display(), pr.number, error);
                    let outcome = if matches!(error, kagi_git::GitError::TerminationUnknown(_)) {
                        kagi_git::oplog::OpOutcome::Unknown {
                            after: kagi_git::StateSummary {
                                head: "unchanged".into(),
                                dirty: "unchanged".into(),
                            },
                            evidence: detail,
                        }
                    } else {
                        kagi_git::oplog::OpOutcome::Failed { error: detail }
                    };
                    app.record_ref_fetch_failure(
                        "fetch-pr",
                        Some(owner),
                        Some(visit),
                        kagi_git::StateSummary {
                            head: format!("PR #{}", pr.number),
                            dirty: "unchanged".into(),
                        },
                        outcome,
                        ref_moves,
                        None,
                        &recorded_repo,
                        cx,
                    );
                    if !applied {
                        app.retry_stale_pr_ref_load(owner, visit, &pr, silent, cx);
                    } else if app.app_sessions.visit(owner) == Some(visit) {
                        app.refresh_github_prs_for(owner, recorded_repo, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn finish_pr_ref_load(
        &mut self,
        owner: crate::app::SessionId,
        visit: u64,
        generation: u64,
        pr: &kagi_domain::github::PullRequest,
        local: Option<LoadedPrLocal>,
        cx: &mut Context<Self>,
    ) -> bool {
        let current_visit = self.app_sessions.visit(owner) == Some(visit);
        let owner_is_active = self.active_session() == Some(owner);
        let Some(ui) = self.ui.get_mut(&owner) else {
            return false;
        };
        let Some(mode) = ui.pr_mode.as_mut() else {
            return false;
        };
        let Some(tab) = mode.tabs.iter_mut().find(|tab| {
            tab.pr.number == pr.number
                && tab.pr.base_repo == pr.base_repo
                && tab.pr.head_sha == pr.head_sha
                && tab.local_refs_generation == generation
                && tab.local_refs_visit == Some(visit)
        }) else {
            return false;
        };
        tab.local_refs_loading = false;
        if !current_visit {
            // Only this visit's request can clear its latch. A reopened PR in
            // the new visit may reuse the same generation and head SHA.
            cx.notify();
            return false;
        }
        let loaded = local.is_some();
        if let Some(local) = local {
            let diff = local.diff;
            super::super::github_pr_detail::install_local_pr_head(
                tab,
                local.base,
                local.base_tip,
                local.head,
                local.commits,
                local.files,
                local.head_blobs,
            );
            tab.diff = diff;
            tab.conflicts = None;
            tab.conflict_selected = None;
            tab.conflict_preview = None;
            tab.conflict_at = 0;
        }
        let reload_conflicts = loaded
            && owner_is_active
            && mode.view == crate::ui::pr_mode::PrView::Conflicts
            && mode.active.is_some_and(|ix| {
                mode.tabs.get(ix).is_some_and(|open| {
                    open.pr.number == pr.number && open.pr.base_repo == pr.base_repo
                })
            });
        cx.notify();
        if reload_conflicts {
            self.pr_mode_load_conflicts(cx);
        }
        true
    }

    /// A completed old request may leave either a newer head waiting in the
    /// same visit, or an unloaded PR waiting after departure and return. The
    /// current visit's active fetch owns its latch; never start over it.
    fn retry_stale_pr_ref_load(
        &mut self,
        owner: crate::app::SessionId,
        visit: u64,
        stale: &kagi_domain::github::PullRequest,
        silent: bool,
        cx: &mut Context<Self>,
    ) {
        if self.active_session() != Some(owner) {
            return;
        }
        let revisited = self.app_sessions.visit(owner) != Some(visit);
        let latest = self.pr_mode().and_then(|mode| {
            mode.tabs
                .iter()
                .find(|tab| {
                    tab.pr.number == stale.number
                        && tab.pr.base_repo == stale.base_repo
                        && !tab.local_refs_loading
                        && if revisited {
                            tab.head.0 != tab.pr.head_sha
                        } else {
                            tab.pr.head_sha != stale.head_sha
                        }
                })
                .map(|tab| tab.pr.clone())
        });
        if let Some(latest) = latest {
            self.fetch_pr_for_open(latest, silent, cx);
        }
    }

    /// Persist the fetch failure for its frozen repository and push its
    /// receipt into the shared Operation Log panel even after a tab switch.
    /// Only the still-current owner may open the panel. Return the recording
    /// so a departed manual Fetch can link its result to the appended receipt.
    // The frozen receipt and owner stamp arrive separately from two fetch jobs.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn record_ref_fetch_failure(
        &mut self,
        op: &'static str,
        owner: Option<crate::app::SessionId>,
        visit: Option<u64>,
        before: kagi_git::StateSummary,
        outcome: kagi_git::oplog::OpOutcome,
        ref_moves: Option<Vec<kagi_domain::ref_moves::RefMove>>,
        technical_detail: Option<String>,
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> kagi_git::backend::recording::Recording {
        let mut entry =
            kagi_git::oplog::OpLogEntry::new(op, repo_path.display().to_string(), before, outcome)
                .with_ref_moves(ref_moves);
        entry.technical_detail = technical_detail;
        let current = owner.is_some_and(|owner| {
            self.active_session() == Some(owner) && self.app_sessions.visit(owner) == visit
        });
        // #907 review: the panel gets the appended entry (the log's id), not
        // the placeholder id 0 — recorded moves enable undo / restore.
        let recording = kagi_git::backend::recording::finalize(entry);
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = &recording {
            klog!("oplog: write failed (non-fatal): {}", error);
            if current {
                self.present_oplog_write_failure(recording.entry(), error, None, cx);
            } else {
                self.app_notices.push_back(
                    super::record::recording_failure_notice(
                        repo_path.display(),
                        op,
                        &recording.entry().outcome,
                        &error.to_string(),
                        None,
                    )
                    .into(),
                );
            }
        }
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(&recording);
        if let Some(panel) = self.op_log.clone() {
            panel.update(cx, |panel, cx| {
                panel.push(entry);
                panel.collapse();
                cx.notify();
            });
        }
        if current
            && !matches!(
                recording,
                kagi_git::backend::recording::Recording::Failed { .. }
            )
        {
            self.bottom_panel_open = true;
            self.bottom_tab = BottomTab::OperationLog;
        }
        recording
    }
}

impl KagiApp {
    /// Open the host's "create pull/merge request" page for `head_branch`
    /// (bare local name, or `<remote>/<branch>` for a Remote-kind item) in
    /// the system browser, comparing it against the current branch.
    ///
    /// No git write at all — this only resolves a URL and opens it, so there
    /// is no plan/preflight/execute pipeline and no oplog entry, mirroring
    /// `commands.rs`'s `help.documentation` / `help.reportIssue`.
    pub fn open_create_pr(
        &mut self,
        head_branch: String,
        kind: BranchKind,
        cx: &mut Context<Self>,
    ) {
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
                    SharedString::from("create-pr: repo session unavailable"),
                ));
                return;
            }
        };

        let (remote_name, head_only) = match kind {
            BranchKind::Remote => match head_branch.split_once('/') {
                Some((r, b)) => (r.to_string(), b.to_string()),
                None => ("origin".to_string(), head_branch.clone()),
            },
            BranchKind::Local => ("origin".to_string(), head_branch.clone()),
        };
        let remote_url = repo
            .remote_url_named(&remote_name)
            .or_else(|| repo.remote_urls().ok().and_then(|v| v.into_iter().next()));

        let base_branch = self
            .view()
            .branches
            .iter()
            .find(|(_, current)| *current)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "main".to_string());

        match remote_url
            .and_then(|u| kagi_domain::pr_url::pr_create_url(&u, &base_branch, &head_only))
        {
            Some(url) => {
                klog!("create-pr: opening {}", url);
                cx.open_url(&url);
            }
            None => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(SharedString::from(
                    "create-pr: remote host not recognized (github.com / gitlab.com / bitbucket.org only)",
                )));
                self.push_toast(
                    ToastKind::Error,
                    i18n::op::user_text::diagnostic(
                        "create-pr: remote host not recognized (github.com / gitlab.com / bitbucket.org only)",
                    ).into_owned(),
                    cx,
                );
            }
        }
    }
}
