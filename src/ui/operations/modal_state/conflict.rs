//! Conflict modal accessors (sequencer continue, abort) over the single
//! `active_modal` field.
//!
//! Split out of `modal_state.rs` on a feature boundary (#707 review): these
//! belong to the one owner of `active_modal`, not to the feature module that
//! opens them. `modals::*` re-exports the state structs themselves, which stay
//! with the action in `ui::conflict_abort`.

use super::super::super::conflict_abort::{ConflictAbortModal, ConflictContinuePlanModal};
use super::super::super::modals::ActiveModal;
use super::super::super::KagiApp;

impl KagiApp {
    #[inline]
    pub fn conflict_continue_modal(&self) -> Option<&ConflictContinuePlanModal> {
        match &self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn conflict_continue_modal_mut(&mut self) -> Option<&mut ConflictContinuePlanModal> {
        match &mut self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_conflict_continue_modal(&mut self, m: ConflictContinuePlanModal) {
        self.active_modal = Some(ActiveModal::ConflictContinue(m));
    }
    #[inline]
    pub fn clear_conflict_continue_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ConflictContinue(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn conflict_abort_modal(&self) -> Option<&ConflictAbortModal> {
        match &self.active_modal {
            Some(ActiveModal::ConflictAbort(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn conflict_abort_modal_mut(&mut self) -> Option<&mut ConflictAbortModal> {
        match &mut self.active_modal {
            Some(ActiveModal::ConflictAbort(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_conflict_abort_modal(&mut self, m: ConflictAbortModal) {
        self.active_modal = Some(ActiveModal::ConflictAbort(m));
    }
    #[inline]
    pub fn clear_conflict_abort_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ConflictAbort(_))) {
            self.active_modal = None;
        }
    }
}
