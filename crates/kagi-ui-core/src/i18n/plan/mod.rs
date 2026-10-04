//! Plan-text localization (ADR-0129 §3).
//!
//! `plan_note_text` / `plan_title_text` / `plan_recovery_text` are the ONLY
//! way the UI renders plan text. EN always delegates to the kagi-domain
//! `message_en()` renderer (no double-maintenance); JA strings live in the
//! per-category submodules (`plan/discard.rs`, …), one per producer
//! (ADR-0129 Phase 2/3 — every category is now fully typed).

pub mod branch;
pub mod checklist;
pub mod checkout;
pub mod cherry_revert;
pub mod cleanup;
pub mod clone;
pub mod commit;
pub mod common;
pub mod conflicts;
pub mod discard;
pub mod force_lease;
pub mod github;
pub mod history;
pub mod maintenance;
pub mod merge;
pub mod oplog_restore;
pub mod pull;
pub mod push;
pub mod rebase;
pub mod remote_branch;
pub mod reset;
pub mod ruleset;
pub mod snapshot;
pub mod stash;
pub mod switch;
pub mod sync;
pub mod tag;
pub mod worktree;

use kagi_domain::plan_note::{
    BranchTitle, CherryRevertTitle, CommitTitle, ConflictsTitle, GithubTitle, HistoryMoveDir,
    HistoryTitle, MaintenanceTitle, PlanNote, PlanRecovery, PlanTitle, PullTitle, PushTitle,
    RebaseTitle, RecoveryKind, StashTitle, SwitchTitle, TagTitle, WorktreeTitle,
};

use super::{lang, op::Op, Lang, Msg};
/// The AFTER chip is a state, not the backend's durable prediction sentence.
/// Match the typed operation, not English producer text (which the oplog keeps).
pub fn after_state_label(title: &PlanTitle) -> Option<&'static str> {
    match title {
        PlanTitle::Maintenance(title) => Some(maintenance::after_state_label(title)),
        PlanTitle::Stash(StashTitle::Drop { .. }) => Some(Msg::AfterStashDrop.t()),
        PlanTitle::Stash(StashTitle::Pop { .. }) => Some(Msg::AfterStashPop.t()),
        PlanTitle::History(HistoryTitle::UndoCommit { .. }) => Some(Msg::AfterHistoryUndo.t()),
        PlanTitle::History(HistoryTitle::Amend { .. }) => Some(Msg::AfterHistoryAmend.t()),
        PlanTitle::History(HistoryTitle::HistoryMove { .. }) => Some(Msg::AfterHistoryMove.t()),
        PlanTitle::Switch(SwitchTitle::SwitchToLatest { .. }) => Some(Msg::AfterSwitchLatest.t()),
        PlanTitle::Github(GithubTitle::ReviewPr { verdict, .. }) => Some(match verdict.as_str() {
            "approve" => Msg::AfterReviewApproved.t(),
            "comment" => Msg::AfterReviewCommented.t(),
            "request-changes" => Msg::AfterReviewChanges.t(),
            _ => return None,
        }),
        PlanTitle::Github(GithubTitle::MergePr { .. }) => Some(Msg::AfterPrMerged.t()),
        _ => None,
    }
}

/// Keep the complete backend prediction in the AX row and Copy all. Maintenance
/// already has a localized explanation because its producer text is English.
pub fn after_state_detail<'a>(title: &PlanTitle, predicted: &'a str) -> &'a str {
    match title {
        PlanTitle::Maintenance(title) => maintenance::after_state_detail(title),
        _ => predicted,
    }
}

/// Interpolate the JA catalog once; inserted branch names are never templates.
pub(super) fn advice_text(msg: Msg, args: &[&dyn std::fmt::Display]) -> String {
    use std::fmt::Write;

    let template = msg.t_for(Lang::Ja);
    let mut parts = template.split("{}");
    let mut out = String::with_capacity(template.len());
    out.push_str(parts.next().unwrap_or_default());
    for arg in args {
        let suffix = parts
            .next()
            .expect("advice argument requires a template slot");
        write!(out, "{arg}{suffix}").expect("standard advice arguments format into a String");
    }
    assert!(
        parts.next().is_none(),
        "advice template requires an argument"
    );
    out
}

