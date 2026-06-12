//! Commit context menu model and overlay renderer.

use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, IntoElement, MouseButton, Pixels, Point,
    SharedString, Window,
};
use gpui_component::tooltip::Tooltip;

use kagi::git::CommitId;

use super::{
    commit_list::{BadgeKind, RefBadge},
    FooterStatus, KagiApp,
};

const BG_SURFACE: u32 = 0x313244;
const BG_SELECTED: u32 = 0x45475a;
const BG_MODAL: u32 = 0x313244;
const BG_MODAL_OVERLAY: u32 = 0x000000;
const TEXT_MAIN: u32 = 0xcdd6f4;
const TEXT_MUTED: u32 = 0x585b70;
const COLOR_BLOCKER: u32 = 0xf38ba8;
const COLOR_WARNING: u32 = 0xf9e2af;

const MENU_W: f32 = 280.0;
const MENU_MARGIN: f32 = 8.0;
const MENU_ROW_H: f32 = 28.0;
const MENU_HEADER_H: f32 = 36.0;
const MENU_GROUP_H: f32 = 22.0;

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
pub struct MenuItem {
    pub action: CommitAction,
    pub label: SharedString,
    pub state: ItemState,
    pub dangerous: bool,
}

#[derive(Clone, Debug)]
pub struct MenuGroup {
    pub title: Option<&'static str>,
    pub items: Vec<MenuItem>,
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
            items: vec![
                item(
                    CommitAction::CheckoutCommit,
                    checkout_commit_label(ctx),
                    checkout_commit_state(ctx),
                    false,
                ),
                checkout_ref_item,
            ],
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
            this.commit_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );
    let dismiss_right = cx.listener(
        |this: &mut KagiApp, _event: &gpui::MouseDownEvent, _window, cx| {
            this.commit_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );

    let mut menu = div()
        .id("commit-context-menu")
        // Block mouse events from reaching the dismiss backdrop below —
        // without this, pressing a menu item fires the backdrop's
        // on_mouse_down first, the menu unmounts, and the item's on_click
        // (down+up on the same element) never completes (user-reported bug).
        .occlude()
        .absolute()
        .top(px(y))
        .left(px(x))
        .w(px(MENU_W))
        .max_h(px((viewport_h - MENU_MARGIN * 2.0).max(120.0)))
        .overflow_hidden()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(BG_SELECTED))
        .bg(rgb(BG_MODAL))
        .shadow_md()
        .child(
            div()
                .h(px(MENU_HEADER_H))
                .px_3()
                .flex()
                .flex_row()
                .items_center()
                .border_b_1()
                .border_color(rgb(BG_SELECTED))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
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
                COLOR_WARNING
            } else {
                TEXT_MUTED
            };
            menu = menu.child(
                div()
                    .h(px(MENU_GROUP_H))
                    .px_3()
                    .pt_2()
                    .text_xs()
                    .text_color(rgb(title_color))
                    .child(SharedString::from(title)),
            );
        }
        for (item_ix, item) in group.items.into_iter().enumerate() {
            if item.state == ItemState::Hidden {
                continue;
            }
            menu = menu.child(render_menu_item(
                group_ix,
                item_ix,
                target.clone(),
                item,
                cx,
            ));
        }
    }

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .bg(rgb(BG_MODAL_OVERLAY))
                .opacity(0.01)
                .on_mouse_down(MouseButton::Left, dismiss_left)
                .on_mouse_down(MouseButton::Right, dismiss_right),
        )
        .child(menu)
        .into_any_element()
}

pub fn short_title_header(full_sha: &str, title: &str) -> SharedString {
    let short: String = full_sha.chars().take(8).collect();
    let title = truncate_chars(title, 54);
    SharedString::from(format!("{} {}", short, title))
}

pub fn copy_full_sha(app: &mut KagiApp, full_sha: String, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    cx.write_to_clipboard(ClipboardItem::new_string(full_sha));
    eprintln!("[kagi] copy-sha: {}", short);
    app.status_footer = FooterStatus::Idle(SharedString::from("SHA copied"));
}

