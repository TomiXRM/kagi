//! Delay real inspection results without leaking a pending task across scenarios.
use super::*;
use kagi::ui::oplog_panel::OpLogPanel;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) struct LoadHold {
    release: Option<crate::evidence_support::Reply<()>>,
    reached: Arc<AtomicBool>,
}

impl LoadHold {
    pub(super) fn new(cx: &mut VisualTestAppContext) -> Self {
        let (hold, release) = crate::evidence_support::deferred(cx);
        let reached = OpLogPanel::hold_next_preserved_conflict_draft_for_e2e(hold);
        Self {
            release: Some(release),
            reached,
        }
    }

    pub(super) fn wait(&self, cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
        crate::busy_dismiss::wait(
            cx,
            app,
            "real preserved-draft read reaches its admission boundary",
            |_| self.reached.load(Ordering::Acquire),
        );
    }
}

impl Drop for LoadHold {
    fn drop(&mut self) {
        // Clear a hook that never started, and release an already-consumed hook
        // even if a consumer assertion unwinds before its explicit release.
        OpLogPanel::clear_preserved_conflict_draft_hold_for_e2e();
        if let Some(release) = self.release.take() {
            release.send(());
        }
    }
}

pub(super) fn pending_language_drafts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &str,
) -> String {
    let old = LoadHold::new(cx);
    click_control(cx, window, "oplog-preserved-drafts-open");
    old.wait(cx, app);
    paint(cx, app, window);
    for language in [Lang::Ja, Lang::En] {
        app.update(cx, |app, cx| app.set_lang(language, cx));
        paint(cx, app, window);
        assert_eq!(
            draft_text(cx, app),
            None,
            "the real result is still pending"
        );
        assert!(e2e::control_bounds(window.window_id(), "oplog-preserved-drafts-back").is_some());
    }
    back(cx, app, window);
    let current = LoadHold::new(cx);
    click_control(cx, window, "oplog-preserved-drafts-open");
    current.wait(cx, app);
    paint(cx, app, window);
    for language in [Lang::Ja, Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(language, cx));
        paint(cx, app, window);
        assert_eq!(draft_text(cx, app), None);
    }
    // Same Attachment, newer inspection: the old real result cannot settle it.
    drop(old);
    cx.run_until_parked();
    assert_eq!(
        draft_text(cx, app),
        None,
        "old generation cannot replace Loading"
    );
    let panel = panel(cx, app);
    assert!(!cx.read(|cx| panel.read(cx).preserved_conflict_draft_unavailable()));
    drop(current);
    wait_drafts(cx, app, window, expected)
}