/// Localized text for one plan note (blocker / warning).
pub fn plan_note_text(note: &PlanNote) -> String {
    match lang() {
        Lang::En => note.message_en(),
        Lang::Ja => note_ja_any(note),
    }
}

/// The JA half of [`plan_note_text`], callable without going through `lang()`.
///
/// A note can **carry** another note — a PR merge keeps the local branch for
/// the delete-branch family's own typed reason (#705) — so the Japanese
/// dispatch has to be reachable from inside a category renderer.
pub(crate) fn note_ja_any(note: &PlanNote) -> String {
    match note {
        PlanNote::Common(n) => common::note_ja(n),
        PlanNote::Discard(n) => discard::note_ja(n),
        PlanNote::Branch(n) => branch::note_ja(n),
        PlanNote::Stash(n) => stash::note_ja(n),
        PlanNote::History(n) => history::note_ja(n),
        PlanNote::Pull(n) => pull::note_ja(n),
        PlanNote::Push(n) => push::note_ja(n),
        PlanNote::Switch(n) => switch::note_ja(n),
        PlanNote::Checkout(n) => checkout::note_ja(n),
        PlanNote::Merge(n) => merge::note_ja(n),
        PlanNote::Worktree(n) => worktree::note_ja(n),
        PlanNote::CherryRevert(n) => cherry_revert::note_ja(n),
        PlanNote::Cleanup(n) => cleanup::note_ja(n),
        PlanNote::Conflicts(n) => conflicts::note_ja(n),
        PlanNote::Commit(n) => commit::note_ja(n),
        PlanNote::Checklist(n) => checklist::note_ja(n),
        PlanNote::Tag(n) => tag::note_ja(n),
        PlanNote::RemoteBranch(n) => remote_branch::note_ja(n),
        PlanNote::Reset(n) => reset::note_ja(n),
        PlanNote::ForceLease(n) => force_lease::note_ja(n),
        PlanNote::Github(n) => github::note_ja(n),
        PlanNote::Rebase(n) => rebase::note_ja(n),
        PlanNote::Snapshot(n) => snapshot::note_ja(n),
        PlanNote::Sync(n) => sync::note_ja(n),
        PlanNote::Maintenance(n) => maintenance::note_ja(n),
        PlanNote::Ruleset(n) => ruleset::note_ja(n),
        PlanNote::OplogRestore(n) => oplog_restore::note_ja(n),
        PlanNote::Clone(n) => clone::note_ja(n),
    }
}

/// Localized text for the plan title.
pub fn plan_title_text(title: &PlanTitle) -> String {
    match lang() {
        Lang::En => title.message_en(),
        Lang::Ja => match title {
            PlanTitle::Branch(t) => branch::title_ja(t),
            PlanTitle::Stash(t) => stash::title_ja(t),
            PlanTitle::History(t) => history::title_ja(t),
            PlanTitle::Pull(t) => pull::title_ja(t),
            PlanTitle::Push(t) => push::title_ja(t),
            PlanTitle::Switch(t) => switch::title_ja(t),
            PlanTitle::Checkout(t) => checkout::title_ja(t),
            PlanTitle::Merge(t) => merge::title_ja(t),
            PlanTitle::Worktree(t) => worktree::title_ja(t),
            PlanTitle::CherryRevert(t) => cherry_revert::title_ja(t),
            PlanTitle::Cleanup(t) => cleanup::title_ja(t),
            PlanTitle::Conflicts(t) => conflicts::title_ja(t),
            PlanTitle::Commit(t) => commit::title_ja(t),
            PlanTitle::Tag(t) => tag::title_ja(t),
            PlanTitle::RemoteBranch(t) => remote_branch::title_ja(t),
            PlanTitle::Reset(t) => reset::title_ja(t),
            PlanTitle::ForceLease(t) => force_lease::title_ja(t),
            PlanTitle::Github(t) => github::title_ja(t),
            PlanTitle::Rebase(t) => rebase::title_ja(t),
            PlanTitle::Snapshot(t) => snapshot::title_ja(t),
            PlanTitle::Sync(t) => sync::title_ja(t),
            PlanTitle::Maintenance(t) => maintenance::title_ja(t),
            PlanTitle::OplogRestore(t) => oplog_restore::title_ja(t),
            PlanTitle::Clone(t) => clone::title_ja(t),
            PlanTitle::Discard { .. } => discard::title_ja(title),
        },
    }
}

