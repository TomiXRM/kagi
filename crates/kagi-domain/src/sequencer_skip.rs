//! How to read the repository after `git <op> --skip` (#540) — pure, no git2.
//!
//! `git rebase --skip` drops the current pick and keeps replaying. If the very
//! next commit also conflicts, git stops there and exits **non-zero** — the skip
//! itself did exactly what was asked. Classifying on the exit code alone records
//! that as a failure while the UI is already showing the next conflict session.
//!
//! So the outcome is decided by the repository state *after* the command, the
//! way `execute_conflict_continue` already decides it (#296): is the sequencer
//! still in flight, are there unmerged entries, and did the replay position
//! actually move.

/// What the repository looked like after `git <op> --skip` returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkipObservation {
    /// The command exited zero.
    pub ok: bool,
    /// The sequencer is still in progress (`RepositoryState` is not `Clean`).
    pub in_progress: bool,
    /// A freshly-read index still has unmerged entries.
    pub unmerged: bool,
    /// The commit being replayed (`REBASE_HEAD` / `CHERRY_PICK_HEAD` /
    /// `REVERT_HEAD`) or the rebase step counter moved — the sequencer really
    /// left the step we asked it to skip.
    pub replay_advanced: bool,
}

/// The classified result of a skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipProgress {
    /// The step was dropped and the whole sequence ran to completion.
    Finished,
    /// The step was dropped and the sequencer moved on, stopping again —
    /// normally at the next conflicting commit. This is a success, not a
    /// failure, even though git exits non-zero when it stops on a conflict.
    Advanced,
    /// Nothing moved: the same step is still the current one. A real failure.
    NoProgress,
    /// The command reported an error and the state does not prove either
    /// outcome. Never guess in this direction — the caller records `Unknown`.
    Unclear,
}

/// Classify a finished `git <op> --skip` from the repository state it left.
///
/// The exit code is evidence, not the verdict: it only decides the ambiguous
/// cases where the state alone cannot tell progress from a refusal.
pub fn classify_skip(o: SkipObservation) -> SkipProgress {
    // "Still something in flight" — either the sequencer or a conflicted index.
    if !(o.in_progress || o.unmerged) {
        return if o.ok {
            SkipProgress::Finished
        } else {
            // Out of the sequence, but git complained: we cannot say the step
            // was dropped cleanly.
            SkipProgress::Unclear
        };
    }
    if o.ok || (o.replay_advanced && o.unmerged) {
        SkipProgress::Advanced
    } else if !o.replay_advanced {
        SkipProgress::NoProgress
    } else {
        SkipProgress::Unclear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #540: `rebase --skip` stops at the next conflicting commit with exit 1.
    #[test]
    fn advanced_to_next_conflict_is_not_a_failure() {
        assert_eq!(
            classify_skip(SkipObservation {
                ok: false,
                in_progress: true,
                unmerged: true,
                replay_advanced: true,
            }),
            SkipProgress::Advanced
        );
    }

    #[test]
    fn clean_exit_still_mid_sequence_is_advanced() {
        // e.g. an interactive rebase stopping at `edit` / `break`.
        assert_eq!(
            classify_skip(SkipObservation {
                ok: true,
                in_progress: true,
                unmerged: false,
                replay_advanced: true,
            }),
            SkipProgress::Advanced
        );
    }

    #[test]
    fn finished_sequence_is_success() {
        assert_eq!(
            classify_skip(SkipObservation {
                ok: true,
                in_progress: false,
                unmerged: false,
                replay_advanced: true,
            }),
            SkipProgress::Finished
        );
    }

    #[test]
    fn same_step_after_a_non_zero_exit_is_a_failure() {
        assert_eq!(
            classify_skip(SkipObservation {
                ok: false,
                in_progress: true,
                unmerged: true,
                replay_advanced: false,
            }),
            SkipProgress::NoProgress
        );
    }

    #[test]
    fn moved_but_nothing_to_resolve_after_an_error_is_unclear() {
        assert_eq!(
            classify_skip(SkipObservation {
                ok: false,
                in_progress: true,
                unmerged: false,
                replay_advanced: true,
            }),
            SkipProgress::Unclear
        );
        assert_eq!(
            classify_skip(SkipObservation {
                ok: false,
                in_progress: false,
                unmerged: false,
                replay_advanced: true,
            }),
            SkipProgress::Unclear
        );
    }
}
