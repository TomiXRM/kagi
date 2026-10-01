//! Terminal auto-lock hooks (#772 Phase 1, ADR-0208 決定 3 / 4).
//!
//! Two moments drive everything: the shell **started** (offer to lock the
//! linked worktree with this session's token) and the shell **exited** as
//! observed by the background `wait` (offer to release *that* lock). Both
//! offers are plans the user confirms; nothing here writes a lock.

use gpui::{Context, Window};

use super::{AutoLockOffer, KagiTerminalSession, ShellExit};
use crate::app::SessionId;
use crate::ui::{FooterStatus, KagiApp, SharedString, ToastKind, UnlockWorktreeModal};
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
        // A confirmed lock from an earlier shell still needs its release
        // decision. Starting another shell must not forget its provenance.
        if session.view.is_none() && session.auto_lock.is_some() {
            let message = crate::ui::i18n::Msg::TerminalAutoLockPendingRelease.t();
            session.start_error = Some(message.into());
            self.ui
                .get_mut(&owner)
                .expect("attached owner")
                .terminal_session = Some(session);
            self.status_footer = FooterStatus::Failed(SharedString::from(message));
            self.push_toast(ToastKind::Error, message, cx);
            return;
        }
        // #859: in `nonconcurrent` mode another worktree of this repository
        // with a running shell stops a new one here. Not a write: no oplog.
        if session.view.is_none() {
            if let Some(running) = self.nonconcurrent_blocker(&repo_path) {
                klog!(
                    "terminal: nonconcurrent blocked {} (running in {})",
                    repo_path.display(),
                    running.display()
                );
                let message = crate::ui::i18n::terminal_nonconcurrent_blocked(
                    &repo_path.display().to_string(),
                    &running.display().to_string(),
                );
                session.start_error = Some(message.clone());
                if let Some(ui) = self.ui.get_mut(&owner) {
                    ui.terminal_session = Some(session);
                }
                self.status_footer = FooterStatus::Failed(SharedString::from(message.clone()));
                self.push_toast(ToastKind::Error, message, cx);
                return;
            }
        }
        let mut failure_msg: Option<String> = None;
        let mut exhausted: Option<super::PortsExhausted> = None;
        let started = super::ensure_terminal(
            &mut session,
            owner,
            window,
            cx,
            |msg| failure_msg = Some(msg),
            |ports| exhausted = Some(ports),
        );
        let started_shell = started
            .then(|| {
                session
                    .shell
                    .as_ref()
                    .map(|shell| (shell.generation, shell.pid))
            })
            .flatten();
        // Only a successful confirm records ownership; the new shell starts
        // with no prior claim (the guard above keeps pending releases intact).
        if let Some(ui) = self.ui.get_mut(&owner) {
            ui.terminal_session = Some(session);
        }
        if started {
            if let Some((generation, pid)) = started_shell {
                self.record_started_shell(owner, generation, &repo_path, pid);
            }
            // #772 / ADR-0208 決定 4: opt-in only; plan → confirm, never a write.
            self.offer_auto_lock(owner, cx);
            // #855: a terminal is what assigns a block; show it in the sidebar
            // now rather than at the next reload. #869: in a shared-port mode
            // the block assigned is the main worktree's, so refresh every row.
            let ports = kagi_git::worktree_ports::Assignments::read();
            let mut changed = false;
            for worktree in self.view_mut().worktrees.iter_mut() {
                let port = ports.port(&worktree.path);
                if worktree.port != port {
                    worktree.port = port;
                    changed = true;
                }
            }
            if changed {
                // The sidebar rows are cached on this epoch.
                self.view_epoch = self.view_epoch.wrapping_add(1);
            }
        }
        if let Some(ports) = exhausted {
            // #852: the shell runs; say why it has no KAGI_PORT and how to
            // give it one. Not an operation, so no oplog entry.
            let message = crate::ui::i18n::terminal_ports_exhausted(
                &ports.worktree.display().to_string(),
                ports.range,
                ports.per,
            );
            self.status_footer = FooterStatus::Failed(SharedString::from(message.clone()));
            self.push_toast(ToastKind::Error, message, cx);
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
        // Whether or not the tab is still open (#867 review).
        self.note_shell_wait(owner, generation, &exit);
        let Some(session) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.terminal_session.as_mut())
        else {
            return;
        };
        let Some(shell) = session.shell.as_mut() else {
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
        if matches!(exit, ShellExit::Exited { .. })
            && session
                .auto_lock
                .as_ref()
                .is_some_and(|claim| claim.generation == generation)
        {
            session.release_offer_pending = true;
        }
        cx.notify();
        if matches!(exit, ShellExit::Exited { .. }) {
            // A shell that exited before the user approved its acquire may
            // not leave a confirmable card behind.
            if self.lock_worktree_modal().is_some_and(|modal| {
                modal
                    .auto
                    .as_ref()
                    .is_some_and(|offer| offer.owner == owner && offer.generation == generation)
            }) {
                self.clear_lock_worktree_modal();
            }
            // A confirmed lock is eligible for a release proposal even if
            // the user switched the opt-in OFF after acquiring it.
            self.offer_auto_release(owner, cx);
        }
    }

    /// The shell just started in `owner`'s tab (called from `ensure_terminal`
    /// after a successful spawn). With the opt-in on and the tab a linked
    /// worktree, plan a lock with this session's token and open the ordinary
    /// lock card; the user confirms before anything is written.
    pub(crate) fn offer_auto_lock(&mut self, owner: SessionId, cx: &mut Context<Self>) {
        if !crate::ui::settings::terminal_auto_lock() || self.active_session() != Some(owner) {
            return;
        }
        if self.has_active_modal() {
            // Shell startup is an asynchronous producer, not a user request
            // to replace a confirmation already occupying the one modal slot.
            return;
        }
        let Some(attachment) = self.app_sessions.attachment(owner) else {
            return;
        };
        if self.repo_path.as_ref() != Some(&attachment.path) {
            return;
        }
        let Some(generation) = self
            .ui
            .get(&owner)
            .and_then(|ui| ui.terminal_session.as_ref())
            .and_then(|session| session.shell.as_ref())
            .filter(|shell| shell.exit.is_none())
            .map(|shell| shell.generation)
        else {
            return;
        };
        let Some(repo) = self.worktree_backend("auto-lock-worktree") else {
            return;
        };
        let Some(name) = repo.linked_worktree_name() else {
            klog!("terminal: auto-lock skipped: not a linked worktree");
            return;
        };
        let worktree = match repo.linked_worktree_identity(&name) {
            Ok(worktree) if attachment.worktree.as_ref() == Some(&worktree) => worktree,
            Ok(_) => {
                klog!("terminal: auto-lock skipped: repository identity changed");
                return;
            }
            Err(e) => {
                klog!("terminal: auto-lock skipped: {}", e);
                return;
            }
        };
        let Some(token) = session_token(owner, generation) else {
            klog!("terminal: auto-lock skipped: token entropy unavailable");
            return;
        };
        let target = AutoUnlockTarget { token, worktree };
        let reason = target.token.reason();
        let offer = AutoLockOffer {
            owner,
            generation,
            path: attachment.path,
            target,
        };
        match repo.plan_lock_worktree(&name, Some(&reason)) {
            Ok(plan) => {
                klog!("plan: lock-worktree {} (auto)", name);
                self.set_lock_worktree_modal(crate::ui::LockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    reason,
                    auto: Some(offer),
                });
                // #817: the terminal just took focus; the card owns Enter /
                // Escape through the root, so ask for the root on open.
                self.focus_root_for_modal();
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

    /// A button may clear any modal without traversing root Enter/Escape.
    /// Schedule the pending owner's proposal after render, never reading Git
    /// during render. A competing modal/owner restores the one-shot request.
    pub(crate) fn defer_pending_auto_release(&mut self, cx: &mut Context<Self>) {
        if self.has_active_modal() {
            return;
        }
        let Some(owner) = self.active_session() else {
            return;
        };
        let Some(session) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.terminal_session.as_mut())
        else {
            return;
        };
        if !session.release_offer_pending {
            return;
        }
        session.release_offer_pending = false;
        let app = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, cx| {
                if app.active_session() == Some(owner) && !app.has_active_modal() {
                    app.offer_auto_release(owner, cx);
                } else if let Some(session) = app
                    .ui
                    .get_mut(&owner)
                    .and_then(|ui| ui.terminal_session.as_mut())
                {
                    session.release_offer_pending = session.auto_lock.is_some();
                }
            });
        });
    }

    /// A confirmed, owner-scoped lock may be offered for release only after
    /// its exact shell generation has exited. A background tab or an occupied
    /// modal slot keeps the claim pending until the owner is active again.
    pub(crate) fn offer_auto_release(&mut self, owner: SessionId, cx: &mut Context<Self>) {
        if self.active_session() != Some(owner) || self.has_active_modal() {
            return;
        }
        let Some(offer) = self
            .ui
            .get(&owner)
            .and_then(|ui| ui.terminal_session.as_ref())
            .and_then(|session| {
                let claim = session.auto_lock.as_ref()?;
                let shell = session.shell.as_ref()?;
                (shell.generation == claim.generation
                    && matches!(shell.exit, Some(ShellExit::Exited { .. })))
                .then(|| claim.clone())
            })
        else {
            return;
        };
        if let Some(session) = self
            .ui
            .get_mut(&owner)
            .and_then(|ui| ui.terminal_session.as_mut())
        {
            session.release_offer_pending = false;
        }
        if !self.auto_lock_owner_is_current(&offer) {
            klog!("terminal: auto-unlock skipped: repository identity changed");
            return;
        }
        let repo = match crate::ui::blocking_ops::open_backend(&offer.path) {
            Ok(repo) => repo,
            Err(e) => {
                klog!("terminal: auto-unlock skipped: {}", e);
                return;
            }
        };
        if repo.write_worktree_id().ok().as_ref() != Some(&offer.target.worktree) {
            klog!("terminal: auto-unlock skipped: repository identity changed");
            return;
        }
        let name = offer
            .target
            .worktree
            .git_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match repo.plan_auto_unlock_worktree(&name, &offer.target) {
            Ok(plan) if plan.blockers.is_empty() => {
                klog!("plan: auto-unlock-worktree {}", name);
                self.set_unlock_worktree_modal(UnlockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    auto: Some(offer),
                });
                // #817: the release card has no text field; Enter belongs to
                // the root, never to the terminal or a departing tab.
                self.focus_root_for_modal();
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

/// The process nonce prevents a restarted app with a reused PID/tab counter
/// from adopting a lock it did not acquire. Failure to obtain entropy disables
/// this opt-in operation rather than falling back to a predictable token.
fn session_token(owner: SessionId, generation: u64) -> Option<AutoLockToken> {
    static NONCE: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        let mut nonce = String::with_capacity(32);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            nonce.push(HEX[(byte >> 4) as usize] as char);
            nonce.push(HEX[(byte & 0x0f) as usize] as char);
        }
        Some(nonce)
    });
    let nonce = &*NONCE;
    AutoLockToken::new(&format!(
        "{}-{}-{}-{}",
        nonce.as_ref()?,
        owner.tab.0,
        owner.incarnation,
        generation
    ))
}

impl KagiTerminalSession {
    /// Forget provenance only after the user explicitly declines a release
    /// or its execution succeeds; a new shell cannot discard an active claim.
    pub(crate) fn clear_auto_lock(&mut self) {
        self.auto_lock = None;
        self.release_offer_pending = false;
    }
}
