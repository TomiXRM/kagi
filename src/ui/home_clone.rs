//! Clone task ownership, native destination selection and completion routing.
use super::{
    home_github::{CloneModal, CloneTarget},
    i18n::Msg,
    KagiApp,
};
use gpui::{AppContext as _, Context};
#[cfg(not(feature = "gui-e2e"))]
use gpui::{PathPromptOptions, SharedString};
use kagi_git::{github_repos::RepoListing, ops::CloneRequest};
use std::path::Path;

/// Like the worktree Input identity (#1383), the task's focus handle is its
/// card incarnation. Replanning retains it; closing/reopening creates a new one.
#[derive(PartialEq)]
struct CloneCardContext {
    focus: Option<gpui::FocusHandle>,
    attachment: Option<crate::app::Attachment>,
    home: bool,
}
impl KagiApp {
    fn clone_card_context(&self) -> CloneCardContext {
        CloneCardContext {
            focus: self.modal_focus.clone(),
            attachment: self
                .active_session()
                .and_then(|s| self.app_sessions.attachment(s)),
            home: self.home_in_front(),
        }
    }

    /// Ask where to clone `listing`: the card opens with no folder chosen.
    pub fn open_clone_card(&mut self, listing: RepoListing, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        klog!("clone: card {}", listing.clone_source());
        self.set_clone_modal(CloneModal {
            listing,
            target: None,
            started: None,
        });
    }

