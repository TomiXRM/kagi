//! Editor Workspace bin glue (ADR-0121 C4): the pane itself —
//! `EditorWorkspaceView`, its tabs/tree/save machinery, and the markdown
//! preview — lives in `crates/kagi-ui-editor`. This module keeps everything
//! that touches `kagi_git::Backend` or `KagiApp` state:
//!
//! - the `KagiApp` entry points (open/close/step/save/dirty-guard, ADR-0117 /
//!   ADR-0120),
//! - the [`EditorWorkspaceEvent`] subscription mapping crate events onto app
//!   state (modals, toasts, footer),
//! - the two `Backend` loaders the crate is seeded from (working-tree file
//!   list → `seed_files`, WIP diff → `seed_diff`),
//! - the [`EditorHooks`] injection (`lang_for_path` / `image_from_bytes` /
//!   the `render_diff_list`-based hunks renderer over the bin's
//!   `MainDiffView`).
//!
//! `pub use` keeps every existing `crate::ui::editor_workspace::…` path
//! working (same diff-zero recipe as C2's Ecosystem extraction).

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, Entity, SharedString};

pub use kagi_ui_editor::*;

use super::i18n::Msg;
use super::render_helpers::render_diff_list;
use super::{
    build_main_diff_view, EditorDirtyGuardModal, EditorPendingIntent, FooterStatus, KagiApp,
    MainDiffSource, MainDiffView, ToastKind,
};

/// The host hooks injected at construction (ADR-0121 C4): the bin-owned
/// helpers the crate renders/loads with but must not own.
fn editor_hooks() -> EditorHooks {
    EditorHooks {
        lang_for_path: super::diff_view::lang_for_path,
        image_from_bytes: super::avatar_fetch::image_from_bytes,
        render_hunks: Box::new(|view, cx| {
            // `view.diff` is seeded below as a boxed `MainDiffView` (a bin
            // type the crate can't name) — downcast and hand it to the
            // shared diff-list renderer, exactly as the pre-C4 in-crate arm
            // did.
            let Some(diff) = view
                .diff
                .as_ref()
                .and_then(|d| d.downcast_ref::<MainDiffView>())
            else {
                return gpui::div().into_any_element();
            };
            render_diff_list::<EditorWorkspaceView>(
                diff.clone(),
                None,
                None,
                view.diff_scroll.clone(),
                cx,
            )
            .into_any_element()
        }),
    }
}

// ── KagiApp entry points (ADR-0117 / ADR-0120) ─────────────────────

