//! Main-window native metadata and the shared stateful menu presentation.
use super::{
    commands,
    i18n::{self, Lang, Msg},
    KagiApp,
};
use gpui::{AnyWindowHandle, App, Context, Global, Window};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewMenuState {
    pub sidebar_visible: bool,
    pub fullscreen: bool,
    pub input_focused: bool,
    pub workspace_mode: Option<super::workspace_mode::WorkspaceMode>,
    /// Presentation snapshot derived from the active Editor entity.
    pub editor_details_visible: bool,
    pub find_input: bool,
    pub find_sidebar: bool,
    pub undo_summary: Option<gpui::SharedString>,
    pub redo_summary: Option<gpui::SharedString>,
    pub conflict_skip_armed: bool,
    /// Presentation only: the accepted active-session operation observation.
    pub conflict_operation: Option<kagi_domain::conflict_family::ConflictOperationKind>,
}
impl Default for ViewMenuState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            fullscreen: false,
            input_focused: false,
            workspace_mode: None,
            editor_details_visible: false,
            find_input: false,
            find_sidebar: false,
            undo_summary: None,
            redo_summary: None,
            conflict_skip_armed: false,
            conflict_operation: None,
        }
    }
}
#[derive(Default)]
pub(crate) struct WindowMenus {
    main_windows: Vec<AnyWindowHandle>,
    owner: Option<AnyWindowHandle>,
    last_main: Option<AnyWindowHandle>,
    #[cfg(feature = "gui-e2e")]
    recent_activation_target: Option<AnyWindowHandle>,
    state: ViewMenuState,
    language: Option<Lang>,
}
impl Global for WindowMenus {}

pub(crate) fn register_main_window(window: &Window, cx: &mut App) {
    if !cx.has_global::<WindowMenus>() {
        cx.set_global(WindowMenus::default());
        cx.on_window_closed(|cx, id| {
            let menus = cx.global_mut::<WindowMenus>();
            menus.main_windows.retain(|window| window.window_id() != id);
            if menus.owner.is_some_and(|window| window.window_id() == id) {
                menus.owner = None;
            }
            if menus
                .last_main
                .is_some_and(|window| window.window_id() == id)
            {
                menus.last_main = None;
            }
        })
        .detach();
    }
    cx.global_mut::<WindowMenus>()
        .main_windows
        .push(window.window_handle());
}
pub(crate) fn current_state(cx: &App) -> ViewMenuState {
    cx.try_global::<WindowMenus>()
        .map(|menus| menus.state.clone())
        .unwrap_or_default()
}
/// Utility hosts retain the main window's View labels, but Edit must describe
/// their own focused Input rather than the repository behind them.
pub(super) fn publish_settings_edit_state(window: &mut Window, cx: &mut App) {
    use gpui::Focusable as _;
    use gpui_component::WindowExt as _;
    if !window.is_window_active() || !cx.has_global::<WindowMenus>() {
        return;
    }
    let input_focused = window
        .focused_input(cx)
        .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
    let menus = cx.global_mut::<WindowMenus>();
    if menus.owner == Some(window.window_handle()) && menus.state.input_focused == input_focused {
        return;
    }
    menus.owner = Some(window.window_handle());
    menus.state.input_focused = input_focused;
    menus.state.find_input = false;
    menus.state.find_sidebar = false;
    menus.state.undo_summary = None;
    menus.state.redo_summary = None;
    commands::publish_menus(cx);
}

/// Used by both native menus and the in-app dropdown; IDs and bindings do not vary.
pub fn state_label(id: &str, state: &ViewMenuState) -> Option<&'static str> {
    let msg = match id {
        "repo.conflictAbort" => {
            use kagi_domain::conflict_family::ConflictOperationKind;
            use kagi_domain::plan_note::InProgressOp;
            match state.conflict_operation {
                Some(ConflictOperationKind::Repository(InProgressOp::Merge)) => Msg::MenuAbortMerge,
                Some(ConflictOperationKind::Repository(InProgressOp::Rebase)) => {
                    Msg::MenuAbortRebase
                }
                Some(ConflictOperationKind::Repository(InProgressOp::CherryPick)) => {
                    Msg::MenuAbortCherryPick
                }
                Some(ConflictOperationKind::Repository(InProgressOp::Revert)) => {
                    Msg::MenuAbortRevert
                }
                Some(ConflictOperationKind::StashApply) => Msg::MenuAbortStashApply,
                Some(ConflictOperationKind::Repository(
                    InProgressOp::Bisect | InProgressOp::Other,
                ))
                | None => Msg::MenuAbortOperation,
            }
        }
        "repo.conflictSkip" if state.conflict_skip_armed => Msg::MenuConfirmSkip,
        "view.toggleSidebar" if state.sidebar_visible => Msg::MenuHideSidebar,
        "view.toggleSidebar" => Msg::MenuShowSidebar,
        "view.toggleEditorDetails" if state.editor_details_visible => Msg::EditorDetailsHide,
        "view.toggleEditorDetails" => Msg::EditorDetailsShow,
        "view.fullScreen" if state.fullscreen => Msg::MenuExitFullScreen,
        "view.fullScreen" => Msg::MenuEnterFullScreen,
        "view.showGraph" => Msg::MenuWorkspaceGraph,
        "view.togglePrMode" => Msg::MenuWorkspacePrs,
        "view.toggleEditorWorkspace" => Msg::MenuWorkspaceEditor,
        "window.previousTab" => Msg::MenuPreviousTab,
        "window.nextTab" => Msg::MenuNextTab,
        "window.bringAllToFront" => Msg::MenuBringAllToFront,
        "app.about" => Msg::AboutTitle,
        _ => return None,
    };
    Some(msg.t())
}

