//! Unstaged file-row context menu overlay, split out of `render_helpers.rs`
//! (T-SPLIT-HELPERS-001 / ADR-0116 Wave 3).
//! Behaviour-preserving move — no DOM/style/handler/[kagi]/i18n change.

use super::*;
/// File-row intent frozen at the row event source. `path` is a repository-
/// relative file key, not repository identity; `owner` prevents the intent
/// from being re-routed through a different active tab.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMenu {
    pub owner: crate::app::SessionId,
    pub path: std::path::PathBuf,
    pub anchor: gpui::Point<gpui::Pixels>,
}

#[derive(Clone, Copy)]
pub(crate) enum FileMenuAction {
    OpenExternal,
    ShowHistory,
    Discard,
}

impl KagiApp {
    /// Gate every retained menu callback against the exact intent and its
    /// session owner before consulting active presentation state.
    pub(crate) fn dispatch_file_menu_action(
        &mut self,
        expected: &FileMenu,
        action: FileMenuAction,
        cx: &mut Context<Self>,
    ) {
        if self.file_menu.as_ref() != Some(expected) {
            return;
        }
        let panel_has_owner = self
            .ui()
            .commit_panel
            .as_ref()
            .is_some_and(|panel| panel.read(cx).owner == expected.owner);
        let owner_is_active = self.pane_mutation_admitted(expected.owner) && panel_has_owner;
        self.file_menu = None;
        if !owner_is_active {
            cx.notify();
            return;
        }
        match action {
            FileMenuAction::OpenExternal => {
                self.open_in_external_editor(&expected.path, None, cx);
            }
            FileMenuAction::ShowHistory => {
                self.open_file_history(expected.path.clone(), None, cx);
            }
            FileMenuAction::Discard => self.open_discard_modal_for_path(
                expected.owner,
                expected.path.clone(),
                crate::ui::worktree_wip::WriteOrigin::CommitPanel,
                cx,
            ),
        }
        cx.notify();
    }
}
impl KagiApp {
    pub(crate) fn open_file_menu(
        &mut self,
        menu: FileMenu,
        panel_repo: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let panel_is_source = self.ui().commit_panel.as_ref().is_some_and(|entity| {
            let panel = entity.read(cx);
            panel.owner == menu.owner && panel.repo_path == panel_repo
        });
        if self.active_session() == Some(menu.owner) && panel_is_source {
            self.file_menu = Some(menu);
            cx.notify();
        }
    }
}

/// Unscaled per-row height of these compact menus (px_3 / py 3 / text_sm).
const FILE_MENU_ROW_H: f32 = 22.0;

pub(crate) fn render_file_menu_overlay(
    menu: FileMenu,
    viewport: gpui::Size<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let pos = kagi_ui_core::theme::clamp_menu_pos(
        menu.anchor,
        190.0,
        4.0 + 2.0 * FILE_MENU_ROW_H,
        viewport,
    );
    let dismiss_menu = menu.clone();
    let dismiss = cx.listener(move |this, _e: &gpui::MouseDownEvent, _window, cx| {
        if this.file_menu.as_ref() == Some(&dismiss_menu) {
            this.file_menu = None;
            cx.notify();
        }
    });
    // Issue #286: the menu is keyed by path, so every action targets that exact
    // path regardless of any renumber that happened while it was open.
    let discard_menu = menu.clone();
    let discard_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.dispatch_file_menu_action(&discard_menu, FileMenuAction::Discard, cx);
    });
    // ADR-0089: open File History for this unstaged file.
    let history_menu = menu.clone();
    let history_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.dispatch_file_menu_action(&history_menu, FileMenuAction::ShowHistory, cx);
    });
    let external_menu = menu;
    let wip_ext_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.dispatch_file_menu_action(&external_menu, FileMenuAction::OpenExternal, cx);
    });
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(
            div()
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(theme::scaled_px(190.))
                .occlude()
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(theme().surface))
                .rounded_md()
                .shadow_lg()
                // W27-UIPOLISH: compact (Zed-style) density — tighter vertical
                // padding to match the commit/branch context menus.
                .py(theme::scaled_px(2.))
                .child(crate::ui::e2e::measure_control(
                    "file-menu-ext",
                    div()
                        .id("file-menu-ext")
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(wip_ext_click)
                        .child(SharedString::from(Msg::OpenInExternalEditor.t())),
                ))
                .child(crate::ui::e2e::measure_control(
                    "file-menu-history",
                    div()
                        .id("file-menu-history")
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(history_click)
                        .child(SharedString::from("Show File History")),
                ))
                .child(crate::ui::e2e::measure_control(
                    "file-menu-discard",
                    div()
                        .id("file-menu-discard")
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(discard_click)
                        .child(SharedString::from("Discard changes…")),
                )),
        )
        .into_any_element()
}

