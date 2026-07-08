//! Editor Workspace tree context-menu fs operations (T-WS-EDITOR-007): the
//! `open_/cancel_/confirm_` triple for the Rename/New File/New Folder prompt
//! and the Delete (Trash) confirm, plus the tree-menu action dispatcher and
//! the plain (non-modal) actions — Copy Path, Reveal in Finder, History,
//! Add to .gitignore.
//!
//! Stage/Unstage-by-path live in `operations/commit.rs` next to the existing
//! by-index staging methods; Discard-by-path lives in `operations/discard.rs`
//! next to `open_discard_modal_for_index` (per CLAUDE.md: keep a feature's
//! `plan_/preflight_/execute_` neighbors together). This module is the
//! dispatcher that calls into both, plus everything that's purely a `std::fs`
//! / clipboard / process op.
//!
//! Rename/New File/New Folder/Delete/Add-to-.gitignore all touch the
//! filesystem, so the existing FS-watcher → `on_worktree_changed` refresh
//! (already wired for the Editor Workspace, T-WS-EDITOR-002 §4) picks up the
//! tree change on its own — no manual `start_load` here. Only the
//! entity-local bookkeeping the watcher can't infer is handled explicitly:
//! remapping the open buffer/tabs on rename, closing tabs on delete, and
//! opening a brand-new file in the editor immediately.

use crate::ui::*;

use super::super::editor_fs_ops;
use super::super::editor_tree_menu::EditorTreeAction;

impl KagiApp {
    // ── Rename / New File / New Folder prompt ───────────────────────

