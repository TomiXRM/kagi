//! Branch context menu model and overlay renderer.

use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, IntoElement, MouseButton, Pixels, Point,
    SharedString, Window,
};
use gpui_component::tooltip::Tooltip;

use kagi::git::CommitId;

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    i18n::Msg,
    theme::theme,
    KagiApp,
};

const MENU_W: f32 = 300.0;
const MENU_MARGIN: f32 = 8.0;
// W27-UIPOLISH: Zed-style compact density (kept in sync with context_menu.rs).
const MENU_ROW_H: f32 = 24.0;
const MENU_HEADER_H: f32 = 30.0;
const MENU_GROUP_H: f32 = 18.0;

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
    OpenWorktreeFromBranch,
    RevealHead,
    Pull,
    PullFfOnly,
    Push,
    PushAndCreateUpstream,
    SetUpstream,
    NoUpstreamInfo,
    FetchRemoteBranch,
    CreatePr,
    MergeIntoCurrent,
    RebaseCurrentOnto,
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
    pub ahead: usize,
    pub behind: usize,
    pub dirty: bool,
    pub conflict_mode: BranchConflictMode,
    pub protected: bool,
    pub checked_out_in_other_worktree: bool,
    pub checked_out_worktree_path: Option<String>,
    pub merged_into_current: bool,
    pub is_pushed: bool,
    pub detached_head: bool,
    pub busy: bool,
    pub current_branch: Option<String>,
}

