//! Localize diagnostic payloads before the typed note renderer inserts them.
use kagi_domain::plan_note::{
    CloneNote, CommonNote, PlanNote, PullNote, PushNote, StashNote, WorktreeNote,
};
use std::borrow::Cow;

macro_rules! payload {
    ($note:expr) => {
        match $note {
            PlanNote::Common(CommonNote::GitErrorPassthrough { message }) => Some(message),
            PlanNote::Clone(CloneNote::DestinationUnreadable { error, .. }) => Some(error),
            PlanNote::Pull(
                PullNote::NoUpstreamWithHint { err, .. } | PullNote::NoUpstream { err, .. },
            )
            | PlanNote::Push(PushNote::NoUpstreamWithErr { err, .. })
            | PlanNote::Worktree(WorktreeNote::LockStateUnreadable { err, .. }) => Some(err),
            PlanNote::Stash(
                StashNote::ManifestUnavailable { reason }
                | StashNote::ApplyPredictionUnavailable { reason }
                | StashNote::PopPredictionUnavailable { reason },
            ) => Some(reason),
            PlanNote::Worktree(
                WorktreeNote::LockedWithReason {
                    reason: Some(reason),
                }
                | WorktreeNote::RemoveLocked {
                    reason: Some(reason),
                    ..
                }
                | WorktreeNote::LocksWorktree {
                    reason: Some(reason),
                    ..
                }
                | WorktreeNote::AlreadyLocked {
                    reason: Some(reason),
                    ..
                },
            ) => Some(reason),
            _ => None,
        }
    };
}

pub(super) fn note_payload(note: &PlanNote) -> Cow<'_, PlanNote> {
    if let Some(value) = payload!(note) {
        if let Cow::Owned(display) = super::diagnostic(value) {
            let mut localized = note.clone();
            if let Some(value) = payload!(&mut localized) {
                *value = display;
            }
            return Cow::Owned(localized);
        }
    }
    Cow::Borrowed(note)
}
