use super::*;

fn ctx() -> BranchMenuContext {
    BranchMenuContext {
        name: "feature/x".to_string(),
        head_sha: "1234567890abcdef".to_string(),
        kind: BranchKind::Local,
        is_current: false,
        has_upstream: true,
        upstream_name: Some("origin/feature/x".to_string()),
        ahead: Some(2),
        behind: Some(3),
        dirty: false,
        conflict_mode: BranchConflictMode::None,
        protected: false,
        checked_out_in_other_worktree: false,
        checked_out_worktree_path: None,
        worktree_path: None,
        merged_into_current: true,
        is_pushed: true,
        detached_head: false,
        busy: false,
        current_branch: Some("main".to_string()),
        is_soloed: false,
    }
}

fn item_for(groups: &[MenuGroup<BranchAction>], action: BranchAction) -> &MenuItem<BranchAction> {
    MenuItem::find_action(groups, &action).expect("action must exist")
}

fn state_for(groups: &[MenuGroup<BranchAction>], action: BranchAction) -> ItemState {
    item_for(groups, action).state.clone()
}

fn assert_enabled(groups: &[MenuGroup<BranchAction>], action: BranchAction) {
    assert_eq!(state_for(groups, action), ItemState::Enabled);
}

fn assert_hidden(groups: &[MenuGroup<BranchAction>], action: BranchAction) {
    assert_eq!(state_for(groups, action), ItemState::Hidden);
}

fn assert_disabled_contains(
    groups: &[MenuGroup<BranchAction>],
    action: BranchAction,
    needle: &str,
) {
    match state_for(groups, action) {
        ItemState::Disabled(reason) => assert!(
            reason.as_ref().contains(needle),
            "reason {:?} must contain {:?}",
            reason,
            needle
        ),
        other => panic!("expected disabled, got {:?}", other),
    }
}

/// #473: a branch checked out in ANOTHER worktree gets an "Open worktree"
/// item naming that directory — Checkout is disabled for it, so without
/// this the menu offered no route to that working tree.
#[test]
fn open_worktree_dir_only_when_checked_out_elsewhere() {
    let plain = branch_context_menu_items(&ctx());
    assert!(
        MenuItem::find_action(&plain, &BranchAction::OpenWorktreeDir).is_none(),
        "no worktree → no Open worktree item"
    );

    let mut c = ctx();
    c.worktree_path = Some(std::path::PathBuf::from("/tmp/wt-ahead"));
    let groups = branch_context_menu_items(&c);
    assert_enabled(&groups, BranchAction::OpenWorktreeDir);
    assert!(
        item_for(&groups, BranchAction::OpenWorktreeDir)
            .label
            .as_ref()
            .contains("/tmp/wt-ahead"),
        "the item must name the worktree directory"
    );
}

#[test]
fn local_non_current_with_upstream() {
    let groups = branch_context_menu_items(&ctx());

    assert_enabled(&groups, BranchAction::Checkout);
    assert_enabled(&groups, BranchAction::Pull);
    assert_enabled(&groups, BranchAction::Push);
    assert_enabled(&groups, BranchAction::SetUpstream);
    assert_enabled(&groups, BranchAction::RenameBranch);
    assert_enabled(&groups, BranchAction::RevealHead);
    assert_enabled(&groups, BranchAction::ToggleSolo);
    assert_enabled(&groups, BranchAction::CreateBranchFromHere);
    assert_enabled(&groups, BranchAction::DeleteBranch);
    assert_enabled(&groups, BranchAction::CopyBranchName);
    assert_enabled(&groups, BranchAction::CopyHeadSha);
    assert_enabled(&groups, BranchAction::CopyUpstreamName);
    assert_enabled(&groups, BranchAction::MergeIntoCurrent);
    assert_eq!(
        item_for(&groups, BranchAction::Pull).label.as_ref(),
        "Pull ↓3"
    );
    assert_eq!(
        item_for(&groups, BranchAction::Push).label.as_ref(),
        "Push ↑2"
    );
}

#[test]
fn local_non_current_without_upstream() {
    let mut c = ctx();
    c.has_upstream = false;
    c.upstream_name = None;
    c.ahead = Some(0);
    c.behind = Some(0);
    let groups = branch_context_menu_items(&c);

    assert_disabled_contains(&groups, BranchAction::Pull, "upstream");
    assert_hidden(&groups, BranchAction::Push);
    assert_enabled(&groups, BranchAction::PushAndCreateUpstream);
    assert_disabled_contains(&groups, BranchAction::NoUpstreamInfo, "upstream");
    assert_disabled_contains(&groups, BranchAction::CopyUpstreamName, "upstream");
}

