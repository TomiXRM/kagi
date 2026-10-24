//! Conflict file utility menu.
use super::*;
/// Actions for the per-file "…" overflow menu.
#[derive(Clone)]
enum ConflictFileAction {
    OpenExternally,
    CopyPath,
    CopyGitCommand,
    OpenTerminal,
}

/// The per-file "…" overflow menu (T-CONFLICT-DASH-022): the full set of
/// file/repo actions (the card icons are quick shortcuts for the top two).
/// Rendered with the shared `menu_overlay` machinery so it gets the same
/// viewport clamping (stays on-screen near the right edge) and styling as the
/// commit / branch / stash context menus, anchored at the click position.
///
/// ADR-0118: rendered as a TOP-LEVEL overlay on the `KagiApp` context (a sibling
/// of the body in `render.rs`, never inside the `ConflictView` entity render), so
/// its `on_select` may call the parent's conflict actions directly without
/// leasing the entity. The owning `file_menu` state lives on the entity, so the
/// dismiss/select close over the entity handle to clear it via `entity.update`.
#[allow(clippy::too_many_arguments)]
pub fn render_file_menu(
    entity: &Entity<ConflictView>,
    mode: &ConflictMode,
    idx: usize,
    pos: gpui::Point<gpui::Pixels>,
    window_active: bool,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Header = the file the menu acts on.
    let header = mode
        .session
        .files
        .get(idx)
        .map(|f| SharedString::from(f.path.to_string_lossy().into_owned()))
        .unwrap_or_default();

    let item = |action: ConflictFileAction, label: &str| {
        MenuItem::command(
            action,
            SharedString::from(label.to_string()),
            ItemState::Enabled,
            false,
        )
    };
    let groups = vec![
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ConflictFileAction::OpenExternally,
                    Msg::ConflictExternalTool.t(),
                ),
                item(ConflictFileAction::CopyPath, Msg::ConflictCopyPath.t()),
            ],
        },
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ConflictFileAction::CopyGitCommand,
                    Msg::ConflictCopyGitCommand.t(),
                ),
                item(
                    ConflictFileAction::OpenTerminal,
                    Msg::ConflictOpenTerminal.t(),
                ),
            ],
        },
    ];

    let dismiss_entity = entity.clone();
    let on_dismiss = move |_this: &mut KagiApp, _w: &mut Window, cx: &mut Context<KagiApp>| {
        dismiss_entity.update(cx, |v, _| v.file_menu = None);
    };
    let select_entity = entity.clone();
    let on_select = move |this: &mut KagiApp,
                          action: ConflictFileAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        // This runs on the `KagiApp` context (top-level overlay), so the entity
        // is NOT leased — clearing `file_menu` via `update` and calling the parent
        // conflict actions directly is safe here.
        select_entity.update(cx, |v, _| v.file_menu = None);
        match action {
            ConflictFileAction::OpenExternally => this.conflict_open_external_tool(idx, cx),
            ConflictFileAction::CopyPath => this.conflict_copy_path(idx, cx),
            ConflictFileAction::CopyGitCommand => this.conflict_copy_git_command(cx),
            ConflictFileAction::OpenTerminal => this.conflict_open_terminal(window, cx),
        }
    };

    menu_overlay::render_menu_overlay(
        "conflict-file-context-menu",
        "conflict-file-menu-item",
        220.0,
        "Danger",
        pos,
        header,
        groups,
        on_dismiss,
        on_select,
        None,
        window_active,
        window,
        cx,
    )
}