pub fn branch_context_menu_items(ctx: &BranchMenuContext) -> Vec<MenuGroup<BranchAction>> {
    let mut checkout_label = match ctx.kind {
        BranchKind::Local => format!("Checkout {}", ctx.name),
        BranchKind::Remote => format!("Checkout {} as local branch", ctx.name),
    };
    if ctx.dirty {
        checkout_label.push_str(" (dirty working tree)");
    }
    if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        checkout_label.push_str(" (conflicts)");
    }
    let merge_label = match &ctx.current_branch {
        Some(current) => format!("Merge {} into {}", ctx.name, current),
        None => format!("Merge {} into current branch", ctx.name),
    };
    let rebase_label = match &ctx.current_branch {
        Some(current) => format!("Rebase {} onto {}", current, ctx.name),
        None => format!("Rebase current branch onto {}", ctx.name),
    };

    vec![
        MenuGroup {
            title: Some("Checkout / Open"),
            items: vec![
                item(
                    BranchAction::Checkout,
                    checkout_label,
                    checkout_state(ctx),
                    false,
                ),
                item(
                    BranchAction::OpenWorktreeFromBranch,
                    "Open worktree from branch...",
                    open_worktree_state(ctx),
                    false,
                ),
                item(
                    BranchAction::RevealHead,
                    "Reveal branch HEAD in graph",
                    ItemState::Enabled,
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Sync"),
            items: vec![
                item(
                    BranchAction::NoUpstreamInfo,
                    "No upstream set",
                    no_upstream_info_state(ctx),
                    false,
                ),
                item(BranchAction::Pull, pull_label(ctx), pull_state(ctx), false),
                item(
                    BranchAction::PullFfOnly,
                    "Pull ff-only",
                    mutating_stub_state(ctx),
                    false,
                ),
                item(BranchAction::Push, push_label(ctx), push_state(ctx), false),
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
                    BranchAction::FetchRemoteBranch,
                    "Fetch remote branch",
                    remote_stub_state(ctx),
                    false,
                ),
                item(
                    BranchAction::CreatePr,
                    "Create PR",
                    remote_stub_state(ctx),
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Integrate"),
            items: vec![
                item(
                    BranchAction::MergeIntoCurrent,
                    merge_label,
                    merge_state(ctx),
                    false,
                ),
                item(
                    BranchAction::RebaseCurrentOnto,
                    rebase_label,
                    rebase_state(ctx),
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Create"),
            items: vec![
                item(
                    BranchAction::CreateBranchFromHere,
                    "Create branch from here...",
                    create_branch_state(ctx),
                    false,
                ),
                item(
                    BranchAction::CreateWorktreeFromHere,
                    "Create worktree from here...",
                    create_worktree_state(ctx),
                    false,
                ),
                item(
                    BranchAction::CreateTagHere,
                    "Create tag here...",
                    mutating_stub_state(ctx),
                    false,
                ),
            ],
        },
        MenuGroup {
            title: Some("Manage"),
            items: vec![
                item(
                    BranchAction::RenameBranch,
                    "Rename branch...",
                    rename_state(ctx),
                    false,
                ),
                item(
                    BranchAction::DeleteBranch,
                    delete_label(ctx),
                    delete_state(ctx),
                    true,
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
            ],
        },
        MenuGroup {
            title: Some("Advanced / Dangerous"),
            items: vec![
                item(
                    BranchAction::ResetCurrentToHead,
                    if ctx.protected {
                        "Reset current to this HEAD... (protected)"
                    } else {
                        "Reset current to this HEAD..."
                    },
                    mutating_stub_state(ctx),
                    true,
                ),
                item(
                    BranchAction::ForceWithLeasePush,
                    "Force-with-lease push...",
                    mutating_stub_state(ctx),
                    true,
                ),
                item(
                    BranchAction::DeleteRemoteBranch,
                    "Delete remote branch...",
                    delete_remote_state(ctx),
                    true,
                ),
            ],
        },
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
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let viewport = window.viewport_size();
    let visible_items = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .filter(|item| item.state != ItemState::Hidden)
        .count() as f32;
    let visible_groups = groups
        .iter()
        .filter(|group| {
            group
                .items
                .iter()
                .any(|item| item.state != ItemState::Hidden)
        })
        .count() as f32;
    let menu_h = MENU_HEADER_H + visible_items * MENU_ROW_H + visible_groups * MENU_GROUP_H + 16.0;
    let viewport_w = f32::from(viewport.width);
    let viewport_h = f32::from(viewport.height);
    let raw_x = f32::from(state.position.x);
    let raw_y = f32::from(state.position.y);
    let x = if raw_x + MENU_W + MENU_MARGIN > viewport_w {
        (viewport_w - MENU_W - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_x.max(MENU_MARGIN)
    };
    let y = if raw_y + menu_h + MENU_MARGIN > viewport_h {
        (viewport_h - menu_h - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_y.max(MENU_MARGIN)
    };

    let dismiss_left = cx.listener(
        |this: &mut KagiApp, _event: &gpui::MouseDownEvent, _window, cx| {
            this.branch_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );
    let dismiss_right = cx.listener(
        |this: &mut KagiApp, _event: &gpui::MouseDownEvent, _window, cx| {
            this.branch_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );

    let mut menu = div()
        .id("branch-context-menu")
        .occlude()
        .absolute()
        .top(px(y))
        .left(px(x))
        .w(px(MENU_W))
        .max_h(px((viewport_h - MENU_MARGIN * 2.0).max(120.0)))
        .overflow_hidden()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().modal))
        .shadow_md()
        .child(
            div()
                .h(px(MENU_HEADER_H))
                .px_3()
                .flex()
                .flex_row()
                .items_center()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(header),
        );

    for (group_ix, group) in groups.into_iter().enumerate() {
        if !group
            .items
            .iter()
            .any(|item| item.state != ItemState::Hidden)
        {
            continue;
        }
        if let Some(title) = group.title {
            let title_color = if title == "Advanced / Dangerous" {
                theme().color_warning
            } else {
                theme().text_muted
            };
            menu = menu.child(
                div()
                    .h(px(MENU_GROUP_H))
                    .px_3()
                    .pt_1()
                    .text_xs()
                    .text_color(rgb(title_color))
                    .child(SharedString::from(title)),
            );
        }
        for (item_ix, item) in group.items.into_iter().enumerate() {
            if item.state == ItemState::Hidden {
                continue;
            }
            menu = menu.child(render_menu_item(group_ix, item_ix, state.clone(), item, cx));
        }
    }

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .occlude()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.01)
                .on_mouse_down(MouseButton::Left, dismiss_left)
                .on_mouse_down(MouseButton::Right, dismiss_right),
        )
        .child(menu)
        .into_any_element()
}

pub fn copy_branch_name(app: &mut KagiApp, name: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(name.clone()));
    app.push_toast(super::ToastKind::Info, format!("Copied {}", name));
}

pub fn copy_head_sha(app: &mut KagiApp, sha: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()));
    app.push_toast(super::ToastKind::Info, format!("Copied {}", sha));
}

pub fn copy_upstream_name(app: &mut KagiApp, upstream: String, cx: &mut Context<KagiApp>) {
    cx.write_to_clipboard(ClipboardItem::new_string(upstream.clone()));
    app.push_toast(super::ToastKind::Info, format!("Copied {}", upstream));
}

fn render_menu_item(
    group_ix: usize,
    item_ix: usize,
    state: BranchMenuState,
    item: MenuItem<BranchAction>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let enabled = item.state == ItemState::Enabled;
    let action = item.action.clone();
    let label_color = match (&item.state, item.dangerous) {
        (ItemState::Enabled, true) => theme().color_blocker,
        (ItemState::Enabled, false) => theme().text_main,
        (ItemState::Disabled(_), true) => theme().color_blocker_muted,
        (ItemState::Disabled(_), false) => theme().text_muted,
        (ItemState::Hidden, _) => theme().text_muted,
    };
    let text = if item.dangerous {
        SharedString::from(format!("⚠ {}", item.label.as_ref()))
    } else {
        item.label.clone()
    };

    let click = cx.listener(
        move |this: &mut KagiApp, _event: &gpui::ClickEvent, window, cx| {
            this.branch_menu = None;
            this.dispatch_branch_action(action.clone(), state.clone(), window, cx);
            cx.notify();
        },
    );

    let row = div()
        .id(SharedString::from(format!(
            "branch-menu-item-{}-{}",
            group_ix, item_ix
        )))
        .h(px(MENU_ROW_H))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_sm()
        .text_color(rgb(label_color))
        .overflow_hidden()
        .child(div().flex_1().truncate().child(text));

    let row = if enabled {
        row.on_click(click)
            .hover(|style| style.bg(rgb(theme().selected)).cursor_pointer())
    } else {
        row.hover(|style| style.bg(rgb(theme().surface)))
    };

    match item.state {
        ItemState::Disabled(reason) => row
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            .into_any_element(),
        _ => row.into_any_element(),
    }
}

fn item(
    action: BranchAction,
    label: impl Into<String>,
    state: ItemState,
    dangerous: bool,
) -> MenuItem<BranchAction> {
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

fn mutating_stub_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else {
        disabled(Msg::BcmNotImplementedYet.t())
    }
}

fn remote_stub_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        disabled(Msg::BcmNotImplementedYet.t())
    }
}

fn checkout_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
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

fn open_worktree_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Remote) {
        disabled(Msg::BcmNotImplementedYet.t())
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

fn create_branch_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

fn create_worktree_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        ItemState::Enabled
    }
}

