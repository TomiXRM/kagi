//! The read model's single owner (#482 stage 2).
//!
//! Before this, one repository's snapshot-derived display data existed twice:
//! `KagiApp::active_view` (the tab on screen) and `tab_cache` (every other tab),
//! with a whole-struct clone in each direction on every switch, reload, remote
//! apply and load-more. Two owners of one truth is the bug generator: a partial
//! WIP update wrote only the active copy, a solo toggle only the active copy,
//! and a background scan landing after a switch wrote the *wrong* tab's copy.
//!
//! [`Reads`] replaces both with one map keyed by [`SessionId`], so the read
//! model belongs to the session that owns the worktree. Switching tabs changes
//! which key is read — no copy of anything. The value is shared with the view as
//! an `Arc`, and because the view borrows rather than keeping a clone,
//! [`Reads::get_mut`] mutates in place (`Arc::make_mut` at refcount 1): a
//! status-only refresh no longer copies the row and detail vectors.
//!
//! Freshness is a [`RequestSlot`] per owner keyed by [`ReadKey`] — the owner's
//! `SessionId` (which carries the incarnation, so a closed-and-reopened tab is a
//! different owner) plus a monotonic read revision. Starting a read bumps the
//! revision, so a newer read supersedes an older one; admitting a mutation bumps
//! it too, so a read that observed the pre-mutation repository can no longer be
//! accepted. A stale completion is dropped without clearing the newer request's
//! loading state, and a failed read settles so the same read can be asked for
//! again (the #489 contract, reused verbatim).
//!
//! Pure data: no gpui, no git2, no I/O. `V` is a type parameter precisely so
//! this file never names a UI type — the application layer owns the *lifetime*
//! and *freshness* of the read model, the UI owns its shape.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kagi_domain::load_request::RequestSlot;

use super::SessionId;

/// Identity of one read: the owner that asked, and the revision of that owner's
/// read state the request belongs to. Carried by the in-flight job and checked
/// on completion — never re-resolved from "the tab that happens to be active".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadKey {
    session: SessionId,
    revision: u64,
}

impl ReadKey {
    pub fn session(&self) -> SessionId {
        self.session
    }
}

// ── Instrumentation (#482 stage 2 E scenario) ─────────────────────────────
// Three process-wide counters, read by the GUI E2E scenario to prove the
// no-deep-clone claim rather than assert it in prose. Relaxed atomics on paths
// that already allocate a snapshot: not a hot-path cost.
static BUILDS: AtomicU64 = AtomicU64::new(0);
static COPIES: AtomicU64 = AtomicU64::new(0);
static DROPS: AtomicU64 = AtomicU64::new(0);

/// `(published reads, copy-on-write deep copies, read models freed)`.
///
/// `copies` is the number that must stay at zero while the view only ever
/// borrows: a `get_mut` at refcount 1 mutates the shared allocation in place.
pub fn read_counters() -> (u64, u64, u64) {
    (
        BUILDS.load(Ordering::Relaxed),
        COPIES.load(Ordering::Relaxed),
        DROPS.load(Ordering::Relaxed),
    )
}

/// One owner's read state: the value, its in-flight request, and the revision
/// that binds the two.
struct Entry<V> {
    value: Option<Arc<V>>,
    slot: RequestSlot<ReadKey>,
    revision: u64,
}

impl<V> Default for Entry<V> {
    fn default() -> Self {
        Self {
            value: None,
            slot: RequestSlot::new(),
            revision: 0,
        }
    }
}

/// Every attached session's read model, and the freshness state that decides
/// which completions may write into it.
pub struct Reads<V> {
    entries: HashMap<SessionId, Entry<V>>,
    /// Returned by [`Reads::get`] when nothing is attached (Welcome) or the
    /// owner's first read has not landed yet. Never mutated.
    empty: Arc<V>,
    /// ponytail: a write with no attached session has no owner to write to, and
    /// forcing ~10 call sites into `if let Some(view)` buys nothing — it lands
    /// here instead, where nothing reads it.
    sink: Arc<V>,
}