#[test]
fn current_branch_disables_checkout_and_delete() {
    let mut c = ctx();
    c.is_current = true;
    let groups = branch_context_menu_items(&c);

    assert_disabled_contains(&groups, BranchAction::Checkout, "current");
    assert_disabled_contains(&groups, BranchAction::DeleteBranch, "current");
}

#[test]
fn remote_branch_hides_local_delete() {
    let mut c = ctx();
    c.name = "origin/feature/x".to_string();
    c.kind = BranchKind::Remote;
    c.has_upstream = false;
    c.upstream_name = None;
    let groups = branch_context_menu_items(&c);

    assert_enabled(&groups, BranchAction::Checkout);
    assert_enabled(&groups, BranchAction::MergeIntoCurrent);
    assert_hidden(&groups, BranchAction::DeleteBranch);
    assert_hidden(&groups, BranchAction::RenameBranch);
    assert_hidden(&groups, BranchAction::SetUpstream);
    assert_enabled(&groups, BranchAction::DeleteRemoteBranch);
}

#[test]
fn busy_keeps_queueable_actions_enabled() {
    let mut c = ctx();
    c.busy = true;
    let groups = branch_context_menu_items(&c);

    // Local checkout and merge freeze their targets for the queue (#355).
    assert_enabled(&groups, BranchAction::Checkout);
    assert_enabled(&groups, BranchAction::MergeIntoCurrent);
    assert_disabled_contains(&groups, BranchAction::CreateBranchFromHere, "operation");
    assert_disabled_contains(&groups, BranchAction::Pull, "operation");
    assert_disabled_contains(&groups, BranchAction::Push, "operation");
    assert_disabled_contains(&groups, BranchAction::SetUpstream, "operation");
    assert_disabled_contains(&groups, BranchAction::RenameBranch, "operation");
    assert_disabled_contains(&groups, BranchAction::DeleteBranch, "operation");
    assert_enabled(&groups, BranchAction::RevealHead);
    assert_enabled(&groups, BranchAction::ToggleSolo);
    assert_enabled(&groups, BranchAction::CopyBranchName);
    assert_enabled(&groups, BranchAction::CopyHeadSha);
}

#[test]
fn busy_still_disables_checking_out_a_remote_branch() {
    let mut c = ctx();
    c.busy = true;
    c.kind = BranchKind::Remote;
    let groups = branch_context_menu_items(&c);
    assert_disabled_contains(&groups, BranchAction::Checkout, "operation");
}

#[test]
fn solo_item_toggles_label_when_active() {
    let groups = branch_context_menu_items(&ctx());
    assert_eq!(
        item_for(&groups, BranchAction::ToggleSolo).label.as_ref(),
        "Solo"
    );

    let mut c = ctx();
    c.is_soloed = true;
    let groups = branch_context_menu_items(&c);
    assert_eq!(
        item_for(&groups, BranchAction::ToggleSolo).label.as_ref(),
        "Exit Solo"
    );
}

#[test]
fn cached_counts_do_not_block_upstream_pull() {
    for is_current in [false, true] {
        for behind in [Some(0), None, Some(3)] {
            let mut c = ctx();
            c.is_current = is_current;
            c.behind = behind;
            let groups = branch_context_menu_items(&c);
            for action in [BranchAction::Pull, BranchAction::PullFfOnly] {
                assert_enabled(&groups, action);
            }
        }
    }
}

#[test]
fn cached_zero_and_unknown_pull_keep_structural_guards() {
    for behind in [Some(0), None] {
        for guard in 0..4 {
            let mut c = ctx();
            c.behind = behind;
            match guard {
                0 => c.busy = true,
                1 => c.detached_head = true,
                2 => c.kind = BranchKind::Remote,
                _ => c.has_upstream = false,
            }
            let groups = branch_context_menu_items(&c);
            for action in [BranchAction::Pull, BranchAction::PullFfOnly] {
                assert!(matches!(state_for(&groups, action), ItemState::Disabled(_)));
            }
        }
    }
}

#[test]
fn zero_ahead_still_disables_push() {
    let mut c = ctx();
    c.ahead = Some(0);
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::Push,
        "nothing",
    );
}

/// #355: a skipped count is unknown, not zero.
#[test]
fn unknown_ahead_keeps_push_enabled() {
    let mut c = ctx();
    c.ahead = None;
    assert_enabled(&branch_context_menu_items(&c), BranchAction::Push);
}

#[test]
fn remote_branch_sync_and_manage_availability() {
    let mut c = ctx();
    c.name = "origin/feature/x".to_string();
    c.kind = BranchKind::Remote;
    c.has_upstream = false;
    c.upstream_name = None;
    c.ahead = Some(0);
    c.behind = Some(0);
    let groups = branch_context_menu_items(&c);

    assert_hidden(&groups, BranchAction::Push);
    assert_hidden(&groups, BranchAction::PushAndCreateUpstream);
    assert_hidden(&groups, BranchAction::SetUpstream);
    assert_hidden(&groups, BranchAction::RenameBranch);
    assert_hidden(&groups, BranchAction::DeleteBranch);
    assert_enabled(&groups, BranchAction::DeleteRemoteBranch);
}

