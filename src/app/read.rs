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
use std::sync::Arc;

use kagi_domain::load_request::RequestSlot;

use super::{AdmissionError, SessionId};

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

/// Instrumentation for the #482 stage 2 E scenario: what this store published,
/// deep-copied and freed. Per store, not process-wide — a global would make two
/// tests in one binary see each other's writes, and every consumer already has
/// the store in hand.
///
/// `copies` is the number that must stay at zero while the view only ever
/// borrows: a `get_mut` at refcount 1 mutates the shared allocation in place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadCounters {
    pub builds: u64,
    pub copies: u64,
    pub drops: u64,
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
    counters: ReadCounters,
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
            counters: ReadCounters::default(),
            empty: Arc::new(V::default()),
            sink: Arc::new(V::default()),
        }
    }

    /// What this store published, deep-copied and freed. See [`ReadCounters`].
    pub fn counters(&self) -> ReadCounters {
        self.counters
    }

    /// Store `value` for `session`, counting the read model it replaces.
    fn store(&mut self, session: SessionId, value: V) {
        self.counters.builds += 1;
        let replaced = self
            .entries
            .entry(session)
            .or_default()
            .value
            .replace(Arc::new(value));
        if replaced.is_some_and(|old| Arc::strong_count(&old) == 1) {
            self.counters.drops += 1;
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
        self.store(key.session, value);
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

    /// Whether this owner has a read at all — i.e. whether there is anything to
    /// show but the empty model. `false` before the first read lands and after
    /// one fails, which is what tells a first load from a refresh.
    pub fn has_read(&self, session: SessionId) -> bool {
        self.entries
            .get(&session)
            .is_some_and(|entry| entry.value.is_some())
    }

    /// Replace the owner's value **without touching its request state**: a
    /// refinement of the read already on screen (commit-graph paging), not a new
    /// observation of the repository. A full read still in flight therefore
    /// still lands — paging must not supersede a reload, whose snapshot also
    /// carries the conflict re-detection and the working-tree baseline that
    /// paging has no way to produce (#482 stage 2 review, item 3).
    pub fn amend(&mut self, session: SessionId, value: V) {
        self.store(session, value);
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

    /// Every open owner's reads are stale. Used at write **admission**: the
    /// lease is one global reservation across all repositories (DESIGN §2.1
    /// "conservative global busy"), so the set that a write may have invalidated
    /// is every session that is open.
    /// ponytail: narrow this to the target worktree and its siblings when
    /// admission becomes per-`RepoId`; over-invalidating only costs a re-read.
    pub fn invalidate_all(&mut self) {
        let sessions: Vec<SessionId> = self.entries.keys().copied().collect();
        for session in sessions {
            self.invalidate(session);
        }
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
        let copied = Arc::strong_count(value) > 1;
        let value = Arc::make_mut(value);
        if copied {
            self.counters.copies += 1;
        }
        value
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
            if entry
                .value
                .is_some_and(|value| Arc::strong_count(&value) == 1)
            {
                self.counters.drops += 1;
            }
        }
    }
}

/// Admission and read invalidation are one step.
///
/// Every write that is admitted makes every read that predates it suspect: the
/// reader observed a repository the writer is about to change. Routing both UI
/// admission points (`reserve_write`, `dispatch_job`) through here means neither
/// can be added to without the invalidation — the coupling ADR-0183 claims and
/// the first review found was only wired to *completion* (#482 stage 2 review,
/// item 1).
pub fn admit<T, V: Default>(
    reads: &mut Reads<V>,
    admitted: Result<T, AdmissionError>,
) -> Result<T, AdmissionError> {
    if admitted.is_ok() {
        reads.invalidate_all();
    }
    admitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Sessions;

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

    /// Item 2 of the stage-2 review, at store level: the placeholder predicate
    /// is `is_loading && !has_read`. A reload started *during* the first load
    /// refuses that first read — and must then settle the slot itself, or the
    /// predicate stays true forever.
    #[test]
    fn a_reload_during_the_first_load_still_settles_the_placeholder() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let first = reads.begin(a);
        assert!(
            reads.is_loading(a) && !reads.has_read(a),
            "placeholder shown"
        );

        let reload = reads.begin(a); // Cmd+R before the first read arrives
        assert!(
            !reads.accept(first, "first".into()),
            "the first read is stale"
        );
        assert!(
            reads.is_loading(a) && !reads.has_read(a),
            "still waiting on the reload",
        );

        assert!(reads.accept(reload, "reloaded".into()));
        assert!(
            !reads.is_loading(a) && reads.has_read(a),
            "a successful reload must clear the placeholder",
        );
        assert_eq!(reads.get(Some(a)), "reloaded");
    }

    /// The same transition the other way: the reload that replaced the first
    /// load *fails*. The placeholder still has to go — the error belongs in the
    /// footer, not behind a spinner that never stops.
    #[test]
    fn a_failing_reload_during_the_first_load_also_settles_the_placeholder() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let first = reads.begin(a);
        let reload = reads.begin(a);
        assert!(!reads.accept(first, "first".into()));
        assert!(reads.fail(reload));
        assert!(
            !reads.is_loading(a) && !reads.has_read(a),
            "a failed reload must clear the placeholder",
        );
        assert!(reads.begin(a).session() == a, "and stay re-requestable");
    }

    /// Item 3: paging refines what is on screen; it is not a fresh observation
    /// of the repository. A full reload in flight must still land, because it
    /// carries semantic processing (conflict re-detection, the working-tree
    /// baseline) that paging cannot reproduce.
    #[test]
    fn paging_does_not_supersede_a_pending_full_reload() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "page 1".into());

        let reload = reads.begin(a); // watcher reload, still in flight
        reads.amend(a, "page 1+2".into()); // Load more lands first
        assert_eq!(reads.get(Some(a)), "page 1+2");
        assert!(reads.is_loading(a), "paging cancelled the reload");
        assert!(
            reads.accept(reload, "reloaded".into()),
            "the pending full reload was refused by paging",
        );
        assert_eq!(reads.get(Some(a)), "reloaded");
    }

    /// Item 1: admission and invalidation are one step. `admit` is what both UI
    /// admission points call, so a read that predates an admitted write is
    /// refused whatever the writer family.
    #[test]
    fn an_admitted_write_invalidates_every_open_owner() {
        let (_s, a, b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        reads.publish(b, "B".into());
        let in_flight_a = reads.begin(a);
        let in_flight_b = reads.begin(b);

        let admitted: Result<(), AdmissionError> = admit(&mut reads, Ok(()));
        assert!(admitted.is_ok());
        assert!(!reads.is_fresh(in_flight_a));
        assert!(!reads.is_fresh(in_flight_b));
        assert!(!reads.accept(in_flight_a, "observed the old tree".into()));
        assert!(!reads.accept(in_flight_b, "observed the old tree".into()));
        assert_eq!(reads.get(Some(a)), "A", "the last good read is kept");
        assert_eq!(reads.get(Some(b)), "B");
    }

    /// A refused admission changes nothing: no read is invalidated by a write
    /// that never started.
    #[test]
    fn a_refused_admission_invalidates_nothing() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        let in_flight = reads.begin(a);
        let refused: Result<(), AdmissionError> = admit(&mut reads, Err(AdmissionError::Busy));
        assert!(refused.is_err());
        assert!(reads.is_fresh(in_flight));
        assert!(reads.accept(in_flight, "landed".into()));
    }

    /// Item 4: a remote re-snapshot reattaches the same tab under a new
    /// incarnation. Releasing the old owner's read is what stops every refresh
    /// from leaving a full rows/details set keyed under an id no tab names.
    #[test]
    fn reattach_releases_the_previous_incarnation_s_read() {
        let mut sessions = Sessions::new();
        let mut reads: Reads<String> = Reads::new();
        let mut session = sessions.attach("/nonexistent/remote".into());
        reads.publish(session, "snapshot 0".into());

        for round in 1..=3 {
            let drops_before = reads.counters().drops;
            let previous = session;
            reads.forget(previous); // what `enter_remote_view` must do
            session = sessions.reattach(previous, "/nonexistent/remote".into());
            let drops_after = reads.counters().drops;
            assert_ne!(session, previous, "a re-snapshot is a new incarnation");
            assert_eq!(drops_after, drops_before + 1, "round {round} leaked a read");
            assert!(reads.share(previous).is_none());
            reads.publish(session, format!("snapshot {round}"));
        }

        let before_close = reads.counters().drops;
        sessions.detach(session);
        reads.forget(session);
        assert_eq!(
            reads.counters().drops,
            before_close + 1,
            "close leaked the last read"
        );
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
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let before = reads.get(Some(a)) as *const String;
        let copies_before = reads.counters().copies;
        reads.get_mut(Some(a)).push('!');
        let copies_after = reads.counters().copies;
        assert_eq!(reads.get(Some(a)), "A!");
        assert_eq!(reads.get(Some(a)) as *const String, before);
        assert_eq!(copies_before, copies_after, "no copy-on-write deep copy");
    }

    /// A held share is exactly what forces a copy — proving the counter really
    /// measures what the no-clone claim depends on.
    #[test]
    fn a_held_share_is_what_would_copy() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let held = reads.share(a).expect("published");
        let before = reads.counters().copies;
        reads.get_mut(Some(a)).push('!');
        let after = reads.counters().copies;
        assert_eq!(after, before + 1);
        assert_eq!(&*held, "A", "the old allocation is untouched");
    }

    #[test]
    fn forget_releases_the_owner_s_read() {
        let (_s, a, _b) = two_sessions();
        let mut reads: Reads<String> = Reads::new();
        reads.publish(a, "A".into());
        let drops_before = reads.counters().drops;
        reads.forget(a);
        let drops_after = reads.counters().drops;
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
