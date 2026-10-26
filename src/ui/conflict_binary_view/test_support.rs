//! Feature-only scheduling of real preparation results; no synthetic media.
use super::{ConflictView, PreviewState};
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

thread_local! {
    static NEXT_HOLD: RefCell<Option<CompletionHold>> = const { RefCell::new(None) };
}

pub(super) struct CompletionHold {
    task: gpui::Task<()>,
    status: Arc<AtomicU8>,
}

impl CompletionHold {
    pub(super) fn reached(&self) {
        self.status.store(1, Ordering::SeqCst);
    }

    pub(super) async fn wait(self) -> Arc<AtomicU8> {
        self.task.await;
        self.status
    }
}

pub(super) fn take_hold() -> Option<CompletionHold> {
    NEXT_HOLD.with(|slot| slot.borrow_mut().take())
}

pub(super) fn record_completion(status: Option<Arc<AtomicU8>>, accepted: bool) {
    if let Some(status) = status {
        status.store(if accepted { 2 } else { 3 }, Ordering::SeqCst);
    }
}

impl ConflictView {
    /// 0: queued, 1: real worker completed, 2: adopted, 3: rejected as stale.
    pub fn hold_next_raw_preview_for_e2e(task: gpui::Task<()>) -> Arc<AtomicU8> {
        let status = Arc::new(AtomicU8::new(0));
        NEXT_HOLD.with(|slot| {
            assert!(slot
                .borrow_mut()
                .replace(CompletionHold {
                    task,
                    status: status.clone(),
                })
                .is_none());
        });
        status
    }

    /// Let a genuine old completion reach the production admission checks even
    /// after its owner retires, exercising correctness independently of cancel.
    pub fn detach_raw_preview_for_e2e(&mut self) {
        self.raw_preview
            .task
            .take()
            .expect("preparation admitted")
            .detach();
    }

    pub fn retire_raw_preview_for_e2e(&mut self, cx: &mut gpui::App) {
        self.retire_raw_preview(cx);
    }

    pub fn raw_preview_state_for_e2e(&self) -> (&'static str, u8) {
        let state = match self.raw_preview.state {
            PreviewState::Loading => "loading",
            PreviewState::Ready(_) => "ready",
            PreviewState::Failed(_) => "failed",
        };
        (
            state,
            self.raw_preview
                .failed
                .as_ref()
                .map_or(0, |(_, attempts)| *attempts),
        )
    }

    pub fn raw_preview_identity_for_e2e(&self) -> Option<usize> {
        match &self.raw_preview.state {
            PreviewState::Ready(pair) => Some(Arc::as_ptr(pair) as usize),
            _ => None,
        }
    }
}