impl<V: Default> Default for Reads<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Default> Reads<V> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            empty: Arc::new(V::default()),
            sink: Arc::new(V::default()),
        }
    }

    /// Start a read for `session`, superseding whatever was in flight for it.
    /// The returned key is what the completion must present.
    pub fn begin(&mut self, session: SessionId) -> ReadKey {
        let entry = self.entries.entry(session).or_default();
        entry.revision += 1;
        let key = ReadKey {
            session,
            revision: entry.revision,
        };
        entry.slot.begin(key);
        key
    }

    /// Whether this owner's own read is outstanding — i.e. whether the view
    /// should render "Loading…" for it.
    pub fn is_loading(&self, session: SessionId) -> bool {
        self.entries
            .get(&session)
            .is_some_and(|entry| entry.slot.is_loading())
    }

    /// Whether `key` still describes its owner's current read revision.
    ///
    /// Used by reads that deliberately do **not** own the slot — the cheap
    /// working-tree/WIP refresh, which is a subset of a full reload and must
    /// never invalidate one, but must itself be dropped when a full reload (or
    /// a mutation) moved the revision under it.
    pub fn is_fresh(&self, key: ReadKey) -> bool {
        self.entries
            .get(&key.session)
            .is_some_and(|entry| entry.revision == key.revision)
    }

    /// A key naming this owner's *current* read revision, without starting a
    /// read. For the cheap refreshes that ride alongside the full read
    /// ([`Reads::is_fresh`]) rather than superseding it.
    pub fn current_key(&self, session: SessionId) -> ReadKey {
        ReadKey {
            session,
            revision: self.revision(session),
        }
    }

    /// Store a completed read. Returns `false` — writing nothing — when the read
    /// was superseded, and leaves the newer request's loading state intact.
    pub fn accept(&mut self, key: ReadKey, value: V) -> bool {
        let Some(entry) = self.entries.get_mut(&key.session) else {
            return false;
        };
        if !entry.slot.accept(&key) {
            return false;
        }
        BUILDS.fetch_add(1, Ordering::Relaxed);
        if let Some(old) = entry.value.replace(Arc::new(value)) {
            count_release(&old);
        }
        true
    }

    /// Settle a failed read so its owner can ask for the same thing again
    /// (a failure must never stick the view on "Loading…").
    pub fn fail(&mut self, key: ReadKey) -> bool {
        self.entries
            .get_mut(&key.session)
            .is_some_and(|entry| entry.slot.accept(&key))
    }

    /// Publish a read that was built synchronously (pre-launch bootstrap, a
    /// remote snapshot, load-more). Supersedes anything in flight for the owner.
    pub fn publish(&mut self, session: SessionId, value: V) {
        let key = self.begin(session);
        self.accept(key, value);
    }

    /// A mutation was admitted (or its plan invalidated) against this owner:
    /// every read that observed the pre-mutation repository is now stale. The
    /// value stays — showing the last good read beats showing nothing — but no
    /// in-flight completion may land on top of the reload that follows.
    pub fn invalidate(&mut self, session: SessionId) {
        let entry = self.entries.entry(session).or_default();
        entry.revision += 1;
        entry.slot.clear();
    }

    /// The read model on screen for `session`, or the empty one when nothing is
    /// attached / the first read has not landed.
    pub fn get(&self, session: Option<SessionId>) -> &V {
        session
            .and_then(|session| self.entries.get(&session))
            .and_then(|entry| entry.value.as_deref())
            .unwrap_or(&self.empty)
    }

    /// Mutable access for the in-place updates that are *not* a whole new read:
    /// a status-only WIP refresh, a solo toggle, the branch-cleanup scan's rows,
    /// the squash ghost edges. `Arc::make_mut` at refcount 1 — which is the
    /// steady state, since the view borrows and never keeps a clone — writes
    /// through to the shared allocation instead of copying it.
    pub fn get_mut(&mut self, session: Option<SessionId>) -> &mut V
    where
        V: Clone,
    {
        let value = match session {
            Some(session) => self
                .entries
                .entry(session)
                .or_default()
                .value
                .get_or_insert_with(|| Arc::new(V::default())),
            None => &mut self.sink,
        };
        if Arc::strong_count(value) > 1 {
            COPIES.fetch_add(1, Ordering::Relaxed);
        }
        Arc::make_mut(value)
    }

    /// A shared handle on one owner's read model. The single-owner rule holds
    /// because nothing keeps one of these across a frame — it exists so the E2E
    /// scenario can compare allocation identity across a tab switch.
    pub fn share(&self, session: SessionId) -> Option<Arc<V>> {
        self.entries.get(&session)?.value.clone()
    }

    /// The owner's read revision, for tests and for callers that want to prove
    /// a transition happened.
    pub fn revision(&self, session: SessionId) -> u64 {
        self.entries
            .get(&session)
            .map(|entry| entry.revision)
            .unwrap_or(0)
    }

    /// The display went away (tab close / detach): drop this owner's read model
    /// and abandon its in-flight read. Closing a tab is not cancelling an
    /// execution — this drops the *read*, nothing else (ADR-0175).
    pub fn forget(&mut self, session: SessionId) {
        if let Some(entry) = self.entries.remove(&session) {
            if let Some(value) = entry.value {
                count_release(&value);
            }
        }
    }
}

