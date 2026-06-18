//! Commit context menu model and overlay renderer.

use gpui::{ClipboardItem, Context, Pixels, Point, SharedString, Window};

use kagi::git::CommitId;

use super::i18n::Msg;
use super::{
    commit_list::{BadgeKind, RefBadge},
    menu_overlay, FooterStatus, KagiApp,
};

// W9-THEME: all colours come from `theme()` (see theme.rs).

const MENU_W: f32 = 280.0;

#[derive(Clone, Debug)]
pub struct CommitMenuState {
    pub row_index: usize,
    pub position: Point<Pixels>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitAction {
    ShowDetails,
    CopySha,
    CopyShortSha,
    CopyMessage,
    CreateBranchHere,
    CreateWorktreeHere,
    CherryPick,
    Revert,
    CheckoutCommit,
    CheckoutRef(String),
    /// Checkout a remote-only branch (e.g. `origin/feature`) by creating a local
    /// tracking branch and switching to it (ADR mirrors the sidebar flow).
    CheckoutTrackingBranch(String),
    CompareWithHead,
    CompareWithWorkingTree,
    ShowChangedFiles,
    ResetToCommit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    Enabled,
    Disabled(SharedString),
    Hidden,
}

#[derive(Clone, Debug)]
pub struct MenuItem<A = CommitAction> {
    pub action: A,
    pub label: SharedString,
    pub state: ItemState,
    pub dangerous: bool,
}

#[derive(Clone, Debug)]
pub struct MenuGroup<A = CommitAction> {
    pub title: Option<&'static str>,
    pub items: Vec<MenuItem<A>>,
}

#[derive(Clone, Debug)]
pub struct MenuContext {
    pub is_head: bool,
    pub is_ancestor_of_head: bool,
    pub is_merge: bool,
    pub dirty: bool,
    pub detached: bool,
    pub has_local_changes: bool,
    pub refs_here: Vec<RefBadge>,
    /// Names of all LOCAL branches in the repo (used to tell whether a remote
    /// badge here is remote-only → offer "checkout as local branch").
    pub local_branches: Vec<String>,
}

pub fn build_commit_menu(ctx: &MenuContext) -> Vec<MenuGroup> {
    let checkout_ref = primary_checkout_ref(&ctx.refs_here);
    let checkout_ref_item = match checkout_ref {
        Some(label) => item(
            CommitAction::CheckoutRef(label.clone()),
            format!("Checkout '{}'...", label),
            ItemState::Enabled,
            false,
        ),
        None => item(
            CommitAction::CheckoutRef(String::new()),
            "Checkout branch/tag here...",
            ItemState::Hidden,
            false,
        ),
    };

    // Remote-only badges at this commit (no local branch of the same name) get a
    // "checkout as local branch" item — creates a local tracking branch and
    // switches to it (mirrors the sidebar remote-branch menu).
    let remote_checkout_items: Vec<MenuItem> = ctx
        .refs_here
        .iter()
        .filter(|b| b.kind == BadgeKind::Remote)
        .filter(|b| {
            let local = remote_local_name(b.label.as_ref());
            !ctx.local_branches.iter().any(|n| n == local)
        })
        .map(|b| {
            let full = b.label.as_ref().to_string();
            item(
                CommitAction::CheckoutTrackingBranch(full.clone()),
                format!("Checkout '{}' as local branch...", full),
                ItemState::Enabled,
                false,
            )
        })
        .collect();

    vec![
        MenuGroup {
            title: Some("Inspect"),
            items: vec![
                item(
                    CommitAction::ShowDetails,
                    "Show commit details",
                    ItemState::Enabled,
                    false,
                ),
                item(CommitAction::CopySha, "Copy SHA", ItemState::Enabled, false),
                item(
                    CommitAction::CopyShortSha,
                    "Copy short SHA",
                    ItemState::Enabled,
                    false,
                ),
                item(
                    CommitAction::CopyMessage,
                    "Copy commit message",
                    ItemState::Enabled,
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Create from this commit"),
            items: vec![
                item(
                    CommitAction::CreateBranchHere,
                    if ctx.detached {
                        "Create branch here... (recommended)"
                    } else {
                        "Create branch here..."
                    },
                    ItemState::Enabled,
                    false,
                ),
                item(
                    CommitAction::CreateWorktreeHere,
                    "Create worktree here...",
                    ItemState::Enabled,
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Apply changes"),
            items: vec![
                item(
                    CommitAction::CherryPick,
                    if ctx.dirty {
                        "Cherry-pick onto current branch... (dirty)"
                    } else {
                        "Cherry-pick onto current branch..."
                    },
                    cherry_pick_state(ctx),
                    false,
                ),
                item(
                    CommitAction::Revert,
                    if ctx.dirty {
                        "Revert this commit... (dirty)"
                    } else {
                        "Revert this commit..."
                    },
                    revert_state(ctx),
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Checkout / Move"),
            items: {
                let mut items = vec![
                    item(
                        CommitAction::CheckoutCommit,
                        checkout_commit_label(ctx),
                        checkout_commit_state(ctx),
                        false,
                    ),
                    checkout_ref_item,
                ];
                items.extend(remote_checkout_items);
                items
            },
        },
        MenuGroup {
            title: Some("Compare"),
            items: vec![
                item(
                    CommitAction::CompareWithHead,
                    "Compare with HEAD",
                    compare_head_state(ctx),
                    false,
                ),
                item(
                    CommitAction::CompareWithWorkingTree,
                    "Compare with working tree",
                    compare_working_tree_state(ctx),
                    false,
                ),
                item(
                    CommitAction::ShowChangedFiles,
                    "Show changed files",
                    ItemState::Enabled,
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Advanced / Dangerous"),
            items: vec![item(
                CommitAction::ResetToCommit,
                "Reset current branch to this commit...",
                reset_state(ctx),
                true,
            )],
        },
    ]
}

pub fn log_commit_menu(row_index: usize, groups: &[MenuGroup]) {
    let parts: Vec<String> = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .filter(|item| item.state != ItemState::Hidden)
        .map(|item| match &item.state {
            ItemState::Enabled => format!("{}=enabled", item.label.as_ref()),
            ItemState::Disabled(reason) => {
                format!("{}=disabled({})", item.label.as_ref(), reason.as_ref())
            }
            ItemState::Hidden => unreachable!(),
        })
        .collect();
    eprintln!(
        "[kagi] context-menu: row={} items={}",
        row_index,
        parts.join("; ")
    );
}

pub fn render_commit_menu_overlay(
    state: CommitMenuState,
    target: CommitId,
    header: SharedString,
    groups: Vec<MenuGroup>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.commit_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: CommitAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.commit_menu = None;
        this.dispatch_commit_action(action, target.clone(), window, cx);
    };
    menu_overlay::render_menu_overlay(
        "commit-context-menu",
        "commit-menu-item",
        MENU_W,
        "Advanced / Dangerous",
        state.position,
        header,
        groups,
        on_dismiss,
        on_select,
        window,
        cx,
    )
}

pub fn short_title_header(full_sha: &str, title: &str) -> SharedString {
    let short: String = full_sha.chars().take(8).collect();
    let title = truncate_chars(title, 54);
    SharedString::from(format!("{} {}", short, title))
}

pub fn copy_full_sha(app: &mut KagiApp, full_sha: String, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    cx.write_to_clipboard(ClipboardItem::new_string(full_sha));
    klog!("copy-sha: {}", short);
    app.status_footer = FooterStatus::Idle(SharedString::from("SHA copied"));
}

pub fn copy_short_sha(app: &mut KagiApp, full_sha: &str, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    cx.write_to_clipboard(ClipboardItem::new_string(short.clone()));
    klog!("copy-short-sha: {}", short);
    app.status_footer = FooterStatus::Idle(SharedString::from("Short SHA copied"));
}

pub fn copy_message(app: &mut KagiApp, full_sha: &str, message: String, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    let len = message.chars().count();
    cx.write_to_clipboard(ClipboardItem::new_string(message));
    klog!("copy-message: {} len={}", short, len);
    app.status_footer = FooterStatus::Idle(SharedString::from("Commit message copied"));
}

fn item(
    action: CommitAction,
    label: impl Into<String>,
    state: ItemState,
    dangerous: bool,
) -> MenuItem {
    MenuItem {
        action,
        label: SharedString::from(label.into()),
        state,
        dangerous,
    }
}

fn disabled(reason: impl Into<SharedString>) -> ItemState {
    ItemState::Disabled(reason.into())
}

fn cherry_pick_state(ctx: &MenuContext) -> ItemState {
    if ctx.detached {
        disabled(Msg::CmDetachedHead.t())
    } else if ctx.is_head {
        disabled(Msg::CmSameAsHead.t())
    } else if ctx.is_merge {
        disabled(Msg::CmMergeUnsupported.t())
    } else if ctx.is_ancestor_of_head {
        disabled(Msg::CmAlreadyInBranch.t())
    } else {
        ItemState::Enabled
    }
}

fn revert_state(ctx: &MenuContext) -> ItemState {
    if ctx.detached {
        disabled(Msg::CmDetachedHead.t())
    } else if ctx.is_merge {
        disabled(Msg::CmMergeUnsupported.t())
    } else if !ctx.is_ancestor_of_head {
        disabled(Msg::CmNotInBranch.t())
    } else {
        ItemState::Enabled
    }
}

fn checkout_commit_state(ctx: &MenuContext) -> ItemState {
    if ctx.is_head {
        disabled(Msg::CmAlreadyHead.t())
    } else {
        ItemState::Enabled
    }
}

fn checkout_commit_label(ctx: &MenuContext) -> &'static str {
    if ctx.dirty {
        "Checkout this commit... (dirty working tree)"
    } else if ctx.is_head {
        "Checkout this commit..."
    } else {
        "Checkout this commit... (detached HEAD)"
    }
}

fn compare_head_state(ctx: &MenuContext) -> ItemState {
    if ctx.is_head {
        disabled(Msg::CmIdentical.t())
    } else {
        ItemState::Enabled
    }
}

fn compare_working_tree_state(ctx: &MenuContext) -> ItemState {
    if ctx.has_local_changes {
        ItemState::Enabled
    } else {
        disabled(Msg::CmNoLocalChanges.t())
    }
}

fn reset_state(ctx: &MenuContext) -> ItemState {
    if ctx.is_head {
        disabled(Msg::CmResetUnneeded.t())
    } else if ctx.detached {
        disabled(Msg::CmNoCurrentBranch.t())
    } else {
        disabled(Msg::CmResetUnimplemented.t())
    }
}

/// The local branch name a remote badge would map to: `origin/feature` →
/// `feature` (strips the first path segment, the remote name).
fn remote_local_name(label: &str) -> &str {
    label.split_once('/').map(|(_, name)| name).unwrap_or(label)
}

fn primary_checkout_ref(refs: &[RefBadge]) -> Option<String> {
    refs.iter()
        .find(|badge| badge.kind == BadgeKind::Branch)
        .or_else(|| refs.iter().find(|badge| badge.kind == BadgeKind::Tag))
        .map(|badge| badge.label.as_ref().trim_end_matches(" ✓").to_string())
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        let s: String = input.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MenuContext {
        MenuContext {
            is_head: false,
            is_ancestor_of_head: false,
            is_merge: false,
            dirty: false,
            detached: false,
            has_local_changes: false,
            refs_here: Vec::new(),
            local_branches: Vec::new(),
        }
    }

    fn state_for(groups: &[MenuGroup], action: CommitAction) -> ItemState {
        groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.action == action)
            .map(|item| item.state.clone())
            .expect("action must exist")
    }

    fn checkout_ref_state(groups: &[MenuGroup]) -> ItemState {
        groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| matches!(item.action, CommitAction::CheckoutRef(_)))
            .map(|item| item.state.clone())
            .expect("checkout ref item must exist")
    }

    fn assert_enabled(groups: &[MenuGroup], action: CommitAction) {
        assert_eq!(state_for(groups, action), ItemState::Enabled);
    }

    fn assert_disabled_contains(groups: &[MenuGroup], action: CommitAction, needle: &str) {
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

    #[test]
    fn requirements_head_selection_row() {
        let mut c = ctx();
        c.is_head = true;
        c.is_ancestor_of_head = true;
        c.has_local_changes = true;
        let groups = build_commit_menu(&c);

        assert_enabled(&groups, CommitAction::ShowDetails);
        assert_disabled_contains(&groups, CommitAction::CherryPick, "HEAD");
        assert_enabled(&groups, CommitAction::Revert);
        assert_disabled_contains(&groups, CommitAction::CheckoutCommit, "HEAD");
        // i18n: default test language is En; assert the English Msg substring.
        assert_disabled_contains(&groups, CommitAction::CompareWithHead, "identical");
        assert_enabled(&groups, CommitAction::CompareWithWorkingTree);
        assert_disabled_contains(&groups, CommitAction::ResetToCommit, "HEAD");
    }

    #[test]
    fn requirements_past_commit_row() {
        let mut c = ctx();
        c.is_ancestor_of_head = true;
        let groups = build_commit_menu(&c);

        assert_disabled_contains(
            &groups,
            CommitAction::CherryPick,
            "already in the current branch",
        );
        assert_enabled(&groups, CommitAction::Revert);
        assert_enabled(&groups, CommitAction::CheckoutCommit);
        assert_enabled(&groups, CommitAction::CompareWithHead);
        assert_disabled_contains(
            &groups,
            CommitAction::CompareWithWorkingTree,
            "local changes",
        );
    }

    #[test]
    fn requirements_other_branch_row() {
        let groups = build_commit_menu(&ctx());

        assert_enabled(&groups, CommitAction::CherryPick);
        assert_disabled_contains(&groups, CommitAction::Revert, "not in the current branch");
        assert_enabled(&groups, CommitAction::CheckoutCommit);
        assert_enabled(&groups, CommitAction::CompareWithHead);
    }

    #[test]
    fn requirements_merge_commit_row() {
        let mut c = ctx();
        c.is_merge = true;
        c.is_ancestor_of_head = true;
        let groups = build_commit_menu(&c);

        assert_disabled_contains(&groups, CommitAction::CherryPick, "merge commit");
        assert_disabled_contains(&groups, CommitAction::Revert, "merge commit");
        assert_enabled(&groups, CommitAction::CheckoutCommit);
        assert_enabled(&groups, CommitAction::CompareWithHead);
    }

    #[test]
    fn requirements_dirty_worktree_row() {
        let mut c = ctx();
        c.dirty = true;
        c.has_local_changes = true;
        let groups = build_commit_menu(&c);

        assert_enabled(&groups, CommitAction::CreateBranchHere);
        assert_enabled(&groups, CommitAction::CherryPick);
        assert_enabled(&groups, CommitAction::CheckoutCommit);
        assert_enabled(&groups, CommitAction::CompareWithWorkingTree);
    }

    #[test]
    fn requirements_detached_head_row() {
        let mut c = ctx();
        c.detached = true;
        c.refs_here = vec![RefBadge {
            kind: BadgeKind::Tag,
            label: SharedString::from("v1.0.0"),
        }];
        let groups = build_commit_menu(&c);

        assert_enabled(&groups, CommitAction::CreateBranchHere);
        assert_disabled_contains(&groups, CommitAction::CherryPick, "detached");
        assert_disabled_contains(&groups, CommitAction::Revert, "detached");
        assert_enabled(&groups, CommitAction::CheckoutCommit);
        assert_eq!(checkout_ref_state(&groups), ItemState::Enabled);
        assert_disabled_contains(&groups, CommitAction::ResetToCommit, "branch");
    }

    #[test]
    fn hidden_checkout_ref_is_not_drawn_without_refs_here() {
        let groups = build_commit_menu(&ctx());
        assert_eq!(checkout_ref_state(&groups), ItemState::Hidden);
    }

    #[test]
    fn checkout_ref_prefers_local_branch_here() {
        let mut c = ctx();
        c.refs_here = vec![
            RefBadge {
                kind: BadgeKind::Tag,
                label: SharedString::from("v1.0.0"),
            },
            RefBadge {
                kind: BadgeKind::Branch,
                label: SharedString::from("feature/checkout ✓"),
            },
        ];
        let groups = build_commit_menu(&c);
        let item = groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| matches!(item.action, CommitAction::CheckoutRef(_)))
            .expect("checkout ref item");

        assert_eq!(item.state, ItemState::Enabled);
        assert_eq!(item.label.as_ref(), "Checkout 'feature/checkout'...");
        assert_eq!(
            item.action,
            CommitAction::CheckoutRef("feature/checkout".to_string())
        );
    }

    #[test]
    fn truncates_header_by_chars() {
        let header = short_title_header("1234567890", "日本語メッセージがとても長いので切り詰める");
        assert!(header.as_ref().starts_with("12345678 日本語"));
    }

    fn has_tracking_checkout(groups: &[MenuGroup], remote: &str) -> bool {
        groups.iter().flat_map(|g| g.items.iter()).any(|i| {
            matches!(&i.action, CommitAction::CheckoutTrackingBranch(n) if n == remote)
                && i.state == ItemState::Enabled
        })
    }

    #[test]
    fn remote_only_badge_offers_tracking_checkout() {
        let mut c = ctx();
        c.refs_here = vec![RefBadge {
            kind: BadgeKind::Remote,
            label: "origin/feature".into(),
        }];
        // No local `feature` → offer "checkout as local branch".
        c.local_branches = vec!["main".to_string()];
        let groups = build_commit_menu(&c);
        assert!(has_tracking_checkout(&groups, "origin/feature"));
    }

    #[test]
    fn remote_badge_with_local_counterpart_has_no_tracking_checkout() {
        let mut c = ctx();
        c.refs_here = vec![RefBadge {
            kind: BadgeKind::Remote,
            label: "origin/feature".into(),
        }];
        // A local `feature` already exists → no tracking-checkout item.
        c.local_branches = vec!["main".to_string(), "feature".to_string()];
        let groups = build_commit_menu(&c);
        assert!(!has_tracking_checkout(&groups, "origin/feature"));
    }
}