/// Inspector / Compare changed-file row context menu: History / Open in
/// Editor / Copy Path. Same overlay skeleton as the unstaged-file menu above
/// (backdrop dismiss, `.occlude()` on both layers).
pub(crate) fn render_inspector_file_menu_overlay(
    fi: usize,
    pos: gpui::Point<gpui::Pixels>,
    viewport: gpui::Size<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let pos =
        kagi_ui_core::theme::clamp_menu_pos(pos, 190.0, 4.0 + 3.0 * FILE_MENU_ROW_H, viewport);
    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _window, cx| {
        this.inspector_file_menu = None;
        cx.notify();
    });
    let history_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.inspector_file_menu = None;
        this.open_file_history_inspector_file(fi, cx);
        cx.notify();
    });
    let edit_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.inspector_file_menu = None;
        if let Some((path, _)) = this.inspector_file_ref(fi, cx) {
            // Reuse a running workspace — `open_editor_workspace` rebuilds the
            // entity and would drop open tabs / dirty buffers.
            if this.ui().editor_workspace.is_none() {
                this.open_editor_workspace(cx);
            }
            if let Some(ws) = this.ui().editor_workspace.clone() {
                ws.update(cx, |v, cx| v.open_tab(path, cx));
            }
        }
        cx.notify();
    });
    let ext_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.inspector_file_menu = None;
        if let Some((path, _)) = this.inspector_file_ref(fi, cx) {
            this.open_in_external_editor(&path, None, cx);
        }
        cx.notify();
    });
    let copy_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.inspector_file_menu = None;
        if let Some((path, _)) = this.inspector_file_ref(fi, cx) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                path.to_string_lossy().into_owned(),
            ));
            this.push_toast(
                ToastKind::Info,
                SharedString::from(Msg::EditorTreeCopyPath.t()),
                cx,
            );
        }
        cx.notify();
    });

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(
            div()
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(theme::scaled_px(190.))
                .occlude()
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(theme().surface))
                .rounded_md()
                .shadow_lg()
                .py(theme::scaled_px(2.))
                .child(item(
                    ("insp-menu-history", fi),
                    SharedString::from(Msg::MenuShowFileHistory.t()),
                    history_click,
                ))
                .child(item(
                    ("insp-menu-edit", fi),
                    SharedString::from(Msg::MenuOpenInEditor.t()),
                    edit_click,
                ))
                .child(item(
                    ("insp-menu-ext", fi),
                    SharedString::from(Msg::OpenInExternalEditor.t()),
                    ext_click,
                ))
                .child(item(
                    ("insp-menu-copy", fi),
                    SharedString::from(Msg::EditorTreeCopyPath.t()),
                    copy_click,
                )),
        )
        .into_any_element()
}

/// Compact menu row shared by the hand-rolled overlays here.
fn item<H>(id: (&'static str, usize), label: SharedString, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(id)
        .px_3()
        .py(theme::scaled_px(3.))
        .text_sm()
        .text_color(rgb(theme().text_main))
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .on_click(handler)
        .child(label)
}

/// GitHub Phase 1: sidebar PR row context menu — Open on GitHub / Copy URL.
pub(crate) fn render_pr_menu_overlay(
    pr: kagi_domain::github::PullRequest,
    pos: gpui::Point<gpui::Pixels>,
    viewport: gpui::Size<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let pos =
        kagi_ui_core::theme::clamp_menu_pos(pos, 190.0, 4.0 + 2.0 * FILE_MENU_ROW_H, viewport);
    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _window, cx| {
        this.pr_menu = None;
        cx.notify();
    });
    let pr_open = pr.clone();
    let open_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.pr_menu = None;
        this.open_pr_in_browser(&pr_open);
        cx.notify();
    });
    let pr_copy = pr.clone();
    let copy_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.pr_menu = None;
        this.copy_pr_url(&pr_copy, cx);
        cx.notify();
    });
    let pr_peek = pr.clone();
    let peek_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.pr_menu = None;
        this.open_pr_peek(&pr_peek, cx);
        cx.notify();
    });
    let pr_jump = pr.clone();
    let jump_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.pr_menu = None;
        this.jump_to_pr_head(&pr_jump, cx);
        cx.notify();
    });
    let n = pr.number as usize;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(
            div()
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(theme::scaled_px(190.))
                .occlude()
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(theme().surface))
                .rounded_md()
                .shadow_lg()
                .py(theme::scaled_px(2.))
                .child(item(
                    ("pr-menu-peek", n),
                    SharedString::from(Msg::PrPeek.t()),
                    peek_click,
                ))
                .child(item(
                    ("pr-menu-jump", n),
                    SharedString::from(Msg::PrJumpToBranch.t()),
                    jump_click,
                ))
                .child(item(
                    ("pr-menu-open", n),
                    SharedString::from(Msg::PrOpenOnGitHub.t()),
                    open_click,
                ))
                .child(item(
                    ("pr-menu-copy", n),
                    SharedString::from(Msg::PrCopyUrl.t()),
                    copy_click,
                )),
        )
        .into_any_element()
}
