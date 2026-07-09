//! Editor Workspace tree right-click context menu (T-WS-EDITOR-007): the
//! per-target action set (File / Dir / Root) + the top-level overlay
//! renderer.
//!
//! Mirrors `conflict_view::render_file_menu`'s pattern exactly (per the
//! ticket): rendered on `KagiApp` — never inside `EditorWorkspaceView::render`
//! — so `on_select` dispatches `KagiApp` methods directly without leasing the
//! entity. `render.rs`'s `editor_tree_menu_overlay` is the sibling of its
//! `conflict_file_menu_overlay`; the owning `tree_menu` state lives on the
//! entity, cleared via `entity.update` from the dismiss/select closures here.

use std::path::PathBuf;

use gpui::{Context, Entity, Pixels, Point, SharedString, Window};

use super::context_menu::{ItemState, MenuGroup, MenuItem};
use super::editor_fs_ops;
use super::editor_workspace::{dir_path_for_tree_index, EditorWorkspaceView, TreeMenuTarget};
use super::file_tree::TreeRow;
use super::i18n::Msg;
use super::menu_overlay;
use super::KagiApp;

/// One action offered by the tree context menu. Carries its resolved target
/// path/flags, captured when the menu is built (render time) — the same
/// latency window as every other right-click menu in this codebase (commit /
/// branch / stash / conflict-file all capture their index similarly).
#[derive(Clone, Debug)]
pub enum EditorTreeAction {
    Rename(PathBuf),
    PreviewMarkdown(PathBuf),
    Delete { path: PathBuf, is_dir: bool },
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    Reveal(PathBuf),
    History(PathBuf),
    Stage(PathBuf),
    Unstage(PathBuf),
    Discard(PathBuf),
    AddGitignore(PathBuf),
    NewFile(PathBuf),
    NewFolder(PathBuf),
}

/// Resolved info about the right-clicked target, read from the entity once
/// at menu-build time.
struct TargetInfo {
    /// Repo-relative path; empty for `Root`.
    path: PathBuf,
    is_dir: bool,
    untracked: bool,
    unstaged: bool,
    staged: bool,
}

fn resolve_target(view: &EditorWorkspaceView, target: TreeMenuTarget) -> Option<TargetInfo> {
    match target {
        TreeMenuTarget::Root => Some(TargetInfo {
            path: PathBuf::new(),
            is_dir: true,
            untracked: false,
            unstaged: false,
            staged: false,
        }),
        TreeMenuTarget::File(fi) => {
            let f = view.files.get(fi)?;
            Some(TargetInfo {
                path: f.path.clone(),
                is_dir: false,
                untracked: f.untracked,
                unstaged: f.unstaged,
                staged: f.staged,
            })
        }
        TreeMenuTarget::Dir(ti) => {
            if !matches!(view.tree.get(ti), Some(TreeRow::Dir { .. })) {
                return None;
            }
            let path = dir_path_for_tree_index(&view.tree, ti)?;
            Some(TargetInfo {
                path,
                is_dir: true,
                untracked: false,
                unstaged: false,
                staged: false,
            })
        }
    }
}

fn item(
    action: EditorTreeAction,
    label: &'static str,
    dangerous: bool,
) -> MenuItem<EditorTreeAction> {
    MenuItem {
        action,
        label: SharedString::from(label),
        state: ItemState::Enabled,
        dangerous,
    }
}

