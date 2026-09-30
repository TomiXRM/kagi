//! #355 (ADR-0086 amendment): explaining a slow read in the busy snackbar.
//!
//! A tracked read registers here when it starts and hands its background work
//! a [`ReadHandle`]. Dropping the handle — however the work ends: a result, an
//! error, a panic, a dropped task — ends the read, so no lost completion can
//! leave the explanation up (#289's lesson). A per-read ticker on the UI
//! executor re-checks it every [`TICK`]; once it has spent
//! [`SLOW_READ_THRESHOLD`] in an explained phase the snackbar names what is
//! slow and why and, when the result has an "unknown" rendering to fall back
//! to, offers Skip. Skip ends that part of the read — it is not an operation
//! cancel, and it is not recorded in the oplog.
//!
//! [`SLOW_READ_THRESHOLD`]: kagi_ui_core::slow_read::SLOW_READ_THRESHOLD

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::Context;
use kagi_git::{SnapshotPhase, SnapshotProbe};
use kagi_ui_core::slow_read::{is_slow, SlowRead};

use super::KagiApp;

/// How often an in-flight read is re-checked.
const TICK: Duration = Duration::from_millis(250);

/// Held by a read's background work; its drop ends the read.
pub(super) struct ReadHandle(Arc<ReadState>);

impl ReadHandle {
    /// The probe a snapshot read reports its phase through and reads Skip from.
    pub(super) fn probe(&self) -> &SnapshotProbe {
        &self.0.probe
    }
}

impl Drop for ReadHandle {
    fn drop(&mut self) {
        self.0.finished.store(true, Ordering::Relaxed);
    }
}

struct ReadState {
    /// `None` for a snapshot, whose explained part follows its phase.
    fixed: Option<SlowRead>,
    probe: SnapshotProbe,
    finished: AtomicBool,
}

impl ReadState {
    fn current(&self) -> Option<SlowRead> {
        self.fixed.or(match self.probe.phase() {
            SnapshotPhase::Worktrees => Some(SlowRead::Worktrees),
            SnapshotPhase::AheadBehind => Some(SlowRead::AheadBehind),
            SnapshotPhase::Other => None,
        })
    }
}

struct Tracked {
    state: Arc<ReadState>,
    /// The explained part running as of the last tick, and since when.
    kind: Option<SlowRead>,
    since: Instant,
    /// This part crossed the threshold (and was logged).
    slow: bool,
    /// The user skipped this part; it is no longer explained.
    skipped: bool,
}

/// One session's tracked reads (lives in `TabUiState`).
#[derive(Default)]
pub(super) struct SlowReads {
    reads: Vec<Tracked>,
}

impl SlowReads {
    fn begin(&mut self, fixed: Option<SlowRead>, now: Instant) -> ReadHandle {
        let state = Arc::new(ReadState {
            fixed,
            probe: SnapshotProbe::default(),
            finished: AtomicBool::new(false),
        });
        self.reads.push(Tracked {
            kind: state.current(),
            state: state.clone(),
            since: now,
            slow: false,
            skipped: false,
        });
        ReadHandle(state)
    }

    /// Drop ended reads and follow phase changes. Returns whether what the
    /// snackbar shows may have changed, and the parts that just became slow.
    fn tick(&mut self, now: Instant) -> (bool, Vec<SlowRead>) {
        let before = self.explained();
        self.reads
            .retain(|read| !read.state.finished.load(Ordering::Relaxed));
        let mut newly_slow = Vec::new();
        for read in &mut self.reads {
            let kind = read.state.current();
            if kind != read.kind {
                read.kind = kind;
                read.since = now;
                read.slow = false;
                read.skipped = false;
            }
            if let Some(kind) = read.kind {
                if !read.slow && is_slow(now.saturating_duration_since(read.since)) {
                    read.slow = true;
                    newly_slow.push(kind);
                }
            }
        }
        (self.explained() != before, newly_slow)
    }

    /// The part to explain: the oldest slow, unskipped read.
    pub(super) fn explained(&self) -> Option<SlowRead> {
        self.reads
            .iter()
            .find(|read| read.slow && !read.skipped)
            .and_then(|read| read.kind)
    }

    /// Skip every slow read of `kind`: a snapshot stops counting ahead/behind.
    fn skip(&mut self, kind: SlowRead) {
        for read in &mut self.reads {
            if read.slow && read.kind == Some(kind) {
                read.skipped = true;
                if kind == SlowRead::AheadBehind {
                    read.state.probe.skip_ahead_behind();
                }
            }
        }
    }
}

