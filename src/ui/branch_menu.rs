//! Branch context menu model and overlay renderer.

use gpui::{ClipboardItem, Context, Pixels, Point, SharedString, Window};

use kagi_git::CommitId;

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    i18n::{self, Msg},
    menu_overlay, KagiApp,
};

#[path = "branch_menu_availability.rs"]
mod availability;
use availability::*;

const MENU_W: f32 = 300.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchConflictMode {
    None,
    Conflicted,
}

#[derive(Clone, Debug)]
pub struct BranchMenuState {
    pub name: String,
    pub target: CommitId,
    pub kind: BranchKind,
    pub position: Point<Pixels>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchAction {
    Checkout,
    SwitchToLatest,
    OpenWorktreeFromBranch,
    /// #473: open the worktree this branch is already checked out in, as its
    /// own tab. Only offered when `worktree_path` is set.
    OpenWorktreeDir,
    RevealHead,
    ToggleSolo,
    Pull,
    PullFfOnly,
    /// #536: sync to remote keeping local work in backups.
    SyncToRemote,
    Push,
    PushAndCreateUpstream,
    SetUpstream,
    NoUpstreamInfo,
    FetchRemoteBranch,
    Fetch,
    CreatePr,
    MergeIntoCurrent,
    RebaseCurrentOnto,
    /// #344: move the clicked branch onto the current one by ref update
    /// only (`git replay`), without checking it out.
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
}

#[derive(Clone, Debug)]
pub struct BranchMenuContext {
    pub name: String,
    pub head_sha: String,
    pub kind: BranchKind,
    pub is_current: bool,
    pub has_upstream: bool,
    pub upstream_name: Option<String>,
    /// Counts reflect the last fetch, not live upstream freshness. Pull stays
    /// enabled even at zero; an unknown count is re-read during planning (#355).
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub dirty: bool,
    pub conflict_mode: BranchConflictMode,
    pub protected: bool,
    pub checked_out_in_other_worktree: bool,
    pub checked_out_worktree_path: Option<String>,
    /// #473: the working-tree path of the OTHER worktree this branch is checked
    /// out in (`None` for the current worktree, or when it is not checked out
    /// anywhere else). Drives the "Open worktree" item.
    pub worktree_path: Option<std::path::PathBuf>,
    pub merged_into_current: bool,
    pub is_pushed: bool,
    pub detached_head: bool,
    pub busy: bool,
    pub current_branch: Option<String>,
    pub is_soloed: bool,
}

pub fn branch_context_menu_items(ctx: &BranchMenuContext) -> Vec<MenuGroup<BranchAction>> {
    let mut checkout_label = match ctx.kind {
        BranchKind::Local => format!("Checkout {}", ctx.name),
        BranchKind::Remote => format!("Checkout {} as local branch", ctx.name),
    };
    if ctx.dirty {
        checkout_label.push_str(" (dirty working tree)");
    }
    if ctx.conflict_mode == BranchConflictMode::Conflicted {
        checkout_label.push_str(" (conflicts)");
    }
    vec![
        group(vec![item(
            BranchAction::Checkout,
            checkout_label,
            checkout_state(ctx),
            false,
        )]),
        group(primary_sync_items(ctx)),
        group(vec![
            item(
                BranchAction::CreateBranchFromHere,
                "Create branch here…",
                create_branch_state(ctx),
                false,
            ),
            item(
                BranchAction::RenameBranch,
                "Rename branch…",
                rename_state(ctx),
                false,
            ),
            item(
                BranchAction::DeleteBranch,
                delete_label(ctx),
                delete_state(ctx),
                true,
            ),
        ]),
        group(vec![
            MenuItem::submenu(Msg::BcmIntegrateMenu.t(), integrate_items(ctx)),
            MenuItem::submenu(Msg::BcmWorktreeMenu.t(), worktree_items(ctx)),
            MenuItem::submenu(Msg::BcmAdvancedMenu.t(), advanced_items(ctx)),
        ]),
    ]
}

fn group(items: Vec<MenuItem<BranchAction>>) -> MenuGroup<BranchAction> {
    MenuGroup { title: None, items }
}

fn primary_sync_items(ctx: &BranchMenuContext) -> Vec<MenuItem<BranchAction>> {
    if ctx.is_current {
        vec![
            item(BranchAction::Pull, pull_label(ctx), pull_state(ctx), false),
            item(BranchAction::Push, push_label(ctx), push_state(ctx), false),
            item(
                BranchAction::Fetch,
                "Fetch",
                if ctx.busy {
                    disabled(Msg::BcmBusy.t())
                } else {
                    ItemState::Enabled
                },
                false,
            ),
        ]
    } else {
        vec![item(
            BranchAction::FetchRemoteBranch,
            "Fetch remote branch",
            fetch_remote_branch_state(ctx),
            false,
        )]
    }
}

fn integrate_items(ctx: &BranchMenuContext) -> Vec<MenuGroup<BranchAction>> {
    let current = ctx.current_branch.as_deref().unwrap_or("current branch");
    vec![group(vec![
        item(
            BranchAction::MergeIntoCurrent,
            format!("Merge {} into {}", ctx.name, current),
            merge_state(ctx),
            false,
        ),
        item(
            BranchAction::RebaseCurrentOnto,
            format!("Rebase {} onto {}", current, ctx.name),
            rebase_state(ctx),
            false,
        ),
        item(
            BranchAction::ReplayOnto,
            format!("Replay {} onto {}…", ctx.name, current),
            replay_state(ctx),
            true,
        ),
    ])]
}

fn worktree_items(ctx: &BranchMenuContext) -> Vec<MenuGroup<BranchAction>> {
    let open = ctx
        .worktree_path
        .as_ref()
        .map(|path| {
            item(
                BranchAction::OpenWorktreeDir,
                format!("{}: {}", Msg::MenuOpenWorktreeDir.t(), path.display()),
                ItemState::Enabled,
                false,
            )
        })
        .into_iter()
        .collect();
    vec![
        group(open),
        group(vec![
            item(
                BranchAction::OpenWorktreeFromBranch,
                "Open worktree from branch…",
                open_worktree_state(ctx),
                false,
            ),
            item(
                BranchAction::CreateWorktreeFromHere,
                "Create worktree here…",
                create_worktree_state(ctx),
                false,
            ),
        ]),
    ]
}

fn advanced_items(ctx: &BranchMenuContext) -> Vec<MenuGroup<BranchAction>> {
    let sync = if ctx.is_current {
        Vec::new()
    } else {
        vec![
            item(BranchAction::Pull, pull_label(ctx), pull_state(ctx), false),
            item(BranchAction::Push, push_label(ctx), push_state(ctx), false),
        ]
    };
    vec![
        group(inspect_items(ctx)),
        group(vec![
            item(
                BranchAction::CreateTagHere,
                "Create tag here…",
                create_tag_state(ctx),
                false,
            ),
            item(
                BranchAction::CreatePr,
                "Create PR",
                create_pr_state(ctx),
                false,
            ),
        ]),
        group(sync),
        group(sync_options(ctx)),
        group(danger_items(ctx)),
    ]
}

fn inspect_items(ctx: &BranchMenuContext) -> Vec<MenuItem<BranchAction>> {
    vec![
        item(
            BranchAction::RevealHead,
            "Reveal branch HEAD in graph",
            ItemState::Enabled,
            false,
        ),
        item(
            BranchAction::ToggleSolo,
            if ctx.is_soloed { "Exit Solo" } else { "Solo" },
            ItemState::Enabled,
            false,
        ),
        item(
            BranchAction::CopyBranchName,
            "Copy branch name",
            ItemState::Enabled,
            false,
        ),
        item(
            BranchAction::CopyHeadSha,
            copy_head_sha_label(ctx),
            ItemState::Enabled,
            false,
        ),
        item(
            BranchAction::CopyUpstreamName,
            "Copy upstream name",
            copy_upstream_state(ctx),
            false,
        ),
    ]
}

fn sync_options(ctx: &BranchMenuContext) -> Vec<MenuItem<BranchAction>> {
    let mut items = vec![
        item(
            BranchAction::NoUpstreamInfo,
            Msg::NoUpstreamSet.t(),
            no_upstream_info_state(ctx),
            false,
        ),
        item(
            BranchAction::PullFfOnly,
            "Pull ff-only",
            pull_state(ctx),
            false,
        ),
        item(
            BranchAction::PushAndCreateUpstream,
            "Push and create upstream",
            push_create_upstream_state(ctx),
            false,
        ),
        item(
            BranchAction::SetUpstream,
            set_upstream_label(ctx),
            set_upstream_state(ctx),
            false,
        ),
        item(
            BranchAction::SwitchToLatest,
            switch_to_latest_label(ctx),
            switch_to_latest_state(ctx),
            false,
        ),
    ];
    if ctx.is_current {
        items.push(item(
            BranchAction::FetchRemoteBranch,
            "Fetch remote branch",
            fetch_remote_branch_state(ctx),
            false,
        ));
    }
    items
}

fn danger_items(ctx: &BranchMenuContext) -> Vec<MenuItem<BranchAction>> {
    vec![
        item(
            BranchAction::SyncToRemote,
            "Sync to remote (keep local)…",
            sync_to_remote_state(ctx),
            true,
        ),
        item(
            BranchAction::ResetCurrentToHead,
            if ctx.protected {
                "Reset current to this HEAD… (protected)"
            } else {
                "Reset current to this HEAD…"
            },
            reset_current_state(ctx),
            true,
        ),
        item(
            BranchAction::ForceWithLeasePush,
            "Force-with-lease push…",
            force_lease_push_state(ctx),
            true,
        ),
        item(
            BranchAction::DeleteRemoteBranch,
            "Delete remote branch…",
            delete_remote_state(ctx),
            true,
        ),
    ]
}

pub fn header(ctx: &BranchMenuContext) -> SharedString {
    if ctx.is_current {
        SharedString::from(format!("{} ✓", ctx.name))
    } else {
        SharedString::from(ctx.name.clone())
    }
}

pub fn is_protected_branch(name: &str) -> bool {
    name == "main"
        || name == "master"
        || name == "develop"
        || name.chars().take(8).collect::<String>() == "release/"
}

pub fn render_branch_menu_overlay(
    state: BranchMenuState,
    header: SharedString,
    groups: Vec<MenuGroup<BranchAction>>,
    keys: &super::menu_keys::MenuKeys,
    window_active: bool,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let position = state.position;
    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.branch_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: BranchAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.branch_menu = None;
        this.dispatch_branch_action(action, state.clone(), window, cx);
    };
    menu_overlay::render_menu_overlay(
        "branch-context-menu",
        "branch-menu-item",
        MENU_W,
        "Advanced / Dangerous",
        position,
        header,
        groups,
        on_dismiss,
        on_select,
        Some(keys),
        window_active,
        window,
        cx,
    )
}

pub fn copy_branch_name(app: &mut KagiApp, name: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(name.clone()));
    app.push_toast(super::ToastKind::Info, i18n::copied_fmt(&name), cx);
}

pub fn copy_head_sha(app: &mut KagiApp, sha: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()));
    app.push_toast(super::ToastKind::Info, i18n::copied_fmt(&sha), cx);
}

pub fn copy_upstream_name(app: &mut KagiApp, upstream: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(upstream.clone()));
    app.push_toast(super::ToastKind::Info, i18n::copied_fmt(&upstream), cx);
}

fn item(
    action: BranchAction,
    label: impl Into<String>,
    state: ItemState,
    dangerous: bool,
) -> MenuItem<BranchAction> {
    MenuItem::command(action, SharedString::from(label.into()), state, dangerous)
}

#[cfg(test)]
#[path = "branch_menu_tests.rs"]
mod tests;
