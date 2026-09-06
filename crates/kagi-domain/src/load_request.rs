//! Request-scoped ownership of an in-flight async load (#489).
//!
//! The bug this exists to make impossible: a view that keeps `loading: bool`
//! and `data: Option<T>` as two independent fields. Change the selection while
//! a load is in flight and the two disagree — the *old* request's completion is
//! discarded for being stale (so it never clears `loading`), while the *new*
//! selection's request is suppressed by a `if loading { return; }` guard that is
//! still `true` from the request that just got thrown away. The pane sits on
//! "Loading…" forever.
//!
//! [`RequestSlot`] fixes that by making the loading state *belong to a request*
//! instead of to the slot: there is one in-flight key or none, starting a new
//! request supersedes the old one, and a completion may only clear the slot if
//! it is the request the slot is actually waiting for.
//!
//! The key is whatever identifies the request end to end — e.g.
//! `(file_generation, commit_hash)`. Pure data: no I/O, no gpui, no git2.

/// The single in-flight load for one view slot, identified by its key.
///
/// See the [module docs](self) for why the loading flag lives here rather than
/// as a bare `bool` next to the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSlot<K> {
    in_flight: Option<K>,
}

impl<K> Default for RequestSlot<K> {
    fn default() -> Self {
        Self { in_flight: None }
    }
}

impl<K: PartialEq> RequestSlot<K> {
    /// An idle slot.
    pub const fn new() -> Self {
        Self { in_flight: None }
    }

    /// Whether a request is in flight — i.e. whether the view should render
    /// "Loading…". True only while *this* slot's own request is outstanding.
    pub fn is_loading(&self) -> bool {
        self.in_flight.is_some()
    }

    /// The key of the in-flight request, if any.
    pub fn in_flight(&self) -> Option<&K> {
        self.in_flight.as_ref()
    }

    /// Take ownership of the slot for `key` and report whether the caller
    /// should actually issue the request.
    ///
    /// A different key supersedes whatever was in flight (the older
    /// completion is then a no-op — see [`Self::accept`]). The *same* key
    /// already in flight returns `false`, so a repeated ask doesn't duplicate
    /// work.
    pub fn begin(&mut self, key: K) -> bool {
        if self.in_flight.as_ref() == Some(&key) {
            return false;
        }
        self.in_flight = Some(key);
        true
    }

    /// Accept a completion for `key`.
    ///
    /// Returns `true` — and clears the loading state — only when `key` is the
    /// request this slot is waiting for. A stale completion returns `false`
    /// and leaves the newer request's loading state exactly as it was, so the
    /// caller must drop it without writing any data.
    pub fn accept(&mut self, key: &K) -> bool {
        if self.in_flight.as_ref() != Some(key) {
            return false;
        }
        self.in_flight = None;
        true
    }

    /// Abandon the in-flight request because the slot's whole subject went
    /// away (file switch, panel reset, new selection). Call this wherever the
    /// slot's *data* is cleared — the request and the data it would fill are
    /// one thing, and clearing only one of them is the bug above.
    pub fn clear(&mut self) {
        self.in_flight = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Key = (u64, &'static str);

    fn slot() -> RequestSlot<Key> {
        RequestSlot::new()
    }

    #[test]
    fn idle_slot_is_not_loading() {
        let s = slot();
        assert!(!s.is_loading());
        assert_eq!(s.in_flight(), None);
    }

    #[test]
    fn begin_marks_loading_and_asks_for_the_request() {
        let mut s = slot();
        assert!(s.begin((1, "a")));
        assert!(s.is_loading());
        assert_eq!(s.in_flight(), Some(&(1, "a")));
    }

    #[test]
    fn same_key_twice_does_not_duplicate_the_request() {
        let mut s = slot();
        assert!(s.begin((1, "a")));
        assert!(!s.begin((1, "a")));
        assert!(s.is_loading());
    }

    #[test]
    fn newest_request_wins() {
        let mut s = slot();
        s.begin((1, "a"));
        assert!(s.begin((1, "b")));
        assert_eq!(s.in_flight(), Some(&(1, "b")));
    }

    /// The #489 regression: A is in flight, the user selects B, then A's
    /// completion lands late. It must be dropped, and it must NOT clear B's
    /// loading state.
    #[test]
    fn stale_completion_is_ignored_and_leaves_the_newer_request_loading() {
        let mut s = slot();
        s.begin((1, "a"));
        s.begin((1, "b"));
        assert!(!s.accept(&(1, "a")));
        assert!(s.is_loading());
        assert_eq!(s.in_flight(), Some(&(1, "b")));
    }

    #[test]
    fn loading_clears_only_for_the_matching_request() {
        let mut s = slot();
        s.begin((1, "a"));
        s.begin((1, "b"));
        assert!(!s.accept(&(1, "a")));
        assert!(s.is_loading());
        assert!(s.accept(&(1, "b")));
        assert!(!s.is_loading());
    }

    /// The same commit under a different file generation (a file switch, or a
    /// save/watcher re-read) is a different request.
    #[test]
    fn generation_is_part_of_the_identity() {
        let mut s = slot();
        s.begin((1, "a"));
        assert!(!s.accept(&(2, "a")));
        assert!(s.is_loading());
    }

    #[test]
    fn completion_after_settling_is_a_no_op() {
        let mut s = slot();
        s.begin((1, "a"));
        assert!(s.accept(&(1, "a")));
        assert!(!s.accept(&(1, "a")));
        assert!(!s.is_loading());
    }

    /// A failed load still settles the slot, so the same key can be asked for
    /// again (acceptance criterion: failures never stick on "Loading…").
    #[test]
    fn failure_settles_and_stays_re_requestable() {
        let mut s = slot();
        s.begin((1, "a"));
        assert!(s.accept(&(1, "a"))); // caller stores `None` / an error
        assert!(!s.is_loading());
        assert!(s.begin((1, "a")));
        assert!(s.is_loading());
    }

    #[test]
    fn clear_abandons_the_in_flight_request() {
        let mut s = slot();
        s.begin((1, "a"));
        s.clear();
        assert!(!s.is_loading());
        assert!(!s.accept(&(1, "a")));
        assert!(!s.is_loading());
    }
}
