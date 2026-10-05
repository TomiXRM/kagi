use super::fixtures::FixtureKind::*;
use super::{modals, popups};
use super::{Action, Entry};

macro_rules! capture {
    ($name:literal, $description:literal, $fixture:ident, $open:path) => {
        Entry {
            name: concat!("inventory:", $name),
            description: $description,
            fixture: $fixture,
            action: Action::Capture($open),
        }
    };
}
macro_rules! skip {
    ($name:literal, $description:literal, $reason:literal) => {
        Entry {
            name: concat!("inventory:", $name),
            description: $description,
            fixture: Basic,
            action: Action::Skip($reason),
        }
    };
}

/// #1013's numbered index, without duplicate attempts at Update/ForceLeasePush.
/// Names stay stable because the CLI substring filter and comparison PNGs use them.
pub(super) const INVENTORY: &[Entry] = &[
    capture!("01-RemoteBrowse", "Remote connect command → open_remote_browse", Basic, modals::remote_browse),
    capture!("02-Clone", "Home Repositories row → open_clone_card", Basic, modals::clone_card),
    capture!("03-Update", "Available release → open_update_modal", Basic, modals::update),
    capture!("04-SmartCommit", "Commit panel Generate → consent", Basic, modals::smart_commit),
    capture!("05-AppNotice", "Operation notice → present_app_notice", Basic, modals::app_notice),
    capture!("06-Checkout", "Branch checkout → open_plan_modal", Branches, modals::checkout),
    capture!("07-Pull", "Toolbar Pull → open_pull_modal", Basic, modals::pull),
    capture!("08-Amend", "Commit panel Amend → open_amend_modal", Basic, modals::amend),
    capture!("09-Pop", "Stash menu Pop → open_pop_modal", Stash, modals::pop),
    capture!("10-StashDrop", "Stash menu Drop → open_stash_drop_modal", Stash, modals::stash_drop),
    capture!("11-PushTag", "Tag menu Push → open_push_tag_modal", TagRemote, modals::push_tag),
    capture!("12-PrMerge", "PR page Merge → open_pr_merge_modal", Branches, modals::pr_merge),
    capture!("13-PrFields", "PR Labels → field picker", Basic, modals::pr_fields),
    capture!("14-Push", "Toolbar Push → open_push_modal", Basic, modals::push),
    capture!("15-BranchPlan", "Branch menu Push → open_branch_plan_modal", Basic, modals::branch_plan),
    capture!("16-SetUpstream", "Branch menu Set upstream", Basic, modals::set_upstream),
    capture!("17-RenameBranch", "Branch menu Rename", Basic, modals::rename_branch),
    capture!("18-Merge", "Branch menu Merge → open_merge_modal", Branches, modals::merge),
    capture!("19-TrackingCheckout", "Remote branch Checkout → tracking plan", RemoteBranch, modals::tracking_checkout),
    capture!("20-SwitchToLatest", "Remote branch Switch to latest", RemoteBranch, modals::switch_to_latest),
    capture!("21-CreateBranch", "Commit menu Create branch", Basic, modals::create_branch),
    capture!("22-CreateTag", "Commit menu Create tag", Basic, modals::create_tag),
    capture!("23-CreateWorktree", "Commit menu Create worktree", Basic, modals::create_worktree),
    capture!("24-UnlockWorktree", "Worktree menu Unlock", LockedWorktree, modals::unlock_worktree),
    capture!("25-RemoveWorktree", "Worktree menu Remove", LinkedWorktree, modals::remove_worktree),
    capture!("26-WorktreeLockReason", "Worktree menu Lock → reason", LinkedWorktree, modals::worktree_lock_reason),
    capture!("27-LockWorktree", "Worktree Lock reason → Review", LinkedWorktree, modals::lock_worktree),
    capture!("28-PruneWorktrees", "Worktree menu Prune", Basic, modals::prune_worktrees),
    capture!("29-RepairWorktrees", "Worktree menu Repair", Basic, modals::repair_worktrees),
    capture!("30-RepoHealth", "Health fix → commit graph plan", Basic, modals::repo_health),
    capture!("31-ApplySuggestion", "PR thread Apply suggestion", Basic, modals::apply_suggestion),
    capture!("32-OplogRestore", "Operation Log row → Restore", Oplog, modals::oplog_restore),
    capture!("33-StashPush", "Toolbar Stash → stash modal", Dirty, modals::stash_push),
    capture!("34-StashApply", "Stash menu Apply", Stash, modals::stash_apply),
    capture!("35-CherryPick", "Commit menu Cherry-pick", CherryPick, modals::cherry_pick),
    capture!("36-Revert", "Commit menu Revert", Basic, modals::revert),
    capture!("37-History", "Toolbar Undo → history plan", Basic, modals::history),
    capture!("38-DeleteBranch", "Branch menu Delete", Branches, modals::delete_branch),
    capture!("39-DeleteRemoteBranch", "Remote branch menu Advanced Delete", RemoteBranch, modals::delete_remote_branch),
    capture!("40-ResetCurrent", "Commit menu Reset current", Basic, modals::reset_current),
    capture!("41-ForceLeasePush", "Branch menu Force-with-lease Push", ForceLease, modals::force_lease_push),
    capture!("42-RebaseCurrentOnto", "Branch menu Rebase onto", Branches, modals::rebase_current_onto),
    capture!("43-BranchCleanup", "Cleanup view bulk Delete", Branches, modals::branch_cleanup),
    capture!("44-Discard", "File menu Discard", Dirty, modals::discard),
    capture!("45-ConflictContinue", "Resolved cherry-pick conflict → Continue", ConflictSequencer, modals::conflict_continue),
    capture!("46-ConflictAbort", "Conflict mode Abort", Conflict, modals::conflict_abort),
    capture!("47-EditorDirtyGuard", "Editor with unsaved changes → Close", Basic, modals::editor_dirty_guard),
    capture!("48-EditorFsPrompt", "Editor tree menu Rename", Basic, modals::editor_fs_prompt),
    capture!("49-EditorDeleteConfirm", "Editor tree menu Delete", Basic, modals::editor_delete_confirm),
    capture!("50-TrustRepo", "Foreign-owned repo → trust prompt (test-only state setter)", Basic, modals::trust_repo),
    capture!("51-BranchPicker-checkout", "branch.checkout command → branch picker", Branches, popups::branch_picker_checkout),
    capture!("52-BranchPicker-delete", "branch.delete command → branch picker", Branches, popups::branch_picker_delete),
    capture!("53-Info-About", "app.about command → About overlay", Basic, popups::info_about),
    capture!("54-Info-Shortcuts", "help.shortcuts command → shortcuts overlay", Basic, popups::info_shortcuts),
    capture!("55-Settings", "app.settings command → Settings overlay", Basic, popups::settings),
    capture!("56-CommandPalette", "Command palette action → search overlay", Basic, popups::command_palette),
    capture!("57-CommitMenu", "Right-click commit row", Basic, popups::commit_menu),
    capture!("58-BranchMenu", "Right-click local branch", Branches, popups::branch_menu),
    capture!("59-RemoteBranchMenu", "Right-click remote branch", RemoteBranch, popups::remote_branch_menu),
    capture!("60-TagMenu", "Right-click tag", Tag, popups::tag_menu),
    capture!("61-StashMenu", "Right-click stash", Stash, popups::stash_menu),
    capture!("62-WorktreeMenu", "Right-click linked worktree", LinkedWorktree, popups::worktree_menu),
    capture!("63-CommitPlan", "Commit with safety blocker → review plan", CommitBlocker, popups::commit_plan),
    capture!("64-WorktreeInspection", "Hover linked worktree row → inspect card", LinkedWorktree, popups::worktree_inspection),
    capture!("65-Toast", "Operation feedback → toast stack", Basic, popups::toast),
    capture!("66-BusySnackbar", "Slow ahead/behind read → busy corner banner with Skip", SlowRead, popups::busy_snackbar),
    capture!("67-EditorTreeMenu", "Right-click editor tree root", Basic, popups::editor_tree_menu),
    capture!("68-ConflictFileMenu", "Right-click unresolved conflict file", Conflict, popups::conflict_file_menu),
    capture!("69-FilterMenu", "List filter control → dropdown", Basic, popups::filter_menu),
    capture!("70-FileMenu", "Right-click changed file", Dirty, popups::file_menu),
    capture!("71-InspectorFileMenu", "Right-click inspector file", Basic, popups::inspector_file_menu),
    capture!("72-PrMenu", "PR row → context menu", Basic, popups::pr_menu),
    capture!("73-CoauthorMenu", "Commit panel co-author control", Basic, popups::coauthor_menu),
    skip!("74-StashPlanning", "Open Stash before background plan resolves", "Stash planner completes asynchronously before the runner's settled native screenshot; unlike snapshot reads it has no deterministic gui-e2e hold seam, so a PNG would depict a different state"),
];