    /// Open the fs-prompt modal. `base` is the Rename target's full
    /// repo-relative path, or the parent directory to create inside for
    /// New File/New Folder (empty `PathBuf` = repo root). `prefill` seeds the
    /// input (the current name for Rename, empty for New File/New Folder).
    pub fn open_editor_fs_prompt(
        &mut self,
        kind: EditorFsPromptKind,
        base: std::path::PathBuf,
        prefill: String,
        cx: &mut Context<Self>,
    ) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        self.set_editor_fs_prompt_modal(EditorFsPromptModal {
            kind,
            base,
            input: prefill,
            input_state: None,
            error: None,
        });
        cx.notify();
    }

    /// Cancel the fs-prompt modal without touching the filesystem.
    pub fn cancel_editor_fs_prompt(&mut self) {
        self.clear_editor_fs_prompt_modal();
    }

    /// Validate + perform the Rename/New File/New Folder, then close the
    /// modal on success (re-shows with an error message otherwise, same shape
    /// as every other plan-less confirm in this codebase).
    pub fn confirm_editor_fs_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_fs_prompt_modal().cloned() else {
            return;
        };
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let name = modal.input.trim().to_string();
        if let Err(e) = editor_fs_ops::validate_fs_name(&name) {
            self.set_editor_fs_prompt_modal(EditorFsPromptModal {
                error: Some(SharedString::from(e)),
                ..modal
            });
            cx.notify();
            return;
        }

        match modal.kind {
            EditorFsPromptKind::Rename => {
                let old_rel = modal.base.clone();
                let new_rel = old_rel
                    .parent()
                    .map(|p| p.join(&name))
                    .unwrap_or_else(|| std::path::PathBuf::from(&name));
                if editor_fs_ops::path_touches_git_dir(&old_rel)
                    || editor_fs_ops::path_touches_git_dir(&new_rel)
                {
                    self.fail_editor_fs_prompt(modal, "Cannot rename inside .git", cx);
                    return;
                }
                let old_full = repo_path.join(&old_rel);
                let new_full = repo_path.join(&new_rel);
                // Reject genuine collisions, but allow a case-only rename
                // (File.txt → file.txt) on a case-insensitive filesystem,
                // where the new name is the same inode as the old.
                if new_full.exists() && !editor_fs_ops::same_file(&old_full, &new_full) {
                    self.fail_editor_fs_prompt(modal, "Already exists", cx);
                    return;
                }
                match std::fs::rename(&old_full, &new_full) {
                    Ok(()) => {
                        klog!(
                            "editor-ws: fs-renamed {} -> {}",
                            old_rel.display(),
                            new_rel.display()
                        );
                        self.clear_editor_fs_prompt_modal();
                        if let Some(ev) = self.editor_workspace.clone() {
                            ev.update(cx, |v, cx| v.remap_renamed_path(&old_rel, &new_rel, cx));
                        }
                    }
                    Err(e) => self.fail_editor_fs_prompt(modal, &format!("Rename failed: {e}"), cx),
                }
            }
            EditorFsPromptKind::NewFile => {
                let rel = modal.base.join(&name);
                if editor_fs_ops::path_touches_git_dir(&rel) {
                    self.fail_editor_fs_prompt(modal, "Cannot create inside .git", cx);
                    return;
                }
                let full = repo_path.join(&rel);
                if full.exists() {
                    self.fail_editor_fs_prompt(modal, "Already exists", cx);
                    return;
                }
                match std::fs::File::create(&full) {
                    Ok(_) => {
                        klog!("editor-ws: fs-created {}", rel.display());
                        self.clear_editor_fs_prompt_modal();
                        if let Some(ev) = self.editor_workspace.clone() {
                            ev.update(cx, |v, cx| v.open_tab(rel.clone(), cx));
                        }
                    }
                    Err(e) => self.fail_editor_fs_prompt(modal, &format!("Create failed: {e}"), cx),
                }
            }
            EditorFsPromptKind::NewDir => {
                let rel = modal.base.join(&name);
                if editor_fs_ops::path_touches_git_dir(&rel) {
                    self.fail_editor_fs_prompt(modal, "Cannot create inside .git", cx);
                    return;
                }
                let full = repo_path.join(&rel);
                if full.exists() {
                    self.fail_editor_fs_prompt(modal, "Already exists", cx);
                    return;
                }
                match std::fs::create_dir(&full) {
                    Ok(()) => {
                        klog!("editor-ws: fs-created {}", rel.display());
                        self.clear_editor_fs_prompt_modal();
                    }
                    Err(e) => self.fail_editor_fs_prompt(modal, &format!("Create failed: {e}"), cx),
                }
            }
        }
        cx.notify();
    }

    /// Re-show the fs-prompt modal with `msg` as its error (validation / fs
    /// failure) — small shared tail for `confirm_editor_fs_prompt`'s branches.
    fn fail_editor_fs_prompt(
        &mut self,
        modal: EditorFsPromptModal,
        msg: &str,
        cx: &mut Context<Self>,
    ) {
        self.set_editor_fs_prompt_modal(EditorFsPromptModal {
            error: Some(SharedString::from(msg.to_string())),
            ..modal
        });
        cx.notify();
    }

    // ── Delete (Trash) confirm ───────────────────────────────────────

    /// Open the delete-confirm modal for `path` (repo-relative). For a
    /// directory, counts its contents (capped — `editor_fs_ops::
    /// count_dir_entries_capped`) so the modal can show "N files".
    pub fn open_editor_delete_confirm(
        &mut self,
        path: std::path::PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let (file_count, truncated) = if is_dir {
            let (n, t) = editor_fs_ops::count_dir_entries_capped(&repo_path.join(&path), 5000);
            (Some(n), t)
        } else {
            (None, false)
        };
        let has_dirty_buffers = self.editor_workspace_dirty_under(&path, cx);
        self.set_editor_delete_confirm_modal(EditorDeleteConfirmModal {
            path,
            is_dir,
            file_count,
            truncated,
            has_dirty_buffers,
            error: None,
        });
        cx.notify();
    }

    /// Cancel the delete-confirm modal without touching the filesystem.
    pub fn cancel_editor_delete_confirm(&mut self) {
        self.clear_editor_delete_confirm_modal();
    }

    /// Move the target to `~/.Trash` (see `editor_fs_ops::trash_path` — same-
    /// volume only, never falls back to a permanent delete) and close any
    /// open tab(s) under it. Single explicit confirm click — a Trash move is
    /// recoverable, unlike Discard's `git checkout --`, so this doesn't need
    /// `DiscardModal`'s two-stage arm (ponytail: match the safety mechanism to
    /// the actual risk instead of copying the heaviest gate everywhere).
    pub fn confirm_editor_delete(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_delete_confirm_modal().cloned() else {
            return;
        };
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if !editor_fs_ops::TRASH_SUPPORTED {
            self.set_editor_delete_confirm_modal(EditorDeleteConfirmModal {
                error: Some(SharedString::from(
                    "Delete is only supported on macOS currently",
                )),
                ..modal
            });
            cx.notify();
            return;
        }
        if editor_fs_ops::path_touches_git_dir(&modal.path) {
            self.set_editor_delete_confirm_modal(EditorDeleteConfirmModal {
                error: Some(SharedString::from("Cannot delete .git")),
                ..modal
            });
            cx.notify();
            return;
        }
        let full = repo_path.join(&modal.path);
        match editor_fs_ops::trash_path(&full) {
            Ok(_trashed) => {
                klog!("editor-ws: fs-trashed {}", modal.path.display());
                self.clear_editor_delete_confirm_modal();
                if let Some(ev) = self.editor_workspace.clone() {
                    let path = modal.path.clone();
                    ev.update(cx, |v, cx| v.close_paths_under(&path, cx));
                }
            }
            Err(e) => {
                self.set_editor_delete_confirm_modal(EditorDeleteConfirmModal {
                    error: Some(SharedString::from(e)),
                    ..modal
                });
            }
        }
        cx.notify();
    }

    // ── Add to .gitignore ────────────────────────────────────────────

    /// Append `path` (repo-relative) as a new `.gitignore` line (untracked
    /// files only — the tree-menu builder already gates the item to those).
    /// The fs watcher picks up both the `.gitignore` write and the file
    /// disappearing from `git status` on its own.
    fn add_editor_gitignore(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let rel = path.to_string_lossy().replace('\\', "/");
        match editor_fs_ops::add_gitignore_entry(&repo_path, &rel) {
            Ok(()) => {
                klog!("editor-ws: gitignore-added {}", rel);
                self.push_toast(ToastKind::Info, format!("Added to .gitignore: {rel}"), cx);
            }
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    format!(".gitignore write failed: {e}"),
                    cx,
                );
            }
        }
    }

    // ── Copy Path / Reveal in Finder ─────────────────────────────────

    fn copy_editor_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let full = repo_path.join(path).to_string_lossy().into_owned();
        cx.write_to_clipboard(ClipboardItem::new_string(full));
        self.status_footer = FooterStatus::Idle(SharedString::from("Path copied"));
    }

    fn copy_editor_relative_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let rel = path.to_string_lossy().replace('\\', "/");
        cx.write_to_clipboard(ClipboardItem::new_string(rel));
        self.status_footer = FooterStatus::Idle(SharedString::from("Relative path copied"));
    }

    /// Reveal + select `path` in the platform file manager (mirrors the
    /// open→xdg-open cfg pattern in `open_release_page`): macOS `open -R`
    /// selects the item; Windows `explorer /select,` selects it; Linux has no
    /// universal select support, so `xdg-open` opens the parent directory.
    fn reveal_editor_path_in_finder(&mut self, path: &std::path::Path) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let full = repo_path.join(path);
        #[cfg(target_os = "macos")]
        let spawn = std::process::Command::new("open")
            .arg("-R")
            .arg(&full)
            .spawn();
        #[cfg(target_os = "windows")]
        let spawn = std::process::Command::new("explorer")
            .arg(format!("/select,{}", full.display()))
            .spawn();
        #[cfg(target_os = "linux")]
        let spawn = {
            // No universal select support; open the containing directory.
            let dir = full.parent().unwrap_or(&full);
            std::process::Command::new("xdg-open").arg(dir).spawn()
        };
        match spawn {
            Ok(_) => {
                klog!("editor-ws: reveal {}", path.display());
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::OpenedInFinder.t()));
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("reveal failed: {e}")));
            }
        }
    }

    // ── Tree menu action dispatch ─────────────────────────────────────

    /// Dispatch one `EditorTreeAction` chosen from the tree context menu.
    /// Runs on the `KagiApp` context (the top-level overlay's `on_select`,
    /// never inside a leased entity), so every branch below may call into
    /// `self`/the editor-workspace entity directly.
    pub fn dispatch_editor_tree_action(
        &mut self,
        action: EditorTreeAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            EditorTreeAction::Rename(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.open_editor_fs_prompt(EditorFsPromptKind::Rename, path, name, cx);
            }
            EditorTreeAction::Delete { path, is_dir } => {
                self.open_editor_delete_confirm(path, is_dir, cx);
            }
            EditorTreeAction::CopyPath(path) => self.copy_editor_path(&path, cx),
            EditorTreeAction::CopyRelativePath(path) => self.copy_editor_relative_path(&path, cx),
            EditorTreeAction::Reveal(path) => self.reveal_editor_path_in_finder(&path),
            EditorTreeAction::History(path) => self.open_file_history(path, None, cx),
            EditorTreeAction::Stage(path) => self.do_stage_file_by_path(path, cx),
            EditorTreeAction::Unstage(path) => self.do_unstage_file_by_path(path, cx),
            EditorTreeAction::Discard(path) => self.open_discard_modal_for_path(path, cx),
            EditorTreeAction::AddGitignore(path) => self.add_editor_gitignore(&path, cx),
            EditorTreeAction::NewFile(base) => {
                self.open_editor_fs_prompt(EditorFsPromptKind::NewFile, base, String::new(), cx)
            }
            EditorTreeAction::NewFolder(base) => {
                self.open_editor_fs_prompt(EditorFsPromptKind::NewDir, base, String::new(), cx)
            }
        }
    }
}
