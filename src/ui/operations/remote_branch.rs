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

use crate::ui::blocking_ops::*;
use crate::ui::*;

impl KagiApp {
    /// Open the delete-remote-branch modal for `remote_branch` (e.g.
    /// `"origin/feature/x"`).
    pub fn open_delete_remote_branch_modal(&mut self, remote_branch: impl Into<String>) {
        let remote_branch = remote_branch.into();
        let repo = match self.repo_session.as_ref() {
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
    /// thread (busy_op) and reloads.
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

        if self.busy_op.is_some() {
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

        self.busy_op = Some("delete-remote-branch");
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
        let task = cx.background_spawn(async move {
            delete_remote_branch_blocking(&bg_path, &bg_plan, &bg_remote_branch)
        });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(after) => {
                klog!("async: delete-remote-branch finished");
                app.record_op(
                    "delete-remote-branch",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "delete-remote-branch: '{}' deleted",
                    remote_branch
                )));
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: delete-remote-branch failed — {}", err_msg);
                app.record_op(
                    "delete-remote-branch",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                app.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                    remote_branch: remote_branch.clone(),
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    confirm_armed: false,
                });
            }
        });
    }
}

impl KagiApp {
    /// Fetch a single remote branch's refspec (e.g. `"origin/feature/x"`).
    /// No modal, no plan — fetch never merges or moves the current branch, so
    /// it fires directly, mirroring the repo-level `Fetch` command.
    pub fn fetch_remote_branch_async(&mut self, remote_branch: String, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        klog!("fetch-remote-branch: start {}", remote_branch);
        let bg_path = repo_path.clone();
        let bg_remote_branch = remote_branch.clone();
        let task = cx.background_spawn(async move {
            let backend =
                kagi_git::Backend::open(&bg_path).map_err(|e| format!("repo open error: {e}"))?;
            backend
                .fetch_remote_branch(&bg_remote_branch)
                .map_err(|e| format!("{e}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
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
                        app.status_footer =
                            FooterStatus::Failed(SharedString::from(format!("Fetch failed: {e}")));
                        app.push_toast(ToastKind::Error, format!("Fetch failed: {e}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
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
        let repo = match self.repo_session.as_ref() {
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
            .active_view
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