/// Build the header + grouped items for `info`. `is_root` narrows the menu to
/// New File… / New Folder… only (the ticket's "empty area" scope — the repo
/// root itself isn't a row that can be renamed/deleted/revealed).
fn build_editor_tree_menu(
    info: &TargetInfo,
    is_root: bool,
) -> (SharedString, Vec<MenuGroup<EditorTreeAction>>) {
    let path = info.path.clone();
    let header = if is_root {
        SharedString::from("(repo root)")
    } else {
        SharedString::from(
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        )
    };

    let mut groups = Vec::new();

    // New File… / New Folder… — every target (File row uses its parent dir;
    // Dir/Root use the target itself as the base — resolved in
    // `dispatch_editor_tree_action`, which needs the *directory* to create
    // inside, not the clicked file).
    let new_base = if info.is_dir {
        path.clone()
    } else {
        path.parent().map(PathBuf::from).unwrap_or_default()
    };
    groups.push(MenuGroup {
        title: None,
        items: vec![
            item(
                EditorTreeAction::NewFile(new_base.clone()),
                Msg::EditorTreeNewFile.t(),
                false,
            ),
            item(
                EditorTreeAction::NewFolder(new_base),
                Msg::EditorTreeNewFolder.t(),
                false,
            ),
        ],
    });

    if is_root {
        return (header, groups);
    }

    // Markdown preview — .md file rows only, above the generic file items.
    if !info.is_dir && crate::ui::editor_markdown::is_markdown_path(&path) {
        groups.push(MenuGroup {
            title: None,
            items: vec![item(
                EditorTreeAction::PreviewMarkdown(path.clone()),
                Msg::EditorTreePreviewMarkdown.t(),
                false,
            )],
        });
    }

    // Rename / Copy Path / Copy Relative Path / Reveal — File + Dir rows.
    groups.push(MenuGroup {
        title: None,
        items: vec![
            item(
                EditorTreeAction::Rename(path.clone()),
                Msg::EditorTreeRename.t(),
                false,
            ),
            item(
                EditorTreeAction::CopyPath(path.clone()),
                Msg::EditorTreeCopyPath.t(),
                false,
            ),
            item(
                EditorTreeAction::CopyRelativePath(path.clone()),
                Msg::EditorTreeCopyRelativePath.t(),
                false,
            ),
            item(
                EditorTreeAction::Reveal(path.clone()),
                if cfg!(target_os = "macos") {
                    Msg::EditorTreeRevealFinder.t()
                } else {
                    Msg::EditorTreeRevealFile.t()
                },
                false,
            ),
        ],
    });

    // Git-aware items — File rows only.
    if !info.is_dir {
        let mut git_items = Vec::new();
        git_items.push(item(
            EditorTreeAction::History(path.clone()),
            Msg::EditorTreeHistory.t(),
            false,
        ));
        if info.unstaged {
            git_items.push(item(
                EditorTreeAction::Stage(path.clone()),
                Msg::EditorTreeStage.t(),
                false,
            ));
        }
        if info.staged {
            git_items.push(item(
                EditorTreeAction::Unstage(path.clone()),
                Msg::EditorTreeUnstage.t(),
                false,
            ));
        }
        if info.untracked {
            git_items.push(item(
                EditorTreeAction::AddGitignore(path.clone()),
                Msg::EditorTreeAddGitignore.t(),
                false,
            ));
        }
        groups.push(MenuGroup {
            title: None,
            items: git_items,
        });
    }

    // Danger group: Delete (gated to macOS — see `editor_fs_ops::TRASH_SUPPORTED`)
    // + Discard Changes… (tracked, changed files only).
    let mut danger_items = Vec::new();
    if editor_fs_ops::TRASH_SUPPORTED {
        danger_items.push(item(
            EditorTreeAction::Delete {
                path: path.clone(),
                is_dir: info.is_dir,
            },
            Msg::EditorTreeDelete.t(),
            true,
        ));
    }
    if !info.is_dir && !info.untracked && (info.staged || info.unstaged) {
        danger_items.push(item(
            EditorTreeAction::Discard(path),
            Msg::EditorTreeDiscard.t(),
            true,
        ));
    }
    if !danger_items.is_empty() {
        groups.push(MenuGroup {
            title: Some("Danger"),
            items: danger_items,
        });
    }

    (header, groups)
}

/// Render the tree context menu overlay, top-level on `KagiApp` (see module
/// doc). Returns `None` when `target` no longer resolves (e.g. a stale `Dir`
/// index after the tree reloaded mid-click) — the caller just shows nothing
/// that frame rather than a menu for a target that no longer exists.
pub fn render_editor_tree_menu(
    entity: &Entity<EditorWorkspaceView>,
    target: TreeMenuTarget,
    pos: Point<Pixels>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let info = resolve_target(entity.read(cx), target)?;
    let is_root = matches!(target, TreeMenuTarget::Root);
    let (header, groups) = build_editor_tree_menu(&info, is_root);

    let dismiss_entity = entity.clone();
    let on_dismiss = move |_this: &mut KagiApp, _w: &mut Window, cx: &mut Context<KagiApp>| {
        dismiss_entity.update(cx, |v, cx| v.close_tree_menu(cx));
    };
    let select_entity = entity.clone();
    let on_select = move |this: &mut KagiApp,
                          action: EditorTreeAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        // Runs on the `KagiApp` context (top-level overlay) — the entity is
        // NOT leased here, so clearing `tree_menu` and dispatching the fs/git
        // action directly on `this` is safe (mirrors `conflict_view`'s
        // per-file menu).
        select_entity.update(cx, |v, cx| v.close_tree_menu(cx));
        this.dispatch_editor_tree_action(action, window, cx);
    };

    Some(menu_overlay::render_menu_overlay(
        "editor-tree-context-menu",
        "editor-tree-menu-item",
        220.0,
        "Danger",
        pos,
        header,
        groups,
        on_dismiss,
        on_select,
        window,
        cx,
    ))
}