pub fn checked(id: &str, state: &ViewMenuState) -> bool {
    if id == "view.toggleEditorDetails" {
        return state.workspace_mode == Some(super::workspace_mode::WorkspaceMode::Editor)
            && state.editor_details_visible;
    }
    use super::workspace_mode::WorkspaceMode;
    let mode = match id {
        "view.showGraph" => WorkspaceMode::Graph,
        "view.togglePrMode" => WorkspaceMode::Prs,
        "view.toggleEditorWorkspace" => WorkspaceMode::Editor,
        _ => return commands::lang_for_command(id).is_some_and(|lang| lang == i18n::lang()),
    };
    state.workspace_mode == Some(mode)
}
impl KagiApp {
    pub(super) fn sync_window_presentation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title =
            if self.home_in_front() || self.repo_path.is_none() && self.remote_view.is_none() {
                gpui::SharedString::from("Kagi")
            } else {
                self.view().window_title.clone()
            };
        // A just-opened tab may not have its first accepted read yet. Publish
        // its known repository identity immediately, never a blank/stale title.
        if title.is_empty() {
            let name = self
                .tabs
                .get(self.active_tab)
                .map(|tab| tab.name.as_str())
                .unwrap_or("Kagi");
            if self.native_window_title.as_ref() != name {
                window.set_window_title(name);
                self.native_window_title = name.to_owned().into();
            }
        } else if self.native_window_title != title {
            window.set_window_title(title.as_ref());
            self.native_window_title = title;
        }
        // Inactive renders retain the last owner, even while the app is inactive.
        // Only initial publication may occur without an active main window.
        if !self.window_active_for_render(window)
            && (cx.active_window().is_some() || cx.global::<WindowMenus>().owner.is_some())
        {
            return;
        }
        self.publish_window_menu_state(window, cx);
    }
    fn menu_input_focused(&self, window: &mut Window, cx: &mut App) -> bool {
        use gpui::Focusable as _;
        use gpui_component::WindowExt as _;
        window.focused_input(cx).is_some_and(|input| {
            let focus = input.read(cx).focus_handle(cx);
            focus.is_focused(window)
                && self
                    .root_focus
                    .as_ref()
                    .is_some_and(|root| root.contains(&focus, window))
        })
    }
    /// Label targets follow editing context, not temporary action availability.
    fn history_menu_target(&self, input: bool, window: &mut Window, cx: &mut App) -> bool {
        let terminal = self
            .ui()
            .terminal_session
            .as_ref()
            .and_then(|session| session.view.as_ref())
            .is_some_and(|view| view.read(cx).focus_handle().is_focused(window));
        let snapshot = self.workspace_mode() == super::workspace_mode::WorkspaceMode::Editor
            && self
                .ui()
                .editor_workspace
                .as_ref()
                .is_some_and(|view| view.read(cx).showing_history_commit());
        !input && !terminal && !snapshot
    }
    pub(super) fn history_menu_available(
        &self,
        redo: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let input = self.menu_input_focused(window, cx);
        !self.home_in_front()
            && self.repo_path.is_some()
            && !self.has_active_modal()
            && !self.op_latched()
            && self.history_menu_target(input, window, cx)
            && if redo {
                self.ui().operation_history.can_redo()
            } else {
                self.ui().operation_history.can_undo()
            }
    }
    fn menu_state(
        &self,
        previous: ViewMenuState,
        fullscreen: bool,
        input_focused: bool,
        history_target: bool,
    ) -> ViewMenuState {
        let summary = |old: &Option<gpui::SharedString>, entry: Option<&kagi_git::HistoryEntry>| {
            entry.map(|entry| {
                old.as_ref()
                    .filter(|old| old.as_ref() == entry.summary)
                    .cloned()
                    .unwrap_or_else(|| entry.summary.clone().into())
            })
        };
        // Home temporarily has no foreground session. Retain the last history
        // labels; its dispatch tree disables repository actions. Input focus
        // still selects that Input's own Undo/Redo without reading a parked tab.
        let history = if self.home_in_front() {
            None
        } else {
            Some(&self.ui().operation_history)
        };
        ViewMenuState {
            sidebar_visible: self.sidebar.visible,
            fullscreen,
            input_focused,
            workspace_mode: (!self.home_in_front()).then(|| self.workspace_mode()),
            editor_details_visible: previous.editor_details_visible,
            conflict_skip_armed: false,
            conflict_operation: (!self.home_in_front())
                .then(|| {
                    self.view()
                        .operation
                        .as_ref()
                        .map(|operation| operation.kind())
                })
                .flatten(),
            find_input: false,
            find_sidebar: false,
            undo_summary: history
                .map(|history| {
                    summary(
                        &previous.undo_summary,
                        history.peek_undo().filter(|_| history_target),
                    )
                })
                .unwrap_or(previous.undo_summary),
            redo_summary: history
                .map(|history| {
                    summary(
                        &previous.redo_summary,
                        history.peek_redo().filter(|_| history_target),
                    )
                })
                .unwrap_or(previous.redo_summary),
        }
    }
    pub(super) fn find_menu_targets(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (bool, bool) {
        use gpui::Focusable as _;
        use gpui_component::WindowExt as _;
        if self.home_in_front() {
            return (false, false);
        }
        if let Some(input) = window
            .focused_input(cx)
            .filter(|input| input.read(cx).focus_handle(cx).is_focused(window))
        {
            // Kit exposes no searchable getter; use the actual Kagi-owned
            // code Inputs, not every text field's unconditional Search handler.
            let editor = self
                .ui()
                .editor_workspace
                .as_ref()
                .is_some_and(|view| view.read(cx).editor.as_ref() == Some(&input));
            let conflict = self.ui().conflict.as_ref().is_some_and(|view| {
                view.read(cx)
                    .editor_inputs
                    .as_ref()
                    .is_some_and(|inputs| inputs.result == input)
            });
            let issue = self
                .ui()
                .issue_composer
                .editors
                .values()
                .any(|editor| editor.body_input.as_ref() == Some(&input));
            return (editor || conflict || issue, false);
        }
        let terminal = self
            .ui()
            .terminal_session
            .as_ref()
            .and_then(|session| session.view.as_ref())
            .is_some_and(|view| view.read(cx).focus_handle().is_focused(window));
        (
            false,
            !terminal
                && !self.conflict_body_visible()
                && self.front_layer(cx) == super::front_layer::FrontLayer::None
                && self.workspace_mode() == super::workspace_mode::WorkspaceMode::Graph,
        )
    }
    pub(super) fn publish_window_menu_state(&self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.menu_input_focused(window, cx);
        let history_target = self.history_menu_target(input, window, cx);
        let mut state = self.menu_state(
            current_state(cx),
            window.is_fullscreen(),
            input,
            history_target,
        );
        state.conflict_skip_armed = !self.home_in_front()
            && self.ui().conflict.as_ref().is_some_and(|view| {
                view.read(cx)
                    .mode
                    .as_ref()
                    .is_some_and(|mode| mode.skip_approval.is_some())
            });
        state.editor_details_visible = state.workspace_mode
            == Some(super::workspace_mode::WorkspaceMode::Editor)
            && self
                .ui()
                .editor_workspace
                .as_ref()
                .is_some_and(|view| view.read(cx).details_visible());
        (state.find_input, state.find_sidebar) = self.find_menu_targets(window, cx);
        let handle = window.window_handle();
        let language = i18n::lang();
        let menus = cx.global_mut::<WindowMenus>();
        menus.last_main = Some(handle);
        if menus.owner == Some(handle) && menus.state == state && menus.language == Some(language) {
            return;
        }
        menus.owner = Some(handle);
        menus.state = state;
        menus.language = Some(language);
        commands::publish_menus(cx);
    }
    pub(super) fn show_adjacent_tab(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.tabs.len() + usize::from(self.home.is_some());
        if count < 2 {
            return;
        }
        let current = if self.home_in_front() {
            self.tabs.len()
        } else {
            self.active_tab
        };
        let target = if next {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        if target == self.tabs.len() {
            self.open_home_tab(window, cx);
        } else {
            let left = self.send_home_back();
            self.return_to_tab(target, left, cx);
        }
    }
}
/// Raise only main windows; utility hosts are never registered here.
pub(crate) fn bring_all_to_front(cx: &mut App) {
    let targets = activation_order(cx);
    if targets.is_empty() {
        return;
    }
    // Defer so the invoking window is no longer on GPUI's update stack.
    cx.defer(move |cx| {
        for target in targets {
            let _ = target.update(cx, |_, window, _| window.activate_window());
        }
    });
}
fn activation_order(cx: &App) -> Vec<AnyWindowHandle> {
    let Some(menus) = cx.try_global::<WindowMenus>() else {
        return Vec::new();
    };
    let stack = cx.window_stack();
    let frontmost = stack
        .as_ref()
        .and_then(|stack| stack.first().copied())
        .or_else(|| cx.active_window());
    let mut targets = stack.unwrap_or_else(|| menus.main_windows.clone());
    targets.retain(|target| menus.main_windows.contains(target) && Some(*target) != frontmost);
    targets.reverse();
    // Preserve the invoking/frontmost window, including a Settings utility host.
    if let Some(frontmost) = frontmost {
        targets.push(frontmost);
    }
    targets
}

#[cfg(feature = "gui-e2e")]
pub fn bring_all_activation_order(cx: &App) -> Vec<AnyWindowHandle> {
    activation_order(cx)
}
#[cfg(feature = "gui-e2e")]
pub fn main_window_targets(cx: &App) -> Vec<AnyWindowHandle> {
    cx.global::<WindowMenus>().main_windows.clone()
}

pub(super) fn preferred_repository_window(cx: &App) -> Option<AnyWindowHandle> {
    let menus = cx.try_global::<WindowMenus>()?;
    cx.active_window()
        .filter(|window| menus.main_windows.contains(window))
        .or(menus.last_main)
        .or_else(|| {
            activation_order(cx)
                .into_iter()
                .rev()
                .find(|window| menus.main_windows.contains(window))
        })
}

pub(super) fn activate_recent_repository(window: &mut Window, cx: &mut App) {
    cx.global_mut::<WindowMenus>().last_main = Some(window.window_handle());
    // Match Settings' hidden-native convention: record the requested target
    // without makeKeyAndOrderFront taking the user's foreground.
    #[cfg(feature = "gui-e2e")]
    if std::env::var_os("KAGI_GUI_E2E").is_some() {
        cx.global_mut::<WindowMenus>().recent_activation_target = Some(window.window_handle());
        window.refresh();
        return;
    }
    window.activate_window();
    cx.activate(true);
}

#[cfg(feature = "gui-e2e")]
pub fn recent_activation_target(cx: &App) -> Option<AnyWindowHandle> {
    cx.global::<WindowMenus>().recent_activation_target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_busy_and_home_do_not_change_history_menu_labels() {
        let mut app = KagiApp::new_common(Default::default());
        app.open_initial_tab(
            std::path::Path::new("/menu-state-test"),
            "menu-state-test",
            false,
            Default::default(),
        );
        app.record_history(
            kagi_git::OperationKind::Commit,
            "main",
            kagi_git::CommitId("before".into()),
            kagi_git::CommitId("after".into()),
            "commit fixture",
        );
        let state = app.menu_state(Default::default(), false, false, true);
        assert_eq!(state.undo_summary.as_deref(), Some("commit fixture"));
        for redo in [false, true] {
            let state = app.menu_state(state.clone(), false, false, true);
            assert_eq!(
                if redo {
                    state.redo_summary.as_deref()
                } else {
                    state.undo_summary.as_deref()
                },
                Some("commit fixture"),
            );
            app.set_app_notice(String::from("menu-state notice").into());
            assert!(app.has_active_modal());
            assert_eq!(app.menu_state(state.clone(), false, false, true), state);
            app.clear_app_notice();
            assert!(!app.has_active_modal());
            assert_eq!(app.menu_state(state.clone(), false, false, true), state);
            let scope = crate::app::WriteScope::Local(kagi_domain::remove::RepoId(
                "/menu-state-test".into(),
            ));
            let operation = crate::app::OperationId(1);
            app.app_sessions
                .reserve_lease(scope.clone(), operation)
                .unwrap();
            assert!(app.op_latched());
            assert_eq!(app.menu_state(state.clone(), false, false, true), state);
            app.app_sessions.release_lease(&scope, operation);
            assert!(!app.op_latched());
            assert_eq!(app.menu_state(state.clone(), false, false, true), state);
            let tab = app.tabs.pop().unwrap();
            assert!(app.home_in_front());
            let home = app.menu_state(state.clone(), false, false, true);
            assert_eq!(home.workspace_mode, None, "Home has no active workspace");
            assert_eq!(home.undo_summary, state.undo_summary);
            assert_eq!(home.redo_summary, state.redo_summary);
            app.tabs.push(tab);
            if !redo {
                app.ui_mut().unwrap().operation_history.undo();
            }
        }
    }
}
