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

/// Where the sequencer stands, as read off the repository.
///
/// Every field is `None` when the value could not be observed — absent file,
/// I/O error, or unparsable content are indistinguishable at this layer and
/// must all be treated as *missing*, never as a changed value (#567 P1). That
/// is the whole reason this is a struct of `Option`s and not a comparison the
/// caller has already collapsed to a `bool`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplayPosition {
    /// The commit being replayed: `REBASE_HEAD` / `CHERRY_PICK_HEAD` /
    /// `REVERT_HEAD`.
    pub marker: Option<String>,
    /// The rebase step counter (`rebase-merge/msgnum`). Absent for the apply
    /// backend, which is *no evidence* rather than evidence of no movement.
    pub step: Option<usize>,
}

/// What one position signal says about movement across the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    /// Observed on both sides and it moved forward.
    Advanced,
    /// Observed on both sides and it is identical.
    Unchanged,
    /// Observed on neither side: this signal has nothing to say.
    Absent,
    /// Observed on exactly one side, or it moved backwards. Something is wrong
    /// with the observation, not with the sequencer.
    Broken,
}

/// What the repository looked like after `git <op> --skip` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipObservation {
    /// The command exited zero.
    pub ok: bool,
    /// The sequencer is still in progress (`RepositoryState` is not `Clean`).
    pub in_progress: bool,
    /// A freshly-read index still has unmerged entries.
    pub unmerged: bool,
    /// The replay position before the command ran.
    pub before: ReplayPosition,
    /// The replay position after it returned.
    pub after: ReplayPosition,
}

fn step_signal(before: Option<usize>, after: Option<usize>) -> Signal {
    match (before, after) {
        (Some(b), Some(a)) if a > b => Signal::Advanced,
        (Some(b), Some(a)) if a == b => Signal::Unchanged,
        // A counter that went backwards is not a sequencer that rewound: it is
        // a reading we cannot trust.
        (Some(_), Some(_)) => Signal::Broken,
        (None, None) => Signal::Absent,
        _ => Signal::Broken,
    }
}

fn marker_signal(before: &Option<String>, after: &Option<String>) -> Signal {
    match (before, after) {
        (Some(b), Some(a)) if a != b => Signal::Advanced,
        (Some(_), Some(_)) => Signal::Unchanged,
        (None, None) => Signal::Absent,
        // The marker appeared or vanished: unreadable, or a different sequence.
        _ => Signal::Broken,
    }
}

/// Did the sequencer leave the step we asked it to drop?
///
/// `Some(true)` only when a signal was observed on **both** sides and moved
/// forward within the same sequence; `Some(false)` only when a signal was
/// observed on both sides and is identical; `None` when the observation itself
/// is missing or inconsistent — the caller must not read that as progress.
fn replay_advanced(o: &SkipObservation) -> Option<bool> {
    let signals = [
        step_signal(o.before.step, o.after.step),
        marker_signal(&o.before.marker, &o.after.marker),
    ];
    if signals.contains(&Signal::Broken) {
        return None;
    }
    if signals.contains(&Signal::Advanced) {
        return Some(true);
    }
    if signals.contains(&Signal::Unchanged) {
        return Some(false);
    }
    // Nothing observed at all.
    None
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
    if o.ok {
        return SkipProgress::Advanced;
    }
    match replay_advanced(&o) {
        // Moved on and left a conflict behind: the #540 case.
        Some(true) if o.unmerged => SkipProgress::Advanced,
        // Moved on but nothing to resolve — not the shape a skip should leave.
        Some(true) => SkipProgress::Unclear,
        Some(false) => SkipProgress::NoProgress,
        // The observation failed. An unread marker is not progress.
        None => SkipProgress::Unclear,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully observed position.
    fn at(step: usize, marker: &str) -> ReplayPosition {
        ReplayPosition {
            marker: Some(marker.to_string()),
            step: Some(step),
        }
    }

    /// A conflicted, still-in-progress sequencer that exited non-zero — the
    /// shape every #540 judgement call has.
    fn stopped(before: ReplayPosition, after: ReplayPosition) -> SkipObservation {
        SkipObservation {
            ok: false,
            in_progress: true,
            unmerged: true,
            before,
            after,
        }
    }

    /// #540: `rebase --skip` stops at the next conflicting commit with exit 1.
    #[test]
    fn advanced_to_next_conflict_is_not_a_failure() {
        assert_eq!(
            classify_skip(stopped(at(1, "aaa"), at(2, "bbb"))),
            SkipProgress::Advanced
        );
    }

    /// The apply backend writes no `msgnum`: absent on both sides is *no
    /// evidence*, so the marker alone still proves the move.
    #[test]
    fn marker_alone_can_prove_the_move() {
        assert_eq!(
            classify_skip(stopped(
                ReplayPosition {
                    marker: Some("aaa".into()),
                    step: None
                },
                ReplayPosition {
                    marker: Some("bbb".into()),
                    step: None
                },
            )),
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
                before: at(1, "aaa"),
                after: at(2, "bbb"),
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
                before: at(1, "aaa"),
                after: ReplayPosition::default(),
            }),
            SkipProgress::Finished
        );
    }

    #[test]
    fn same_step_after_a_non_zero_exit_is_a_failure() {
        assert_eq!(
            classify_skip(stopped(at(1, "aaa"), at(1, "aaa"))),
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
                before: at(1, "aaa"),
                after: at(2, "bbb"),
            }),
            SkipProgress::Unclear
        );
        assert_eq!(
            classify_skip(SkipObservation {
                ok: false,
                in_progress: false,
                unmerged: false,
                before: at(1, "aaa"),
                after: at(2, "bbb"),
            }),
            SkipProgress::Unclear
        );
    }

    // ── #567 P1: an observation that failed is never progress ──────────────

    /// The marker could not be read afterwards (I/O error, or it vanished).
    /// A plain `!=` on the raw values would have called this Advanced.
    #[test]
    fn marker_disappearing_is_unclear_not_advanced() {
        assert_eq!(
            classify_skip(stopped(
                at(1, "aaa"),
                ReplayPosition {
                    marker: None,
                    step: Some(1)
                },
            )),
            SkipProgress::Unclear
        );
    }

    /// `msgnum` unreadable or unparsable afterwards. The old code folded that
    /// to `0`, which compares unequal to `1` and read as progress.
    #[test]
    fn step_becoming_unreadable_is_unclear_not_advanced() {
        assert_eq!(
            classify_skip(stopped(
                at(1, "aaa"),
                ReplayPosition {
                    marker: Some("aaa".into()),
                    step: None
                },
            )),
            SkipProgress::Unclear
        );
    }

    /// The marker was unreadable *before* the command, so there is nothing to
    /// compare the after-value against.
    #[test]
    fn missing_before_observation_is_unclear() {
        assert_eq!(
            classify_skip(stopped(
                ReplayPosition {
                    marker: None,
                    step: None
                },
                at(2, "bbb"),
            )),
            SkipProgress::Unclear
        );
    }

    /// A counter that went backwards is a bad reading, not a rewound sequencer.
    #[test]
    fn step_regression_is_unclear() {
        assert_eq!(
            classify_skip(stopped(at(2, "aaa"), at(1, "bbb"))),
            SkipProgress::Unclear
        );
    }

    /// Nothing observed on either side: no evidence is not evidence of
    /// movement.
    #[test]
    fn no_observation_at_all_is_unclear() {
        assert_eq!(
            classify_skip(stopped(
                ReplayPosition::default(),
                ReplayPosition::default()
            )),
            SkipProgress::Unclear
        );
    }
}