fn merge_state(ctx: &BranchMenuContext) -> ItemState {
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

fn rebase_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.conflict_mode, BranchConflictMode::Conflicted) {
        disabled(Msg::BcmConflictMode.t())
    } else {
        disabled(Msg::BcmNotImplementedYet.t())
    }
}

fn rename_state(ctx: &BranchMenuContext) -> ItemState {
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

fn delete_state(ctx: &BranchMenuContext) -> ItemState {
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

fn delete_remote_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Local) {
        ItemState::Hidden
    } else if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else {
        disabled(Msg::BcmNotImplementedYet.t())
    }
}

fn copy_upstream_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.has_upstream {
        ItemState::Enabled
    } else {
        disabled(Msg::BcmNoUpstream.t())
    }
}

fn pull_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.kind, BranchKind::Remote) {
        disabled(Msg::BcmNotImplementedYet.t())
    } else if !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else if ctx.behind == 0 {
        disabled(Msg::BcmNothingToPull.t())
    } else {
        ItemState::Enabled
    }
}

fn push_state(ctx: &BranchMenuContext) -> ItemState {
    if ctx.busy {
        disabled(Msg::BcmBusy.t())
    } else if ctx.detached_head {
        disabled(Msg::BcmDetachedHead.t())
    } else if matches!(ctx.kind, BranchKind::Remote) {
        ItemState::Hidden
    } else if !ctx.has_upstream {
        ItemState::Hidden
    } else if ctx.ahead == 0 {
        disabled(Msg::BcmNothingToPush.t())
    } else {
        ItemState::Enabled
    }
}

