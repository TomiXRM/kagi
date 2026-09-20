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

struct LoadedPrLocal {
    base: CommitId,
    base_tip: CommitId,
    head: CommitId,
    commits: Vec<kagi_git::Commit>,
    files: Vec<kagi_git::FileStatus>,
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "delete-remote-branch: repo session unavailable",
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
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "delete-remote-branch plan error: {}",
                    e
                )));
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
            self.record_op(
                "delete-remote-branch",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_delete_remote_branch_modal();
            cx.notify();
            return;
        }

        self.clear_delete_remote_branch_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Deleting remote branch '{}'…",
            modal.remote_branch
        )));
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
                Ok(_) => RunPresentation::status(FooterStatus::Success(SharedString::from(
                    format!("delete-remote-branch: '{}' deleted", remote_branch),
                ))),
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
        klog!("fetch-remote-branch: start {}", remote_branch);
        let bg_path = repo_path.clone();
        let bg_remote_branch = remote_branch.clone();
        let task = cx.background_spawn(async move {
            let result = crate::ui::blocking_ops::open_backend(&bg_path);
            let open_failed = result.is_err();
            let result = result.and_then(|backend| backend.fetch_remote_branch(&bg_remote_branch));
            lease.complete_git(&result);
            result.map_err(|e| {
                if open_failed {
                    i18n::op_failed(i18n::Op::RepoOpen, e)
                } else {
                    format!("{e}")
                }
            })
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                match result {
                    Ok(outcome) => {
                        klog!("fetch-remote-branch: ok {}", remote_branch);
                        if outcome.changed {
                            app.reload(cx);
                        }
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "Fetched {}",
                            remote_branch
                        )));
                        app.push_toast(
                            ToastKind::Success,
                            format!("Fetched {}", remote_branch),
                            cx,
                        );
                    }
                    Err(e) => {
                        klog!("fetch-remote-branch: failed {} — {}", remote_branch, e);
                        let msg = i18n::op_failed(i18n::Op::Fetch, e);
                        app.status_footer = FooterStatus::Failed(SharedString::from(msg.clone()));
                        app.push_toast(ToastKind::Error, msg, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch the base and GitHub synthetic head ref, then finish opening the
    /// PR tab that was shown immediately by the click. The owner and PR head
    /// are frozen before the background work starts.
    pub(crate) fn fetch_pr_for_open(
        &mut self,
        pr: kagi_domain::github::PullRequest,
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
            tab.local_refs_generation = tab.local_refs_generation.wrapping_add(1);
            tab.local_refs_generation
        } else {
            return;
        };
        self.start_pr_ref_fetch(owner, visit, repo_path, pr, generation, cx);
    }

    fn start_pr_ref_fetch(
        &mut self,
        owner: crate::app::SessionId,
        visit: u64,
        repo_path: std::path::PathBuf,
        pr: kagi_domain::github::PullRequest,
        generation: u64,
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
                    app.start_pr_ref_fetch(owner, visit, retry_repo, retry_pr, generation, cx);
                });
            })
            .detach();
            return;
        }
        let Some(lease) = self.reserve_write("fetch", &repo_path, cx) else {
            self.finish_pr_ref_load(owner, visit, generation, &pr, None, cx);
            return;
        };
        klog!("pr-mode: fetch #{} start", pr.number);
        let base_repo = pr.base_repo.clone();
        let base_branch = pr.base.clone();
        let head_sha = pr.head_sha.clone();
        let number = pr.number;
        let recorded_repo = repo_path.clone();
        let task = cx.background_spawn(async move {
            let result = kagi_git::Backend::open(&repo_path).and_then(|backend| {
                let (outcome, base_tip, head) =
                    backend.fetch_pr_refs(&base_repo, number, &base_branch, &head_sha)?;
                let base = backend
                    .merge_base(&base_tip, &head)
                    .unwrap_or_else(|_| base_tip.clone());
                let commits =
                    backend.commits_between(&base, &head, crate::ui::pr_mode::COMMIT_LIMIT)?;
                let files = backend.compare_commits(&base, &head)?;
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
                        diff,
                    },
                ))
            });
            lease.complete_git(&result);
            result
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
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
                        app.retry_stale_pr_ref_load(owner, visit, &pr, cx);
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
                    app.record_pr_fetch_failure(
                        owner,
                        "fetch-pr",
                        kagi_git::StateSummary {
                            head: format!("PR #{}", pr.number),
                            dirty: "unchanged".into(),
                        },
                        outcome,
                        &recorded_repo,
                        cx,
                    );
                    if !applied {
                        app.retry_stale_pr_ref_load(owner, visit, &pr, cx);
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
        if self.app_sessions.visit(owner) != Some(visit) {
            return false;
        }
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
        }) else {
            return false;
        };
        tab.local_refs_loading = false;
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

    /// A newer L1 head may arrive while an older fetch owns the global fetch
    /// lease. Once that stale request settles, hand the lease to the newest
    /// tab intent instead of leaving the page empty until another click.
    fn retry_stale_pr_ref_load(
        &mut self,
        owner: crate::app::SessionId,
        visit: u64,
        stale: &kagi_domain::github::PullRequest,
        cx: &mut Context<Self>,
    ) {
        if self.active_session() != Some(owner) || self.app_sessions.visit(owner) != Some(visit) {
            return;
        }
        let latest = self.pr_mode().and_then(|mode| {
            mode.tabs
                .iter()
                .find(|tab| {
                    tab.pr.number == stale.number
                        && tab.pr.base_repo == stale.base_repo
                        && tab.pr.head_sha != stale.head_sha
                        && !tab.local_refs_loading
                })
                .map(|tab| tab.pr.clone())
        });
        if let Some(latest) = latest {
            self.fetch_pr_for_open(latest, cx);
        }
    }

    /// Persist a passive PR-load failure without replacing another tab's
    /// footer or creating an expiring snackbar. The Operation Log owns the
    /// complete error; when its repository is active, reveal that row.
    fn record_pr_fetch_failure(
        &mut self,
        owner: crate::app::SessionId,
        op: &str,
        before: kagi_git::StateSummary,
        outcome: kagi_git::oplog::OpOutcome,
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let entry =
            kagi_git::oplog::OpLogEntry::new(op, repo_path.display().to_string(), before, outcome);
        if let Err(error) = kagi_git::oplog::append_oplog(&entry) {
            klog!("oplog: write failed (non-fatal): {}", error);
            self.present_oplog_write_failure(&error, cx);
        }
        if let Some(panel) = self.op_log.clone() {
            panel.update(cx, |panel, cx| {
                panel.push(entry);
                panel.collapse();
                cx.notify();
            });
        }
        if self.active_session() == Some(owner) {
            self.bottom_panel_open = true;
            self.bottom_tab = BottomTab::OperationLog;
        }
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
                self.status_footer =
                    FooterStatus::Failed(SharedString::from("create-pr: repo session unavailable"));
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "create-pr: remote host not recognized (github.com / gitlab.com / bitbucket.org only)",
                ));
                self.push_toast(
                    ToastKind::Error,
                    "Create PR: remote host not recognized",
                    cx,
                );
            }
        }
    }
}