    /// Choose the folder to clone into (the system folder dialog); the card
    /// is planned for it.
    pub fn change_clone_parent(&mut self, cx: &mut Context<Self>) {
        if self
            .clone_modal()
            .is_none_or(|modal| modal.started.is_some())
        {
            return;
        }
        let context = self.clone_card_context();
        let picker_owner = self.repo_path.clone().filter(|_| !self.home_in_front());
        #[cfg(feature = "gui-e2e")]
        let Some(receiver) = super::e2e::take_folder_prompt(Msg::CloneChooseFolder.t()) else {
            return;
        };
        #[cfg(not(feature = "gui-e2e"))]
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(Msg::CloneChooseFolder.t())),
        });
        cx.spawn(async move |app, acx| {
            #[cfg(feature = "gui-e2e")]
            let parent = receiver.await;
            #[cfg(not(feature = "gui-e2e"))]
            let parent = receiver
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()))
                .map(|paths| paths.and_then(|paths| paths.into_iter().next()));
            let _ = app.update(acx, |app, cx| {
                if app.clone_card_context() != context
                    || app
                        .clone_modal()
                        .is_none_or(|modal| modal.started.is_some())
                {
                    return;
                }
                match parent {
                    Ok(Some(parent)) => app.replan_clone(&parent, cx),
                    Ok(None) => {}
                    Err(error) => app.report_path_picker_failure(
                        picker_owner,
                        "clone-folder-picker",
                        Msg::CloneChooseFolder.t(),
                        error,
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    /// Plan the open card for a clone inside `parent`.
    pub fn replan_clone(&mut self, parent: &Path, cx: &mut Context<Self>) {
        let Some(modal) = self.clone_modal().cloned() else {
            return;
        };
        if modal.started.is_some() {
            return;
        }
        let target = CloneTarget::new(&modal.listing, parent);
        klog!(
            "clone: plan {} -> {} blockers={}",
            target.request.source,
            target.request.dest.display(),
            target.plan.blockers.len()
        );
        self.set_clone_modal(CloneModal {
            listing: modal.listing,
            target: Some(target),
            started: None,
        });
        cx.notify();
    }

    /// Close the card. A clone already running keeps going in the background
    /// (its row reads "Cloning…", and its result arrives as usual).
    pub fn cancel_clone(&mut self) {
        self.clear_clone_modal();
        self.focus_root_for_modal();
    }

    /// Confirm the card: run the planned clone in the background. Refused on
    /// the spot while any operation is latched — another clone, or a write
    /// or plan in a repository tab (the shared `op_latched` gate).
    pub fn start_clone(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.clone_modal().cloned() else {
            return;
        };
        let Some(CloneTarget { request, plan }) = modal.target.clone() else {
            return;
        };
        if modal.started.is_some() || !plan.blockers.is_empty() {
            return;
        }
        if self.op_latched() {
            let busy = if self.home_github.cloning.is_some() {
                Msg::CloneBusy.t()
            } else {
                Msg::OpInProgress.t()
            };
            self.push_toast_for(
                crate::ui::toast_stack::ToastSubject::App,
                super::ToastKind::Error,
                busy,
                cx,
            );
            return;
        }
        // The card stays up as the clone's progress: pressing Clone must
        // visibly do something at once (user report: a silent 3 s looked like
        // a missed click or a freeze).
        let card = CloneModal {
            started: Some(std::time::Instant::now()),
            ..modal
        };
        self.set_clone_modal(card.clone());
        self.tick_clone_card(cx);
        klog!(
            "clone: start {} -> {}",
            request.source,
            request.dest.display()
        );
        self.home_github.cloning = Some(card);
        self.hidden_busy_job = None;
        let run = request.clone();
        let context = self.clone_card_context();
        #[cfg(feature = "gui-e2e")]
        let hold = super::e2e::take_clone_run();
        cx.spawn(async move |app, acx| {
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            let report = acx
                .background_spawn(async move { kagi_git::ops::execute_clone(&run, &plan) })
                .await;
            let _ = app.update(acx, |app, cx| {
                app.finish_clone(context, request, report, cx)
            });
        })
        .detach();
        cx.notify();
    }

    /// Redraw the running clone's card once a second, for its elapsed time,
    /// while it is on screen. Only the latest ticker runs: one started
    /// before it ends at its next wake (#968).
    pub(super) fn tick_clone_card(&mut self, cx: &mut Context<Self>) {
        self.home_github.clone_tick += 1;
        let tick = self.home_github.clone_tick;
        #[cfg(feature = "gui-e2e")]
        super::e2e::clone_ticker_started();
        cx.spawn(async move |app, acx| {
            loop {
                acx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let running = app
                    .update(acx, |app, cx| {
                        let running = app.home_github.clone_tick == tick
                            && app.clone_modal().is_some_and(|m| m.started.is_some());
                        if running {
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
            #[cfg(feature = "gui-e2e")]
            super::e2e::clone_ticker_ended();
        })
        .detach();
    }

    fn finish_clone(
        &mut self,
        context: CloneCardContext,
        request: CloneRequest,
        report: kagi_git::backend::recording::RunReport,
        cx: &mut Context<Self>,
    ) {
        self.home_github.cloning = None;
        let running_card = self.clone_modal().is_some_and(|modal| {
            modal.started.is_some()
                && modal
                    .target
                    .as_ref()
                    .is_some_and(|target| target.request == request)
        });
        let current_context = self.clone_card_context();
        let current = current_context.attachment == context.attachment
            && current_context.home == context.home
            && running_card;
        // Reopening this running clone is explicit interest; a new card is not.
        // Hide/departure retires navigation, never the clone or its receipt.
        if running_card {
            self.clear_clone_modal();
        }
        klog!(
            "clone: done {} ok={}",
            request.dest.display(),
            report.result.is_ok()
        );
        self.notice_recording_failure("clone", &report.recording, &request.dest, None);
        let toast_id = self.present_recorded(&report.recording, current, cx);
        if let (Some(id), Some(stack)) = (toast_id, self.toast_stack.as_ref()) {
            stack.update(cx, |stack, cx| {
                stack.set_subject(id, super::toast_stack::ToastSubject::App, cx);
            });
        }
        if report.result.is_ok() {
            if current {
                self.open_repository(request.dest, cx);
            } else if matches!(
                report.recording.entry().outcome,
                kagi_git::oplog::OpOutcome::Success { .. }
            ) {
                if let (Some(id), Some(stack)) = (toast_id, self.toast_stack.as_ref()) {
                    stack.update(cx, |stack, cx| {
                        stack.set_action(
                            id,
                            super::types::ToastAction::OpenRepository(request.dest),
                            cx,
                        );
                    });
                }
            }
        }
        cx.notify();
    }
}