/// Short visible heading and up to two typed target chips. The full localized
/// plan title remains the dialog's accessible name and the first Copy all line.
pub fn plan_heading_text(
    title: &PlanTitle,
) -> (&'static str, [Option<std::borrow::Cow<'_, str>>; 2]) {
    use PlanTitle::*;
    let (op, first, second): (
        Op,
        Option<std::borrow::Cow<'_, str>>,
        Option<std::borrow::Cow<'_, str>>,
    ) = match title {
        Branch(BranchTitle::CreateBranch { name, at, .. }) => (
            Op::CreateBranch,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            Some(std::borrow::Cow::Borrowed(at.as_str())),
        ),
        Branch(BranchTitle::RenameBranch { old, new }) => (
            Op::Rename,
            Some(std::borrow::Cow::Borrowed(old.as_str())),
            Some(std::borrow::Cow::Borrowed(new.as_str())),
        ),
        Branch(BranchTitle::DeleteBranch { name, tip }) => (
            Op::Delete,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            tip.as_deref().map(std::borrow::Cow::Borrowed),
        ),
        Stash(StashTitle::Push { next_count }) => (
            Op::StashPush,
            Some(std::borrow::Cow::Owned(
                Msg::PlanHeadingStashes
                    .t()
                    .replace("{}", &next_count.to_string()),
            )),
            None,
        ),
        Stash(StashTitle::Apply { index }) => (
            Op::StashApply,
            Some(std::borrow::Cow::Owned(format!("stash@{{{index}}}"))),
            None,
        ),
        Stash(StashTitle::Pop { index }) => (
            Op::Pop,
            Some(std::borrow::Cow::Owned(format!("stash@{{{index}}}"))),
            None,
        ),
        Stash(StashTitle::Drop { index }) => (
            Op::Drop,
            Some(std::borrow::Cow::Owned(format!("stash@{{{index}}}"))),
            None,
        ),
        Stash(StashTitle::DropRemote { label }) => (
            Op::Drop,
            Some(std::borrow::Cow::Borrowed(label.as_str())),
            None,
        ),
        History(HistoryTitle::UndoCommit { sha, .. }) => (
            Op::Undo,
            Some(std::borrow::Cow::Borrowed(sha.as_str())),
            None,
        ),
        History(HistoryTitle::Amend { sha, .. }) => (
            Op::Amend,
            Some(std::borrow::Cow::Borrowed(sha.as_str())),
            None,
        ),
        History(HistoryTitle::HistoryMove {
            label, branch, to, ..
        }) => (
            if *label == HistoryMoveDir::Undo {
                Op::Undo
            } else {
                Op::Redo
            },
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(to.as_str())),
        ),
        Pull(PullTitle::PullRemote { branch, behind, .. }) => (
            Op::Pull,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Owned(
                Msg::PlanHeadingBehind
                    .t()
                    .replace("{}", &behind.to_string()),
            )),
        ),
        Pull(
            PullTitle::Pull { branch, remote, .. } | PullTitle::PullBranchFf { branch, remote, .. },
        ) => (
            Op::Pull,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        Push(PushTitle::Push {
            branch,
            set_upstream: true,
            ..
        }) => (
            Op::Push,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(Msg::PlanHeadingSetUpstream.t())),
        ),
        Push(
            PushTitle::Push { branch, remote, .. } | PushTitle::PushBranch { branch, remote, .. },
        ) => (
            Op::Push,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        Push(PushTitle::PushBlocked) => (Op::Push, None, None),
        Push(PushTitle::SetUpstream { branch, upstream }) => (
            Op::SetUpstream,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(upstream.as_str())),
        ),
        Switch(SwitchTitle::CheckoutTracking { remote, local }) => (
            Op::CheckoutTracking,
            Some(std::borrow::Cow::Borrowed(local.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        Switch(SwitchTitle::SwitchToLatest { branch, remote }) => (
            Op::SwitchToLatest,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        Checkout(kagi_domain::plan_note::CheckoutTitle::Checkout { branch }) => (
            Op::Checkout,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            None,
        ),
        Checkout(kagi_domain::plan_note::CheckoutTitle::CheckoutCommit { sha, .. }) => (
            Op::Checkout,
            Some(std::borrow::Cow::Borrowed(sha.as_str())),
            None,
        ),
        Merge(kagi_domain::plan_note::MergeTitle::Into { target, current }) => (
            Op::Merge,
            Some(std::borrow::Cow::Borrowed(target.as_str())),
            current.as_deref().map(std::borrow::Cow::Borrowed),
        ),
        Worktree(WorktreeTitle::CreateBranchCheckout { name, at }) => (
            Op::CreateBranch,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            Some(std::borrow::Cow::Borrowed(at.as_str())),
        ),
        Worktree(WorktreeTitle::CreateWorktree { branch, start }) => (
            Op::CreateWorktree,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(start.as_str())),
        ),
        Worktree(WorktreeTitle::UnlockWorktree { name }) => (
            Op::UnlockWorktree,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            None,
        ),
        Worktree(WorktreeTitle::RemoveWorktree { name }) => (
            Op::RemoveWorktree,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            None,
        ),
        Worktree(WorktreeTitle::LockWorktree { name }) => (
            Op::LockWorktree,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            None,
        ),
        Worktree(WorktreeTitle::AutoUnlockWorktree { name }) => (
            Op::UnlockWorktree,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            None,
        ),
        Worktree(WorktreeTitle::PruneWorktrees) => (Op::PruneWorktrees, None, None),
        Worktree(WorktreeTitle::RepairWorktrees) => (Op::RepairWorktrees, None, None),
        CherryRevert(CherryRevertTitle::CherryPick { sha, branch, .. }) => (
            Op::CherryPick,
            Some(std::borrow::Cow::Borrowed(sha.as_str())),
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
        ),
        CherryRevert(CherryRevertTitle::Revert { sha, branch, .. }) => (
            Op::Revert,
            Some(std::borrow::Cow::Borrowed(sha.as_str())),
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
        ),
        Cleanup(kagi_domain::plan_note::CleanupTitle::CleanupDelete { count }) => (
            Op::Cleanup,
            Some(std::borrow::Cow::Owned(
                Msg::PlanHeadingBranches
                    .t()
                    .replace("{}", &count.to_string()),
            )),
            None,
        ),
        Conflicts(ConflictsTitle::Continue { op }) => (
            Op::Continue,
            Some(std::borrow::Cow::Borrowed(op.as_str())),
            None,
        ),
        Conflicts(ConflictsTitle::Abort { op }) => (
            Op::Abort,
            Some(std::borrow::Cow::Borrowed(op.as_str())),
            None,
        ),
        Conflicts(ConflictsTitle::Skip { op }) => (
            Op::Skip,
            Some(std::borrow::Cow::Borrowed(op.as_str())),
            None,
        ),
        Commit(CommitTitle::Commit { summary }) => (
            Op::Commit,
            Some(std::borrow::Cow::Borrowed(summary.as_str())),
            None,
        ),
        Commit(CommitTitle::FinalizeMergeCommit) => (Op::MergeCommit, None, None),
        Tag(TagTitle::CreateTag { name, at }) => (
            Op::CreateTag,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            Some(std::borrow::Cow::Borrowed(at.as_str())),
        ),
        Tag(TagTitle::PushTag { name, remote }) => (
            Op::PushTag,
            Some(std::borrow::Cow::Borrowed(name.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        RemoteBranch(kagi_domain::plan_note::RemoteBranchTitle::DeleteRemoteBranch {
            remote,
            branch,
        }) => (
            Op::Delete,
            Some(std::borrow::Cow::Owned(format!("{remote}/{branch}"))),
            None,
        ),
        Reset(kagi_domain::plan_note::ResetTitle::ResetCurrentToHead { branch, to }) => (
            Op::Reset,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(to.as_str())),
        ),
        ForceLease(kagi_domain::plan_note::ForceLeaseTitle::ForceLeasePush { branch, remote }) => (
            Op::Push,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(remote.as_str())),
        ),
        Github(GithubTitle::MergePr { number, method }) => (
            Op::Merge,
            Some(std::borrow::Cow::Owned(format!("#{number}"))),
            Some(std::borrow::Cow::Borrowed(github::merge_method_label(
                method,
            ))),
        ),
        Github(GithubTitle::ApplySuggestion { path }) => (
            Op::ApplySuggestion,
            Some(std::borrow::Cow::Borrowed(path.as_str())),
            None,
        ),
        Github(GithubTitle::CreateIssue) => (Op::IssueCreate, None, None),
        Github(GithubTitle::CommentIssue { number }) => (
            Op::IssueComment,
            Some(std::borrow::Cow::Owned(format!("#{number}"))),
            None,
        ),
        Github(GithubTitle::CommentPr { number }) => (
            Op::PrComment,
            Some(std::borrow::Cow::Owned(format!("#{number}"))),
            None,
        ),
        Github(GithubTitle::ReviewPr { number, verdict }) => (
            Op::PrReview,
            Some(std::borrow::Cow::Owned(format!("#{number}"))),
            Some(std::borrow::Cow::Borrowed(match verdict.as_str() {
                "approve" => Msg::PlanHeadingApprove.t(),
                "request-changes" => Msg::PlanHeadingRequestChanges.t(),
                "comment" => Msg::PlanHeadingComment.t(),
                _ => verdict.as_str(),
            })),
        ),
        Github(GithubTitle::EditPr { number }) => (
            Op::PrEdit,
            Some(std::borrow::Cow::Owned(format!("#{number}"))),
            None,
        ),
        Rebase(RebaseTitle::RebaseCurrentOnto { branch, onto }) => (
            Op::Rebase,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(onto.as_str())),
        ),
        Rebase(RebaseTitle::ReplayOnto { branch, onto }) => (
            Op::Replay,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(onto.as_str())),
        ),
        Snapshot(kagi_domain::plan_note::SnapshotTitle::Restore { id }) => (
            Op::RestoreToPoint,
            Some(std::borrow::Cow::Borrowed(id.as_str())),
            None,
        ),
        Sync(kagi_domain::plan_note::SyncTitle::SyncToRemote { branch, to, .. }) => (
            Op::SyncToRemote,
            Some(std::borrow::Cow::Borrowed(branch.as_str())),
            Some(std::borrow::Cow::Borrowed(to.as_str())),
        ),
        Maintenance(MaintenanceTitle::WriteCommitGraph) => (Op::WriteCommitGraph, None, None),
        Maintenance(MaintenanceTitle::EnableFsmonitor) => (Op::EnableFsmonitor, None, None),
        OplogRestore(kagi_domain::plan_note::OplogRestoreTitle::Revert { id, op }) => (
            Op::OpRevert,
            Some(std::borrow::Cow::Owned(format!("#{id}"))),
            Some(std::borrow::Cow::Borrowed(op.as_str())),
        ),
        OplogRestore(kagi_domain::plan_note::OplogRestoreTitle::RestoreTo { id, op }) => (
            Op::RestoreToPoint,
            Some(std::borrow::Cow::Owned(format!("#{id}"))),
            Some(std::borrow::Cow::Borrowed(op.as_str())),
        ),
        Clone(kagi_domain::plan_note::CloneTitle::Clone { source }) => (
            Op::Clone,
            Some(std::borrow::Cow::Borrowed(source.as_str())),
            None,
        ),
        Discard { single, count } => (
            Op::Discard,
            Some(
                single
                    .as_deref()
                    .map(std::borrow::Cow::Borrowed)
                    .unwrap_or_else(|| {
                        std::borrow::Cow::Owned(
                            Msg::PlanHeadingFiles.t().replace("{}", &count.to_string()),
                        )
                    }),
            ),
            None,
        ),
    };
    // Detached HEAD has no branch name. Never draw a vacant target chip (also
    // applies to optional target strings supplied by other plan producers).
    (
        op.t(),
        [
            first.filter(|chip| !chip.is_empty()),
            second.filter(|chip| !chip.is_empty()),
        ],
    )
}