/// Count a read model whose last reference is about to go away.
fn count_release<V>(value: &Arc<V>) {
    if Arc::strong_count(value) == 1 {
        DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Sessions;

    /// The counters are process-wide, so the three tests that assert a *delta*
    /// must not run concurrently with each other.
    static COUNTERS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn two_sessions() -> (Sessions, SessionId, SessionId) {
        let mut sessions = Sessions::new();
        // Paths that do not resolve to a worktree still get distinct sessions,
        // which is all these transitions need.
        let a = sessions.attach("/nonexistent/a".into());
        let b = sessions.attach("/nonexistent/b".into());
        (sessions, a, b)
    }

    #[test]
    fn nothing_attached_reads_the_empty_model() {
        let reads: Reads<String> = Reads::new();
        assert_eq!(reads.get(None), "");
    }

    #[test]
    fn a_read_lands_on_its_own_owner() {
        let (_s, a, b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let key = reads.begin(a);
        assert!(reads.accept(key, "A".into()));
        assert_eq!(reads.get(Some(a)), "A");
        assert_eq!(reads.get(Some(b)), "", "B has no read of its own");
    }

    /// A and B load concurrently and complete out of order. Each lands on its
    /// own owner: neither can be dropped for "not being the active tab", and
    /// neither can overwrite the other.
    #[test]
    fn out_of_order_completion_keeps_both_owners() {
        let (_s, a, b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let key_a = reads.begin(a);
        let key_b = reads.begin(b);
        assert!(reads.accept(key_b, "B".into()));
        assert!(reads.accept(key_a, "A".into()));
        assert_eq!(reads.get(Some(a)), "A");
        assert_eq!(reads.get(Some(b)), "B");
    }

    /// The #489 shape at owner scope: a second read for the *same* owner
    /// supersedes the first, and the first's late completion neither writes data
    /// nor clears the newer read's loading state.
    #[test]
    fn same_owner_stale_revision_is_dropped_and_leaves_loading() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let old = reads.begin(a);
        let new = reads.begin(a);
        assert!(!reads.accept(old, "stale".into()));
        assert_eq!(reads.get(Some(a)), "", "a stale read wrote nothing");
        assert!(reads.is_loading(a), "the newer read is still loading");
        assert!(reads.accept(new, "fresh".into()));
        assert!(!reads.is_loading(a));
        assert_eq!(reads.get(Some(a)), "fresh");
    }

    /// A full reload beats an older cheap WIP read: the reload bumps the
    /// revision, so the WIP result that observed the pre-reload tree is refused.
    #[test]
    fn full_reload_beats_an_older_wip_read() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "loaded".into());
        let wip = ReadKey {
            session: a,
            revision: reads.revision(a),
        };
        assert!(reads.is_fresh(wip));
        let reload = reads.begin(a);
        assert!(!reads.is_fresh(wip), "the reload superseded the WIP read");
        assert!(reads.accept(reload, "reloaded".into()));
        assert_eq!(reads.get(Some(a)), "reloaded");
    }

    /// Admitting a mutation invalidates every read that observed the repository
    /// before it, without blanking the display.
    #[test]
    fn mutation_admission_invalidates_in_flight_reads() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "before".into());
        let in_flight = reads.begin(a);
        reads.invalidate(a);
        assert!(!reads.is_fresh(in_flight));
        assert!(!reads.accept(in_flight, "observed the old tree".into()));
        assert_eq!(reads.get(Some(a)), "before", "the last good read is kept");
        assert!(!reads.is_loading(a));
    }

    #[test]
    fn a_failed_read_settles_and_can_be_re_requested() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let first = reads.begin(a);
        assert!(reads.fail(first));
        assert!(!reads.is_loading(a), "a failure never sticks on loading");
        let retry = reads.begin(a);
        assert!(reads.accept(retry, "ok".into()));
        assert_eq!(reads.get(Some(a)), "ok");
    }

    /// A→B→A changes which key is read, and nothing else: same allocation, no
    /// copy. This is the property `active_view`/`tab_cache` could not have.
    #[test]
    fn switching_back_returns_the_same_allocation() {
        let (_s, a, b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        reads.publish(b, "B".into());
        let before = reads.get(Some(a)) as *const String;
        let _ = reads.get(Some(b));
        let after = reads.get(Some(a)) as *const String;
        assert_eq!(before, after);
    }

    #[test]
    fn in_place_update_does_not_copy() {
        let _guard = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let before = reads.get(Some(a)) as *const String;
        let (_, copies_before, _) = read_counters();
        reads.get_mut(Some(a)).push('!');
        let (_, copies_after, _) = read_counters();
        assert_eq!(reads.get(Some(a)), "A!");
        assert_eq!(reads.get(Some(a)) as *const String, before);
        assert_eq!(copies_before, copies_after, "no copy-on-write deep copy");
    }

    /// A held share is exactly what forces a copy — proving the counter really
    /// measures what the no-clone claim depends on.
    #[test]
    fn a_held_share_is_what_would_copy() {
        let _guard = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let held = reads.share(a).expect("published");
        let (_, before, _) = read_counters();
        reads.get_mut(Some(a)).push('!');
        let (_, after, _) = read_counters();
        assert_eq!(after, before + 1);
        assert_eq!(&*held, "A", "the old allocation is untouched");
    }

    #[test]
    fn forget_releases_the_owner_s_read() {
        let _guard = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let (_, _, drops_before) = read_counters();
        reads.forget(a);
        let (_, _, drops_after) = read_counters();
        assert_eq!(drops_after, drops_before + 1, "last reference released");
        assert_eq!(reads.get(Some(a)), "");
        assert!(reads.share(a).is_none());
    }

    /// A closed-and-reopened tab is a different owner, so nothing the old
    /// incarnation read can be shown for the new one.
    #[test]
    fn a_reopened_tab_does_not_inherit_the_old_read() {
        let mut sessions = Sessions::new();
        let first = sessions.attach("/nonexistent/a".into());
        let mut reads: Reads<String> = Reads::new();
        reads.publish(first, "A".into());
        sessions.detach(first);
        reads.forget(first);
        let second = sessions.attach("/nonexistent/a".into());
        assert_ne!(first, second);
        assert_eq!(reads.get(Some(second)), "");
    }
}
