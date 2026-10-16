//! Branch command availability and concise contextual labels.
use super::{BranchConflictMode, BranchKind, BranchMenuContext, ItemState, Msg, SharedString};

pub(super) fn disabled(reason: impl Into<SharedString>) -> ItemState {
    ItemState::Disabled(reason.into())
}

pub(super) fn force_lease_push_state(ctx: &BranchMenuContext) -> ItemState {
    // force_with_lease_push always acts on the currently checked-out branch
    // (mirrors ResetCurrentToHead) — only offer it from that row's menu.
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if !ctx.is_current {
        disabled(Msg::BcmOnlyFromCurrentBranch.t())
    } else if !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn reset_current_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn create_pr_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn fetch_remote_branch_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Local) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn checkout_state(ctx: &BranchMenuContext) -> ItemState {
    // A local checkout behind a running operation is queued (#355 stage 3a);
    // the remote one creates a tracking branch, which the queue does not take.
    if ctx.busy && !matches!(ctx.kind, BranchKind::Local) {
        disabled(Msg::BcmBusy.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else if ctx.checked_out_in_other_worktree {
        let path = ctx
            .checked_out_worktree_path
            .as_deref()
            .unwrap_or("another worktree");
        disabled(format!("{}: {}", Msg::BcmCheckedOutElsewhere.t(), path))
    } else {
        ItemState::Enabled
    }
}

pub(super) fn switch_to_latest_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if matches!(ctx.kind, BranchKind::Local) && !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn switch_to_latest_label(ctx: &BranchMenuContext) -> String {
    let local_name = match ctx.kind {
        BranchKind::Local => ctx.name.clone(),
        BranchKind::Remote => ctx
            .name
            .split_once('/')
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| ctx.name.clone()),
    };
    let behind = ctx.behind.filter(|&n| n > 0);
    if let (BranchKind::Local, true, Some(n)) = (&ctx.kind, ctx.has_upstream, behind) {
        format!("Switch to latest {} ↓{}", local_name, n)
    } else {
        format!("Switch to latest {}", local_name)
    }
}

pub(super) fn open_worktree_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) {
        disabled(Msg::BcmNotImplementedYet.t())
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn create_branch_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn create_worktree_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn create_tag_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn merge_state(ctx: &BranchMenuContext) -> ItemState {
    // A branch-backed merge can freeze its destination while a write runs.
    // The enqueue path rechecks the live HEAD and refuses if it detached.
    if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn rebase_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else {
        ItemState::Enabled
    }
}

/// Replay rewrites the clicked branch (not HEAD) without a checkout, so the
/// current worktree's state is irrelevant except for busy/conflict latches;
/// the plan itself reports a dirty other-worktree, merges and conflicts as
/// blockers (`plan_replay_onto`).
pub(super) fn replay_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else if !matches!(ctx.kind, BranchKind::Local) {
        disabled(Msg::BcmLocalOnly.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn rename_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn delete_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.is_current {
        disabled(Msg::BcmCurrentBranch.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn delete_remote_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Local) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn copy_upstream_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.has_upstream {
        ItemState::Enabled
    } else {
        disabled(Msg::BcmNoUpstream.t())
    }
}

/// #536: only the current branch (its index and working tree are what gets
/// replaced), and only with an upstream to sync to; the plan reports
/// not-fetched / conflicts / already-in-sync as blockers.
pub(super) fn sync_to_remote_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else if !ctx.is_current {
        disabled(Msg::BcmCurrentBranchOnly.t())
    } else if !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn pull_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.kind, BranchKind::Remote) {
        disabled(Msg::BcmNotImplementedYet.t())
    } else if !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn push_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.kind, BranchKind::Remote) || !ctx.has_upstream {
        ItemState::Hidden
    } else if ctx.ahead == Some(0) {
        disabled(Msg::BcmNothingToPush.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn push_create_upstream_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) || ctx.has_upstream {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn set_upstream_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else {
        ItemState::Enabled
    }
}

pub(super) fn no_upstream_info_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Local) && !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Hidden
    }
}

pub(super) fn pull_label(ctx: &BranchMenuContext) -> String {
    match (ctx.has_upstream, ctx.behind) {
        (true, Some(n)) if n > 0 => format!("Pull ↓{}", n),
        (true, _) => "Pull".to_string(),
        (false, _) => "Pull".to_string(),
    }
}

pub(super) fn push_label(ctx: &BranchMenuContext) -> String {
    match (ctx.has_upstream, ctx.ahead) {
        (true, Some(0)) => "Push".to_string(),
        (true, Some(n)) => format!("Push ↑{}", n),
        (true, None) => "Push".to_string(),
        (false, _) if !ctx.is_pushed => "Push and create upstream".to_string(),
        (false, _) => "Push".to_string(),
    }
}

pub(super) fn delete_label(ctx: &BranchMenuContext) -> &'static str {
    if ctx.merged_into_current {
        "Delete branch..."
    } else {
        "Delete branch... (unmerged)"
    }
}

pub(super) fn copy_head_sha_label(ctx: &BranchMenuContext) -> String {
    let short: String = ctx.head_sha.chars().take(8).collect();
    if short.is_empty() {
        "Copy branch HEAD SHA".to_string()
    } else {
        format!("Copy branch HEAD SHA ({})", short)
    }
}

pub(super) fn set_upstream_label(ctx: &BranchMenuContext) -> String {
    match ctx.upstream_name.as_ref() {
        Some(upstream) => format!("Set upstream ({})", upstream),
        None => "Set upstream".to_string(),
    }
}