#[cfg(test)]
#[test]
fn detached_reset_heading_omits_empty_branch_chip() {
    let title = PlanTitle::Reset(kagi_domain::plan_note::ResetTitle::ResetCurrentToHead {
        branch: String::new(),
        to: "41ffc069".into(),
    });
    let (_, chips) = plan_heading_text(&title);
    assert_eq!(chips[0], None);
    assert_eq!(chips[1].as_deref(), Some("41ffc069"));
}

/// Localized text for the recovery block. `None` renders empty (legacy plans
/// always carry `Some`; the Option exists for future no-recovery plans).
pub fn plan_recovery_text(recovery: Option<&PlanRecovery>) -> String {
    let Some(r) = recovery else {
        return String::new();
    };
    match lang() {
        Lang::En => r.message_en(),
        Lang::Ja => match &r.kind {
            RecoveryKind::Branch(r) => branch::recovery_ja(r),
            RecoveryKind::Stash(r) => stash::recovery_ja(r),
            RecoveryKind::History(r) => history::recovery_ja(r),
            RecoveryKind::Pull(r) => pull::recovery_ja(r),
            RecoveryKind::Push(r) => push::recovery_ja(r),
            RecoveryKind::Switch(r) => switch::recovery_ja(r),
            RecoveryKind::Checkout(r) => checkout::recovery_ja(r),
            RecoveryKind::Merge(r) => merge::recovery_ja(r),
            RecoveryKind::Worktree(r) => worktree::recovery_ja(r),
            RecoveryKind::CherryRevert(r) => cherry_revert::recovery_ja(r),
            RecoveryKind::Cleanup(r) => cleanup::recovery_ja(r),
            RecoveryKind::Conflicts(r) => conflicts::recovery_ja(r),
            RecoveryKind::Commit(r) => commit::recovery_ja(r),
            RecoveryKind::Tag(r) => tag::recovery_ja(r),
            RecoveryKind::RemoteBranch(r) => remote_branch::recovery_ja(r),
            RecoveryKind::Reset(r) => reset::recovery_ja(r),
            RecoveryKind::ForceLease(r) => force_lease::recovery_ja(r),
            RecoveryKind::Github(r) => github::recovery_ja(r),
            RecoveryKind::Rebase(r) => rebase::recovery_ja(r),
            RecoveryKind::Snapshot(r) => snapshot::recovery_ja(r),
            RecoveryKind::Sync(r) => sync::recovery_ja(r),
            RecoveryKind::Maintenance(r) => maintenance::recovery_ja(r),
            RecoveryKind::OplogRestore(r) => oplog_restore::recovery_ja(r),
            RecoveryKind::Discard => discard::recovery_ja(),
        },
    }
}

