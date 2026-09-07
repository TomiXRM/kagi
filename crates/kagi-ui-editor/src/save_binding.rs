//! Binding a completed background save to the buffer it was issued from
//! (#486).
//!
//! A save is started from one buffer and lands, milliseconds later, on
//! whatever the view happens to hold by then. Applying `dirty = false` plus
//! the saved text unconditionally is a data-loss path: switch tabs while the
//! write is in flight and the *other* file is marked clean, so its unsaved
//! edits are silently discarded by the next reload, tab close, or workspace
//! close (all of which only guard on `dirty`).
//!
//! Two facts pin a completion down:
//!
//! * **Which buffer** — the repo-relative path plus a monotonic *generation*
//!   ([`deliver`]). The generation is what makes close → re-open of the same
//!   path a different buffer, which a path match alone cannot see.
//! * **Which edit** — the buffer's live text at completion time, compared
//!   against the bytes actually written ([`dirty_after_save`]). That is the
//!   same rule the rest of the pane uses for `dirty` (live text vs. the
//!   loaded/saved snapshot), so an edit made *after* the save started keeps
//!   the buffer dirty — and an edit undone back to the written text correctly
//!   goes clean. A revision *counter* captured at save time would be a proxy
//!   for this comparison; comparing the text is both smaller and exact.
//!
//! Pure — no gpui, no I/O — so the rules above are unit-tested directly.

use std::path::Path;

/// Which buffer, if any, a completed save may be applied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveDelivery {
    /// The saving buffer is still the active one: its content/dirty/banner
    /// fields are the completion's target.
    Active,
    /// The saving buffer was stashed into `tab_cache` by a tab switch. Its
    /// cached state is the target; the visible buffer must not be touched.
    Cached,
    /// The buffer is gone — closed, or re-opened as a new generation. The
    /// write still happened on disk (the caller logs/toasts it), but no view
    /// state may change.
    Dropped,
}

/// Route a completed save to the buffer that issued it.
///
/// `saved` is the `(path, generation)` captured when the save started,
/// `active` the active buffer's `(path, generation)` now, and
/// `cached_generation` the generation of the buffer stashed under **the saved
/// path** (`None` if no tab holds it). Generations come from one monotonic
/// allocator, so an equal generation already implies the same buffer.
pub(crate) fn deliver(
    saved: (&Path, u64),
    active: Option<(&Path, u64)>,
    cached_generation: Option<u64>,
) -> SaveDelivery {
    if active == Some(saved) {
        SaveDelivery::Active
    } else if cached_generation == Some(saved.1) {
        SaveDelivery::Cached
    } else {
        SaveDelivery::Dropped
    }
}

/// Whether the buffer a save landed on still holds unsaved edits: true when
/// its live text is no longer the text that was written. `None` means the
/// buffer has no editor yet, so there is nothing unsaved.
pub(crate) fn dirty_after_save(written: &str, live: Option<&str>) -> bool {
    live.is_some_and(|t| t != written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn unchanged_buffer_takes_the_completion() {
        let a = p("a.rs");
        assert_eq!(deliver((&a, 7), Some((&a, 7)), None), SaveDelivery::Active);
    }

    /// #486 acceptance 1: A is saving, the user switches to a dirty B. B is
    /// active at completion time — nothing about it may change.
    #[test]
    fn completion_for_another_buffer_never_lands_on_the_active_one() {
        let (a, b) = (p("a.rs"), p("b.rs"));
        // B is active and A is not open at all (its tab was closed).
        assert_eq!(deliver((&a, 7), Some((&b, 8)), None), SaveDelivery::Dropped);
        // The usual case: A is still open, stashed by the tab switch.
        assert_eq!(
            deliver((&a, 7), Some((&b, 8)), Some(7)),
            SaveDelivery::Cached
        );
    }

    /// #486 acceptance 4: close → re-open of the same path is a different
    /// buffer, so an in-flight completion must not update the new one.
    #[test]
    fn same_path_reopened_is_a_different_buffer() {
        let a = p("a.rs");
        assert_eq!(deliver((&a, 7), Some((&a, 9)), None), SaveDelivery::Dropped);
        // A *stale* tab under the same path can't claim it either.
        assert_eq!(
            deliver((&a, 7), Some((&a, 9)), Some(9)),
            SaveDelivery::Dropped
        );
    }

    /// A completion whose buffer is neither active nor cached is dropped —
    /// the tab was closed while the write was in flight.
    #[test]
    fn closed_buffer_drops_the_completion() {
        let a = p("a.rs");
        assert_eq!(deliver((&a, 7), None, None), SaveDelivery::Dropped);
    }

    /// #486 acceptance 5: saved exactly what the buffer holds → clean.
    #[test]
    fn buffer_that_still_holds_the_written_text_goes_clean() {
        assert!(!dirty_after_save("hello", Some("hello")));
        assert!(!dirty_after_save("hello", None));
    }

    /// #486 acceptance 2: an edit made after the save started keeps the
    /// buffer dirty, so the close/reload guards still protect it.
    #[test]
    fn edit_after_the_save_started_keeps_the_buffer_dirty() {
        assert!(dirty_after_save("hello", Some("hello!")));
    }

    /// Undo back to the written text is genuinely clean — the disk and the
    /// buffer agree. A revision counter would have called this dirty.
    #[test]
    fn edit_undone_back_to_the_written_text_is_clean() {
        assert!(!dirty_after_save("hello", Some("hello")));
    }
}