impl KagiApp {
    /// Open the Editor workspace for the current repo. No-op when no
    /// repository is open.
    ///
    /// T-WS-EDITOR-005 finding #4: close the File History / Ecosystem
    /// takeovers first. Both beat Editor mode in `resolve_workspace`, so
    /// leaving one open would mean Cmd-Shift-E silently does nothing visible
    /// — the entity would open in the background but the resolver would keep
    /// showing the takeover. The reverse direction (opening Analyze over an
    /// open Editor) is left as-is: that's the existing, intended overlay
    /// behavior (the editor entity keeps running, just hidden).
    pub fn open_editor_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        self.close_file_history();
        self.close_ecosystem_view();
        let view = cx.new(|_| EditorWorkspaceView::new(repo_path, editor_hooks()));
        cx.subscribe(&view, Self::on_editor_workspace_event)
            .detach();
        self.editor_workspace = Some(view.clone());
        klog!("editor-ws: open");
        view.update(cx, |v, cx| v.start_load(cx));
        cx.notify();
    }

    /// Map the crate's outward events onto app state (ADR-0121 C4).
    fn on_editor_workspace_event(
        &mut self,
        view: Entity<EditorWorkspaceView>,
        event: &EditorWorkspaceEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorWorkspaceEvent::CloseRequested => {
                self.close_editor_workspace();
                cx.notify();
            }
            EditorWorkspaceEvent::DirtyGuardRequested(intent) => {
                let intent = match intent {
                    EditorGuardIntent::Reload => EditorPendingIntent::Reload,
                    EditorGuardIntent::CloseTab(path) => {
                        EditorPendingIntent::CloseTab(path.clone())
                    }
                    EditorGuardIntent::Close => EditorPendingIntent::Close,
                };
                self.open_editor_dirty_guard(intent, cx);
            }
            EditorWorkspaceEvent::SaveBlocked => {
                self.push_toast(ToastKind::Error, Msg::EditorWorkspaceSaveBlocked.t(), cx);
                cx.notify();
            }
            EditorWorkspaceEvent::SaveFailed { path, error } => {
                // Non-git file error: surface via toast + footer (the
                // established precedent for a background op outside the
                // plan pipeline — e.g. `repo.fetch`'s failure path,
                // `commands.rs`), not a git plan modal.
                let msg = format!("Save failed: {}: {}", path.display(), error);
                self.push_toast(ToastKind::Error, msg.clone(), cx);
                self.status_footer = FooterStatus::Failed(SharedString::from(msg));
                cx.notify();
            }
            EditorWorkspaceEvent::FilesRequested { generation, source } => {
                self.start_editor_files_load(view, *generation, *source, cx);
            }
            EditorWorkspaceEvent::DiffRequested { req, path } => {
                self.start_editor_diff_load(view, *req, path.clone(), cx);
            }
        }
    }

    /// `Backend` half of the crate's file-list load (`FilesRequested` →
    /// `seed_files`): working-tree status (+ the full worktree listing for
    /// `TreeSource::All`), merged off-thread with the crate's pure merge
    /// helpers. The weak view handle makes a dropped entity a no-op, same as
    /// the pre-C4 in-crate spawn.
    fn start_editor_files_load(
        &mut self,
        view: Entity<EditorWorkspaceView>,
        generation: u64,
        source: TreeSource,
        cx: &mut Context<Self>,
    ) {
        let repo_path = view.read(cx).repo_path.clone();
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&repo_path).map_err(|e| e.to_string())?;
            let status = backend.working_tree_status().map_err(|e| e.to_string())?;
            let changed = merge_working_tree_files(&status);
            match source {
                TreeSource::Changes => Ok(changed),
                TreeSource::All => {
                    let all_paths = backend.worktree_files().map_err(|e| e.to_string())?;
                    Ok(merge_all_files(all_paths, &changed))
                }
            }
        });
        let view = view.downgrade();
        cx.spawn(async move |_app, acx| {
            let result: Result<Vec<WorkspaceFile>, String> = task.await;
            let _ = view.update(acx, |v, cx| v.seed_files(generation, result, cx));
        })
        .detach();
    }

    /// `Backend` half of the crate's WIP-diff load (`DiffRequested` →
    /// `seed_diff`): unstaged diff, falling back to staged (mirrors
    /// `FileHistoryView::load_diff`), built into a `MainDiffView` and handed
    /// over boxed (the crate stores it opaquely; `editor_hooks`'s
    /// `render_hunks` downcasts it back).
    fn start_editor_diff_load(
        &mut self,
        view: Entity<EditorWorkspaceView>,
        req: u64,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let repo_path = view.read(cx).repo_path.clone();
        let bg_path = path.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&repo_path).ok().and_then(|repo| {
                match repo.unstaged_file_diff(&bg_path) {
                    Ok(d) if !d.hunks.is_empty() || d.is_binary => Some(d),
                    _ => repo.staged_file_diff(&bg_path).ok(),
                }
            })
        });
        let view = view.downgrade();
        cx.spawn(async move |_app, acx| {
            let file_diff = task.await;
            let _ = view.update(acx, |v, cx| {
                let diff = file_diff.map(|d| {
                    Box::new(build_main_diff_view(
                        &d,
                        &path,
                        0,
                        MainDiffSource::Unstaged { path: path.clone() },
                    )) as Box<dyn std::any::Any>
                });
                v.seed_diff(req, &path, diff, cx);
            });
        })
        .detach();
    }

    /// Close the Editor workspace (drops the entity; Graph mode is derived
    /// from `editor_workspace.is_none()`).
    pub fn close_editor_workspace(&mut self) {
        self.editor_workspace = None;
    }

    /// True when closing/switching repo context would drop unsaved editor tabs.
    pub fn editor_workspace_any_dirty(&self, cx: &mut Context<Self>) -> bool {
        match self.editor_workspace.as_ref() {
            Some(ev) => ev.read(cx).any_dirty(),
            None => false,
        }
    }

    /// True when deleting `path` would close at least one dirty editor tab.
    pub fn editor_workspace_dirty_under(
        &self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.editor_workspace.as_ref() {
            Some(ev) => ev.read(cx).any_dirty_under(path),
            None => false,
        }
    }

    /// Root ↑/↓ handler branch for the Editor workspace: step the file
    /// selection on the entity (normal parent→child `update`; the entity is
    /// not leased here).
    pub fn step_editor_ws_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(ev) = self.editor_workspace.clone() {
            ev.update(cx, |v, cx| v.step_selection(delta, cx));
        }
    }

    /// Cmd-S root handler (T-WS-EDITOR-002 §3): save the Editor Workspace's
    /// buffer if it's open and dirty; no-op otherwise (closed workspace,
    /// clean buffer, or the "Save" keystroke firing on the wrong tab).
    pub fn save_editor_file(&mut self, cx: &mut Context<Self>) {
        let Some(ev) = self.editor_workspace.clone() else {
            return;
        };
        if !ev.read(cx).dirty {
            return;
        }
        ev.update(cx, |v, cx| v.save(cx));
    }

    /// Open the unsaved-changes confirmation before discarding the Editor
    /// Workspace's dirty buffer (T-WS-EDITOR-002 §5). `ActiveModal` lives on
    /// `KagiApp` per the CLAUDE.md state-update rules; `intent` is what to do
    /// once the user confirms Discard.
    pub fn open_editor_dirty_guard(&mut self, intent: EditorPendingIntent, cx: &mut Context<Self>) {
        self.set_editor_dirty_guard_modal(EditorDirtyGuardModal { intent });
        klog!("editor-ws: dirty-guard");
        cx.notify();
    }

    /// Esc / Cancel on the unsaved-changes modal: dismiss without acting.
    pub fn cancel_editor_dirty_guard(&mut self) {
        self.clear_editor_dirty_guard_modal();
    }

    /// Enter / Discard on the unsaved-changes modal: drop the confirmation
    /// and run the pending action. The entity's `select`/`reload_from_disk`
    /// reset `dirty` as part of navigating/reloading, so no separate
    /// "discard" step is needed here.
    pub fn confirm_editor_dirty_guard(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_dirty_guard_modal().cloned() else {
            return;
        };
        self.clear_editor_dirty_guard_modal();
        match modal.intent {
            EditorPendingIntent::Reload => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.reload_from_disk(cx));
                }
            }
            EditorPendingIntent::CloseTab(path) => {
                if let Some(ev) = self.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.close_tab_now(&path, cx));
                }
            }
            EditorPendingIntent::Close => {
                self.close_editor_workspace();
            }
            EditorPendingIntent::SwitchRepo(path) => {
                self.close_editor_workspace();
                self.switch_repo_by_path(&path, cx);
            }
            EditorPendingIntent::CloseRepoTab(path) => {
                self.close_editor_workspace();
                self.close_tab_by_path(&path, cx);
            }
            EditorPendingIntent::EnterRemoteView { host, root, snap } => {
                self.close_editor_workspace();
                self.enter_remote_view(host, root, (*snap).clone(), cx);
            }
        }
        cx.notify();
    }
}