/// Sentences of the already-localized recovery explanation, for surfaces
/// that offer the plan's structured commands separately. This preserves the
/// former plan-card grouping rule (#994): after trimming, a `git `-prefixed
/// display line is a command line, not prose. It never decides what to run.
pub fn plan_recovery_sentences(recovery: Option<&PlanRecovery>) -> Vec<String> {
    recovery_sentences_from_text(&plan_recovery_text(recovery))
}

fn recovery_sentences_from_text(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("git "))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
#[test]
fn recovery_sentences_exclude_display_commands_in_both_languages() {
    use kagi_domain::plan_note::BranchRecovery;

    let branch = BranchRecovery::CreateBranch {
        name: "demo".into(),
    };
    let en = branch.message_en();
    let ja = branch::recovery_ja(&branch);
    assert_eq!(
        recovery_sentences_from_text(&en),
        ["The new branch 'demo' can be removed without side effects:"]
    );
    assert_eq!(
        recovery_sentences_from_text(&ja),
        ["副作用なく削除できます:"]
    );

    let deleted = BranchRecovery::DeleteBranch {
        name: "demo".into(),
        tip: Some("abc123".into()),
    };
    assert_eq!(
        recovery_sentences_from_text(&deleted.message_en()),
        [
            "To restore the deleted branch:",
            "The tip 'abc123' will be retained by a backup ref; use the receipt's backup ref to restore after GC.",
        ]
    );
}