#[test]
fn merge_and_rebase_labels_include_direction() {
    let groups = branch_context_menu_items(&ctx());

    assert_eq!(
        item_for(&groups, BranchAction::MergeIntoCurrent)
            .label
            .as_ref(),
        "Merge feature/x into main"
    );
    assert_eq!(
        item_for(&groups, BranchAction::RebaseCurrentOnto)
            .label
            .as_ref(),
        "Rebase main onto feature/x"
    );
}

#[test]
fn conflict_mode_disables_integrate_items() {
    let mut c = ctx();
    c.conflict_mode = BranchConflictMode::Conflicted;
    let groups = branch_context_menu_items(&c);

    assert_disabled_contains(&groups, BranchAction::MergeIntoCurrent, "conflicts");
    assert_disabled_contains(&groups, BranchAction::RebaseCurrentOnto, "conflicts");
    assert_disabled_contains(&groups, BranchAction::ReplayOnto, "conflicts");
}

/// #536: sync is current-branch-only, needs an upstream, is dangerous.
#[test]
fn sync_to_remote_item_gating() {
    let mut c = ctx();
    c.is_current = true;
    c.has_upstream = true;
    let groups = branch_context_menu_items(&c);
    let item = item_for(&groups, BranchAction::SyncToRemote);
    assert_eq!(item.label.as_ref(), "Sync to remote (keep local)…");
    assert!(item.dangerous);
    assert_enabled(&groups, BranchAction::SyncToRemote);

    let mut c = ctx();
    c.is_current = false;
    c.has_upstream = true;
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::SyncToRemote,
        "current branch only",
    );
    let mut c = ctx();
    c.is_current = true;
    c.has_upstream = false;
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::SyncToRemote,
        "upstream",
    );
}

/// #344: replay moves the *clicked* branch onto the current one — the
/// label reads in that direction, the item is dangerous (history
/// rewrite), and only a non-current local branch can be replayed.
#[test]
fn replay_item_direction_and_gating() {
    let groups = branch_context_menu_items(&ctx());
    let item = item_for(&groups, BranchAction::ReplayOnto);
    assert_eq!(item.label.as_ref(), "Replay feature/x onto main…");
    assert!(item.dangerous);
    assert_enabled(&groups, BranchAction::ReplayOnto);

    let mut c = ctx();
    c.is_current = true;
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::ReplayOnto,
        "current branch",
    );
    let mut c = ctx();
    c.kind = BranchKind::Remote;
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::ReplayOnto,
        "local branches only",
    );
    let mut c = ctx();
    c.detached_head = true;
    assert_disabled_contains(
        &branch_context_menu_items(&c),
        BranchAction::ReplayOnto,
        "detached",
    );
}

/// The pre-#1238 command IDs remain in the hierarchy, independently of availability.
#[test]
fn compact_menu_retains_every_previous_command() {
    use BranchAction::*;
    let previous = [
        Checkout,
        SwitchToLatest,
        OpenWorktreeFromBranch,
        OpenWorktreeDir,
        RevealHead,
        ToggleSolo,
        Pull,
        PullFfOnly,
        SyncToRemote,
        Push,
        PushAndCreateUpstream,
        SetUpstream,
        NoUpstreamInfo,
        FetchRemoteBranch,
        CreatePr,
        MergeIntoCurrent,
        RebaseCurrentOnto,
        ReplayOnto,
        CreateBranchFromHere,
        CreateWorktreeFromHere,
        CreateTagHere,
        RenameBranch,
        DeleteBranch,
        CopyBranchName,
        CopyHeadSha,
        CopyUpstreamName,
        ResetCurrentToHead,
        ForceWithLeasePush,
        DeleteRemoteBranch,
    ];
    for is_current in [false, true] {
        for kind in [BranchKind::Local, BranchKind::Remote] {
            let mut context = ctx();
            context.is_current = is_current;
            context.kind = kind;
            context.worktree_path = Some("/tmp/linked".into());
            let groups = branch_context_menu_items(&context);
            for action in &previous {
                assert!(
                    MenuItem::find_action(&groups, action).is_some(),
                    "missing {action:?}"
                );
            }
            let rows = groups
                .iter()
                .flat_map(|group| &group.items)
                .filter(|item| item.state != ItemState::Hidden)
                .count();
            assert!(rows <= 10, "{rows} top-level rows");
            assert!(groups.iter().all(|group| group.title.is_none()));
        }
    }
}
