//! Origin- and freshness-aware transitions for the single modal slot (#718).

use super::super::super::modals::{
    ActiveModal, PopPlanModal, PullPlanModal, StashApplyModal, StashDropModal,
};
use super::super::super::KagiApp;

#[derive(Clone, Copy)]
pub(crate) enum AsyncPlanToken {
    None,
    Session,
}

pub(crate) struct AsyncPlanOffer {
    pub(crate) operation: crate::ui::i18n::Op,
    pub(crate) modal: ActiveModal,
    pub(crate) token: AsyncPlanToken,
}

impl AsyncPlanOffer {
    pub(crate) fn new(operation: crate::ui::i18n::Op, modal: ActiveModal) -> Self {
        Self {
            operation,
            modal,
            token: AsyncPlanToken::None,
        }
    }

    pub(crate) fn with_session_token(mut self) -> Self {
        self.token = AsyncPlanToken::Session;
        self
    }
}

pub(crate) enum PlanningPresentation {
    Offer(Box<AsyncPlanOffer>),
    Failed {
        operation: crate::ui::i18n::Op,
        error: String,
    },
}

impl KagiApp {
    /// The only transition allowed to replace a modal already in the slot.
    /// Callers are synchronous user intents; async producers use the typed
    /// offer/update/notice seams below (#718 / ADR-0196).
    pub(super) fn replace_modal_from_user(&mut self, modal: ActiveModal) {
        self.sidebar.swipe.cancel();
        if let Some(ActiveModal::AppNotice(notice)) = self.active_modal.take() {
            self.app_notices.push_front(notice);
        }
        self.modal_list_scroll = gpui::UniformListScrollHandle::new();
        self.active_modal.replace(modal);
    }

    pub(crate) fn offer_plan_from_async(&mut self, offer: AsyncPlanOffer) -> bool {
        if self.active_modal.is_none() {
            self.sidebar.swipe.cancel();
            self.modal_list_scroll = gpui::UniformListScrollHandle::new();
            self.active_modal = Some(offer.modal);
            return true;
        }
        self.discard_contended_plan_from_async(offer.operation, offer.token);
        false
    }

    pub(crate) fn discard_contended_plan_from_async(
        &mut self,
        operation: crate::ui::i18n::Op,
        token: AsyncPlanToken,
    ) {
        if matches!(token, AsyncPlanToken::Session) {
            self.app_sessions.invalidate_plan();
        }
        self.enqueue_outcome_notice(crate::ui::i18n::plan_not_shown_retry(operation).into());
    }

    pub(crate) fn enqueue_outcome_notice(&mut self, notice: crate::ui::modals::AppNotice) {
        self.app_notices.push_back(notice);
        self.present_app_notice();
    }

    pub(super) fn update_expected_modal<T>(
        &mut self,
        update: impl FnOnce(&mut ActiveModal) -> Option<T>,
    ) -> Option<T> {
        self.active_modal.as_mut().and_then(update)
    }

    pub(crate) fn update_pull_plan_from_async(&mut self, modal: PullPlanModal) -> bool {
        self.update_expected_modal(|active| match active {
            ActiveModal::Pull(current) => {
                *current = modal;
                Some(())
            }
            _ => None,
        })
        .is_some()
    }

    pub(crate) fn update_stash_push_plan_from_async(
        &mut self,
        plan: Option<std::sync::Arc<kagi_git::ops::OperationPlan>>,
        error: Option<gpui::SharedString>,
    ) -> bool {
        self.update_expected_modal(|active| match active {
            ActiveModal::StashPush(current) => {
                current.plan = plan;
                current.error = error;
                Some(())
            }
            _ => None,
        })
        .is_some()
    }

    pub(crate) fn update_stash_apply_plan_from_async(&mut self, modal: StashApplyModal) -> bool {
        self.update_expected_modal(|active| match active {
            ActiveModal::StashApply(current) if current.index == modal.index => {
                *current = modal;
                Some(())
            }
            _ => None,
        })
        .is_some()
    }

    pub(crate) fn update_stash_pop_plan_from_async(&mut self, modal: PopPlanModal) -> bool {
        self.update_expected_modal(|active| match active {
            ActiveModal::Pop(current) if current.stash_index == modal.stash_index => {
                *current = modal;
                Some(())
            }
            _ => None,
        })
        .is_some()
    }

    pub(crate) fn update_stash_drop_plan_from_async(&mut self, modal: StashDropModal) -> bool {
        self.update_expected_modal(|active| match active {
            // `None` is a follow-up that reserved the slot before its OID
            // resolved to an index; the arriving plan is what resolves it.
            ActiveModal::StashDrop(current)
                if current.stash_index == modal.stash_index || current.stash_index.is_none() =>
            {
                *current = modal;
                Some(())
            }
            _ => None,
        })
        .is_some()
    }
}