fn push_create_upstream_state(ctx: &BranchMenuContext) -> ItemState {
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

fn set_upstream_state(ctx: &BranchMenuContext) -> ItemState {
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

fn no_upstream_info_state(ctx: &BranchMenuContext) -> ItemState {
    if matches!(ctx.kind, BranchKind::Local) && !ctx.has_upstream {
        disabled(Msg::BcmNoUpstream.t())
    } else {
        ItemState::Hidden
    }
}

fn pull_label(ctx: &BranchMenuContext) -> String {
    if ctx.has_upstream && ctx.behind > 0 {
        format!("Pull ↓{}", ctx.behind)
    } else if ctx.has_upstream {
        "Pull (up to date)".to_string()
    } else {
        "Pull (no upstream)".to_string()
    }
}

fn push_label(ctx: &BranchMenuContext) -> String {
    if ctx.has_upstream && ctx.ahead > 0 {
        format!("Push ↑{}", ctx.ahead)
    } else if ctx.has_upstream {
        "Push (up to date)".to_string()
    } else if !ctx.is_pushed {
        "Push and create upstream".to_string()
    } else {
        "Push".to_string()
    }
}

fn delete_label(ctx: &BranchMenuContext) -> &'static str {
    if ctx.merged_into_current {
        "Delete branch..."
    } else {
        "Delete branch... (unmerged)"
    }
}

fn copy_head_sha_label(ctx: &BranchMenuContext) -> String {
    let short: String = ctx.head_sha.chars().take(8).collect();
    if short.is_empty() {
        "Copy branch HEAD SHA".to_string()
    } else {
        format!("Copy branch HEAD SHA ({})", short)
    }
}

fn set_upstream_label(ctx: &BranchMenuContext) -> String {
    match ctx.upstream_name.as_ref() {
        Some(upstream) => format!("Set upstream ({})", upstream),
        None => "Set upstream".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BranchMenuContext {
        BranchMenuContext {
            name: "feature/x".to_string(),
            head_sha: "1234567890abcdef".to_string(),
            kind: BranchKind::Local,
            is_current: false,
            has_upstream: true,
            upstream_name: Some("origin/feature/x".to_string()),
            ahead: 2,
            behind: 3,
            dirty: false,
            conflict_mode: BranchConflictMode::None,
            protected: false,
            checked_out_in_other_worktree: false,
            checked_out_worktree_path: None,
            merged_into_current: true,
            is_pushed: true,
            detached_head: false,
            busy: false,
            current_branch: Some("main".to_string()),
        }
    }

    fn item_for(
        groups: &[MenuGroup<BranchAction>],
        action: BranchAction,
    ) -> &MenuItem<BranchAction> {
        groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|item| item.action == action)
            .expect("action must exist")
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

    #[test]
    fn local_non_current_with_upstream() {
        let groups = branch_context_menu_items(&ctx());

        assert_enabled(&groups, BranchAction::Checkout);
        assert_enabled(&groups, BranchAction::Pull);
        assert_enabled(&groups, BranchAction::Push);
        assert_enabled(&groups, BranchAction::SetUpstream);
        assert_enabled(&groups, BranchAction::RenameBranch);
        assert_enabled(&groups, BranchAction::RevealHead);
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
        c.ahead = 0;
        c.behind = 0;
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
        assert_disabled_contains(&groups, BranchAction::DeleteRemoteBranch, "not implemented");
    }

    #[test]
    fn busy_disables_mutating_items() {
        let mut c = ctx();
        c.busy = true;
        let groups = branch_context_menu_items(&c);

        assert_disabled_contains(&groups, BranchAction::Checkout, "operation");
        assert_disabled_contains(&groups, BranchAction::CreateBranchFromHere, "operation");
        assert_disabled_contains(&groups, BranchAction::Pull, "operation");
        assert_disabled_contains(&groups, BranchAction::Push, "operation");
        assert_disabled_contains(&groups, BranchAction::SetUpstream, "operation");
        assert_disabled_contains(&groups, BranchAction::RenameBranch, "operation");
        assert_disabled_contains(&groups, BranchAction::DeleteBranch, "operation");
        assert_enabled(&groups, BranchAction::RevealHead);
        assert_enabled(&groups, BranchAction::CopyBranchName);
        assert_enabled(&groups, BranchAction::CopyHeadSha);
    }

    #[test]
    fn upstream_zero_counts_are_noop_disabled() {
        let mut c = ctx();
        c.ahead = 0;
        c.behind = 0;
        let groups = branch_context_menu_items(&c);

        assert_eq!(
            item_for(&groups, BranchAction::Pull).label.as_ref(),
            "Pull (up to date)"
        );
        assert_eq!(
            item_for(&groups, BranchAction::Push).label.as_ref(),
            "Push (up to date)"
        );
        assert_disabled_contains(&groups, BranchAction::Pull, "nothing");
        assert_disabled_contains(&groups, BranchAction::Push, "nothing");
    }

    #[test]
    fn remote_branch_sync_and_manage_availability() {
        let mut c = ctx();
        c.name = "origin/feature/x".to_string();
        c.kind = BranchKind::Remote;
        c.has_upstream = false;
        c.upstream_name = None;
        c.ahead = 0;
        c.behind = 0;
        let groups = branch_context_menu_items(&c);

        assert_hidden(&groups, BranchAction::Push);
        assert_hidden(&groups, BranchAction::PushAndCreateUpstream);
        assert_hidden(&groups, BranchAction::SetUpstream);
        assert_hidden(&groups, BranchAction::RenameBranch);
        assert_hidden(&groups, BranchAction::DeleteBranch);
        assert_disabled_contains(&groups, BranchAction::DeleteRemoteBranch, "not implemented");
    }

    #[test]
    fn merge_and_rebase_labels_include_direction() {
        let groups = branch_context_menu_items(&ctx());

        assert_eq!(
            item_for(&groups, BranchAction::MergeIntoCurrent).label.as_ref(),
            "Merge feature/x into main"
        );
        assert_eq!(
            item_for(&groups, BranchAction::RebaseCurrentOnto).label.as_ref(),
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
    }
}
