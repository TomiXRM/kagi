//! Terminal auto-lock hooks (#772 Phase 1, ADR-0206 決定 3 / 4).
//!
//! Two moments drive everything: the shell **started** (offer to lock the
//! linked worktree with this session's token) and the shell **exited** as
//! observed by the background `wait` (offer to release *that* lock). Both
//! offers are plans the user confirms; nothing here writes a lock.

use gpui::{Context, Window};

use super::{KagiTerminalSession, ShellExit};
use crate::app::SessionId;
use crate::ui::{FooterStatus, KagiApp, SharedString, UnlockWorktreeModal};
use kagi_domain::worktree_autolock::{AutoLockToken, AutoUnlockTarget};

impl KagiApp {
    /// Ensure the active session's terminal exists and its shell is running.
    /// The session owner is frozen into the exit callback so completion cannot
    /// clear another tab's terminal.
    pub fn ensure_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.active_session() else {
            klog!("terminal: no repo_path — cannot start terminal");
            return;
        };
        let repo_path = match self.repo_path.clone() {
            Some(path) => path,
            None => {
                klog!("terminal: no repo_path — cannot start terminal");
                return;
            }
        };
        let mut session = self
            .ui
            .get_mut(&owner)
            .expect("active session must own TabUiState")
            .terminal_session
            .take()
            .unwrap_or_else(|| KagiTerminalSession::new(repo_path.clone()));
        let mut failure_msg: Option<String> = None;
        let started = super::ensure_terminal(&mut session, owner, window, cx, |msg| {
            failure_msg = Some(msg);
        });
        if started {
            // A fresh shell: any lock target from a previous spawn is stale.
            session.clear_auto_lock();
        }
        if let Some(ui) = self.ui.get_mut(&owner) {
            ui.terminal_session = Some(session);
        }
        if started {
            // #772 / ADR-0206 決定 4: opt-in only; plan → confirm, never a write.
            self.offer_auto_lock(owner, cx);
        }

        if let Some(err) = failure_msg {
            use kagi_git::oplog::OpOutcome;
            use kagi_git::ops::StateSummary;
            // terminal start does not go through Backend::run — persist here.
            self.record_op_persist(
                "terminal-start",
                StateSummary {
                    head: "n/a".to_string(),
                    dirty: "n/a".to_string(),
                },
                OpOutcome::Failed { error: err },
                &repo_path,
                cx,
            );
        }
    }

    /// The background `wait` on a shell returned (欠落 2). Delivered for the
    /// owner and spawn generation the child belonged to; a newer spawn in the
    /// same session makes this stale and it is dropped. An `Unknown` wait is
    /// recorded but is not an exit.
    pub(crate) fn on_shell_wait(
        &mut self,
        owner: SessionId,
        generation: u64,
        exit: ShellExit,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.terminal_session.as_mut())
            .and_then(|session| session.shell.as_mut())
        else {
            return;
        };
        if shell.generation != generation {
            return;
        }
        let status = match &exit {
            ShellExit::Exited { code } => format!("exited {code}"),
            ShellExit::Unknown(_) => "unknown".to_string(),
        };
        klog!("terminal: shell wait: gen={} status={}", generation, status);
        shell.exit = Some(exit.clone());
        cx.notify();
        if matches!(exit, ShellExit::Exited { .. }) && crate::ui::settings::terminal_auto_lock() {
            self.offer_auto_release(owner, cx);
        }
    }

    /// The shell just started in `owner`'s tab (called from `ensure_terminal`
    /// after a successful spawn). With the opt-in on and the tab a linked
    /// worktree, plan a lock with this session's token and open the ordinary
    /// lock card; the user confirms before anything is written.
    pub(crate) fn offer_auto_lock(&mut self, owner: SessionId, cx: &mut Context<Self>) {
        if !crate::ui::settings::terminal_auto_lock() {
            return;
        }
        let Some(repo) = self.worktree_backend("auto-lock-worktree") else {
            return;
        };
        let Some(name) = repo.linked_worktree_name() else {
            klog!("terminal: auto-lock skipped: not a linked worktree");
            return;
        };
        let target = match repo.linked_worktree_identity(&name) {
            Ok(worktree) => AutoUnlockTarget {
                token: session_token(owner),
                worktree,
            },
            Err(e) => {
                klog!("terminal: auto-lock skipped: {}", e);
                return;
            }
        };
        let reason = target.token.reason();
        match repo.plan_lock_worktree(&name, Some(&reason)) {
            Ok(plan) => {
                klog!("plan: lock-worktree {} (auto)", name);
                if let Some(session) = self
                    .ui
                    .get_mut(&owner)
                    .and_then(|ui| ui.terminal_session.as_mut())
                {
                    session.auto_lock = Some(target);
                }
                self.set_lock_worktree_modal(crate::ui::LockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    reason,
                });
                cx.notify();
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "auto-lock-worktree plan error: {}",
                    e
                )));
            }
        }
    }

    /// The shell of `owner`'s terminal exited. If this session offered a lock,
    /// plan its release; a plan with blockers (the user never confirmed, the
    /// lock was replaced by hand, another session's token, …) is logged and
    /// dropped — no card, no write (contract B).
    fn offer_auto_release(&mut self, owner: SessionId, cx: &mut Context<Self>) {
        let Some(target) = self
            .ui
            .get(&owner)
            .and_then(|ui| ui.terminal_session.as_ref())
            .and_then(|session| session.auto_lock.clone())
        else {
            return;
        };
        let Some(repo) = self.worktree_backend("auto-unlock-worktree") else {
            return;
        };
        let name = target
            .worktree
            .git_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match repo.plan_auto_unlock_worktree(&name, &target) {
            Ok(plan) if plan.blockers.is_empty() => {
                klog!("plan: auto-unlock-worktree {}", name);
                self.set_unlock_worktree_modal(UnlockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    auto: Some(target),
                });
                cx.notify();
            }
            Ok(plan) => {
                let why = plan
                    .blockers
                    .first()
                    .map(|b| b.message_en())
                    .unwrap_or_default();
                klog!("terminal: auto-unlock skipped: {}", why);
            }
            Err(e) => klog!("terminal: auto-unlock skipped: {}", e),
        }
    }
}

/// The token this process's session `owner` signs its locks with: unique
/// across processes (PID) and across tab reopenings (session incarnation).
fn session_token(owner: SessionId) -> AutoLockToken {
    AutoLockToken::new(&format!(
        "{}-{}-{}",
        std::process::id(),
        owner.tab.0,
        owner.incarnation
    ))
    .expect("pid/tab/incarnation contain no whitespace")
}

impl KagiTerminalSession {
    /// Forget an offered lock target (a new spawn gets a fresh one).
    pub(crate) fn clear_auto_lock(&mut self) {
        self.auto_lock = None;
    }
}