pub fn copy_short_sha(app: &mut KagiApp, full_sha: &str, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    cx.write_to_clipboard(ClipboardItem::new_string(short.clone()));
    eprintln!("[kagi] copy-short-sha: {}", short);
    app.status_footer = FooterStatus::Idle(SharedString::from("Short SHA copied"));
}

pub fn copy_message(app: &mut KagiApp, full_sha: &str, message: String, cx: &mut Context<KagiApp>) {
    let short: String = full_sha.chars().take(8).collect();
    let len = message.chars().count();
    cx.write_to_clipboard(ClipboardItem::new_string(message));
    eprintln!("[kagi] copy-message: {} len={}", short, len);
    app.status_footer = FooterStatus::Idle(SharedString::from("Commit message copied"));
}

fn render_menu_item(
    group_ix: usize,
    item_ix: usize,
    target: CommitId,
    item: MenuItem,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let enabled = item.state == ItemState::Enabled;
    let action = item.action.clone();
    let label_color = match (&item.state, item.dangerous) {
        (ItemState::Enabled, true) => COLOR_BLOCKER,
        (ItemState::Enabled, false) => TEXT_MAIN,
        (ItemState::Disabled(_), true) => 0x8f5360,
        (ItemState::Disabled(_), false) => TEXT_MUTED,
        (ItemState::Hidden, _) => TEXT_MUTED,
    };
    let text = if item.dangerous {
        SharedString::from(format!("⚠ {}", item.label.as_ref()))
    } else {
        item.label.clone()
    };

    let click = cx.listener(
        move |this: &mut KagiApp, _event: &gpui::ClickEvent, window, cx| {
            this.commit_menu = None;
            this.dispatch_commit_action(action.clone(), target.clone(), window, cx);
            cx.notify();
        },
    );

    let row = div()
        .id(SharedString::from(format!(
            "commit-menu-item-{}-{}",
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
            .hover(|style| style.bg(rgb(BG_SELECTED)).cursor_pointer())
    } else {
        row.hover(|style| style.bg(rgb(BG_SURFACE)))
    };

    match item.state {
        ItemState::Disabled(reason) => row
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            .into_any_element(),
        _ => row.into_any_element(),
    }
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
        disabled("detached HEAD")
    } else if ctx.is_head {
        disabled("HEAD と同一")
    } else if ctx.is_merge {
        disabled("merge commit は MVP 対象外")
    } else if ctx.is_ancestor_of_head {
        disabled("既に現在 branch に含まれています")
    } else {
        ItemState::Enabled
    }
}

fn revert_state(ctx: &MenuContext) -> ItemState {
    if ctx.detached {
        disabled("detached HEAD")
    } else if ctx.is_merge {
        disabled("merge commit は MVP 対象外")
    } else if !ctx.is_ancestor_of_head {
        disabled("現在 branch に含まれない")
    } else {
        ItemState::Enabled
    }
}

fn checkout_commit_state(ctx: &MenuContext) -> ItemState {
    if ctx.is_head {
        disabled("既に HEAD")
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
        disabled("同一")
    } else {
        ItemState::Enabled
    }
}

fn compare_working_tree_state(ctx: &MenuContext) -> ItemState {
    if ctx.has_local_changes {
        ItemState::Enabled
    } else {
        disabled("local changes がありません")
    }
}

fn reset_state(ctx: &MenuContext) -> ItemState {
    if ctx.is_head {
        disabled("不要(HEAD と同一)")
    } else if ctx.detached {
        disabled("現在 branch がありません")
    } else {
        disabled("MVP では reset は未実装")
    }
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
        assert_disabled_contains(&groups, CommitAction::CompareWithHead, "同一");
        assert_enabled(&groups, CommitAction::CompareWithWorkingTree);
        assert_disabled_contains(&groups, CommitAction::ResetToCommit, "HEAD");
    }

    #[test]
    fn requirements_past_commit_row() {
        let mut c = ctx();
        c.is_ancestor_of_head = true;
        let groups = build_commit_menu(&c);

        assert_disabled_contains(&groups, CommitAction::CherryPick, "現在 branch に含まれています");
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
        assert_disabled_contains(&groups, CommitAction::Revert, "現在 branch");
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
}