impl KagiApp {
    /// Track a read for `owner`; `fixed` names it, `None` is a snapshot. Keep
    /// the handle alive for exactly as long as the read's work runs.
    pub(super) fn begin_slow_read(
        &mut self,
        owner: crate::app::SessionId,
        fixed: Option<SlowRead>,
        cx: &mut Context<Self>,
    ) -> ReadHandle {
        let now = cx.background_executor().now();
        let handle = match self.ui.get_mut(&owner) {
            Some(ui) => ui.slow_reads.begin(fixed, now),
            None => SlowReads::default().begin(fixed, now),
        };
        let state = handle.0.clone();
        cx.spawn(async move |this, acx| loop {
            acx.background_executor().timer(TICK).await;
            let finished = state.finished.load(Ordering::Relaxed);
            let alive = this
                .update(acx, |app, cx| app.tick_slow_reads(owner, cx))
                .is_ok();
            if finished || !alive {
                break;
            }
        })
        .detach();
        handle
    }

    fn tick_slow_reads(&mut self, owner: crate::app::SessionId, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        let (changed, newly_slow) = ui.slow_reads.tick(now);
        for kind in newly_slow {
            klog!("busy: slow {} after 2s", kind.tag());
        }
        if changed && self.active_session() == Some(owner) {
            cx.notify();
        }
    }

    /// The slow read the busy snackbar explains, for the tab on screen.
    pub(super) fn slow_read_shown(&self) -> Option<SlowRead> {
        self.ui().slow_reads.explained()
    }

    /// The snackbar's Skip: show the rest of the read's result as unknown.
    pub(super) fn skip_slow_read(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.slow_read_shown().filter(|kind| kind.skippable()) else {
            return;
        };
        let Some(ui) = self.ui_mut() else {
            return;
        };
        ui.slow_reads.skip(kind);
        if kind == SlowRead::WorktreeSize {
            // The inspection's own cancel: unmeasured worktrees read "not
            // measured", and the next sweep measures them again.
            ui.worktree_inspections.cancel();
        }
        klog!("busy: skip {}", kind.tag());
        cx.notify();
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static SNAPSHOT_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The hold queued for the next reload's snapshot, if any (Tier A only).
#[cfg(feature = "gui-e2e")]
pub(super) fn take_snapshot_hold() -> Option<gpui::Task<()>> {
    SNAPSHOT_HOLD.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next reload's snapshot in its ahead/behind phase until `hold`
    /// completes; the real snapshot then runs, honouring a Skip made meanwhile.
    pub fn hold_next_snapshot_for_e2e(hold: gpui::Task<()>) {
        SNAPSHOT_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }

    /// Tag of the slow read the busy snackbar explains on screen.
    pub fn slow_read_shown_for_e2e(&self) -> Option<&'static str> {
        self.slow_read_shown().map(SlowRead::tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read is explained only once its explained part has run for the
    /// threshold, and never after its handle is dropped.
    #[test]
    fn explained_after_the_threshold_and_until_the_read_ends() {
        let start = Instant::now();
        let mut reads = SlowReads::default();
        let handle = reads.begin(Some(SlowRead::Analyze), start);

        let (_, slow) = reads.tick(start + Duration::from_millis(1_999));
        assert!(slow.is_empty());
        assert_eq!(reads.explained(), None);

        let (changed, slow) = reads.tick(start + Duration::from_secs(2));
        assert!(changed);
        assert_eq!(slow, vec![SlowRead::Analyze]);
        assert_eq!(reads.explained(), Some(SlowRead::Analyze));
        // Logged once, not every tick.
        assert!(reads.tick(start + Duration::from_secs(3)).1.is_empty());

        drop(handle);
        let (changed, _) = reads.tick(start + Duration::from_secs(4));
        assert!(changed);
        assert_eq!(reads.explained(), None);
    }

    /// A snapshot is explained by its current phase, timed from when that
    /// phase began; Skip reaches the probe and hides the explanation.
    #[test]
    fn a_snapshot_is_timed_per_phase_and_skip_reaches_the_probe() {
        let start = Instant::now();
        let mut reads = SlowReads::default();
        let handle = reads.begin(None, start);

        // Three seconds of an unexplained phase explain nothing.
        assert!(reads.tick(start + Duration::from_secs(3)).1.is_empty());
        handle.probe().set_phase(SnapshotPhase::AheadBehind);
        assert!(reads.tick(start + Duration::from_secs(3)).1.is_empty());
        assert!(reads
            .tick(start + Duration::from_millis(4_999))
            .1
            .is_empty());
        assert_eq!(
            reads.tick(start + Duration::from_secs(5)).1,
            vec![SlowRead::AheadBehind]
        );

        reads.skip(SlowRead::AheadBehind);
        assert!(handle.probe().ahead_behind_skipped());
        assert_eq!(reads.explained(), None);
    }
}
