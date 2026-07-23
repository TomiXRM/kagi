//! Commit Panel — T025
//!
//! GitKraken 風の作業台: staging / unstaging / diff / commit message / commit button。
//! `src/git/staging.rs` (T024) の 6 API のみを使う。
//!
//! ## headless 検証 env vars
//! - `KAGI_COMMIT_PANEL=1`       起動時に Commit Panel を開き件数をログ
//! - `KAGI_STAGE_FILE=<path>`    起動時に1ファイル stage
//! - `KAGI_UNSTAGE_FILE=<path>`  起動時に1ファイル unstage
//! - `KAGI_COMMIT_MSG=<msg>`     コミットメッセージ設定 + KAGI_AUTO_CONFIRM=1 で実際にコミット

use std::path::{Path, PathBuf};

use gpui::{prelude::*, Entity, SharedString, UniformListScrollHandle, WeakEntity};
use gpui_component::input::InputState;

use kagi_git::{Backend, ChangeKind, CommitPreview, FileDiffStat, FileStatus};

use crate::ui::file_tree::{self, TreeRow};
use crate::ui::smart_commit::SmartCommitState;
use crate::ui::KagiApp;

// ──────────────────────────────────────────────────────────────
// CommitPanelFileRef — which file is selected in the panel
// ──────────────────────────────────────────────────────────────

/// Identifies a selected file in the Commit Panel: which section (staged/unstaged)
/// and its index within that section.
#[derive(Clone, Debug, PartialEq)]
pub enum CommitPanelFileRef {
    /// File is in the Unstaged section (unstaged or untracked).
    Unstaged { index: usize },
    /// File is in the Staged section.
    Staged { index: usize },
}

// ──────────────────────────────────────────────────────────────
// CommitPlanModal — plan confirmation for commit
// ──────────────────────────────────────────────────────────────

/// State for an in-progress commit plan confirmation.
#[derive(Clone)]
pub struct CommitPlanModal {
    /// The computed plan (warnings for unstaged remains, preview_files = staged).
    pub plan: std::sync::Arc<kagi_git::ops::OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CommitPanelState — all mutable state for the commit panel
// ──────────────────────────────────────────────────────────────

/// All mutable state for the Commit Panel.
///
/// Stored in `KagiApp` and reset on `reload()`.
#[derive(Clone)]
pub struct CommitPanelState {
    /// Files in the unstaged section (modified + untracked, including conflicted).
    pub unstaged: Vec<FileStatus>,
    /// Files in the staged section.
    pub staged: Vec<FileStatus>,
    /// W16-DIFFSTAT: per-file additions/deletions for unstaged files (index→WT).
    pub unstaged_stats: Vec<FileDiffStat>,
    /// W16-DIFFSTAT: per-file additions/deletions for staged files (HEAD→index).
    pub staged_stats: Vec<FileDiffStat>,
    /// Paths of conflicted files (subset of unstaged — these cannot be staged).
    pub conflicted_paths: std::collections::HashSet<PathBuf>,
    /// Currently selected file (for row highlight in the panel).
    pub selected_file: Option<CommitPanelFileRef>,
    /// Commit message text (simple String; IME fallback — T014 pattern).
    pub commit_msg: String,
    /// When Some, the commit plan confirmation modal is shown.
    pub plan_modal: Option<CommitPlanModal>,
    /// Whether the file list is in tree view (true) or flat view (false).
    pub tree_view: bool,
    /// Cached staged-commit preview (count / A·M·D / target branch / author),
    /// recomputed in [`Self::reload_status`]. **Must not** be recomputed every
    /// render: `commit_preview()` runs a full `working_tree_status` (~150ms on a
    /// large repo), which at 60fps froze the panel to ~6fps (PERF bug).
    pub preview: Option<CommitPreview>,
    /// PERF: cached tree rows for the unstaged section, rebuilt in
    /// [`reload_status`] so the tree is NOT recomputed every frame.
    pub unstaged_tree: Vec<TreeRow>,
    /// PERF: cached tree rows for the staged section (see `unstaged_tree`).
    pub staged_tree: Vec<TreeRow>,
    /// PERF: O(1) lookup from unstaged file path → index into `unstaged_stats`.
    /// Replaces the per-row `find_stat` linear scan (was O(N²) per frame).
    pub unstaged_stat_index: std::collections::HashMap<PathBuf, usize>,
    /// PERF: O(1) lookup from staged file path → index into `staged_stats`.
    pub staged_stat_index: std::collections::HashMap<PathBuf, usize>,
}

impl CommitPanelState {
    /// Create a new CommitPanelState from the current repo status.
    pub fn from_repo(repo_path: &Path) -> Self {
        let mut state = CommitPanelState {
            unstaged: Vec::new(),
            staged: Vec::new(),
            unstaged_stats: Vec::new(),
            staged_stats: Vec::new(),
            conflicted_paths: std::collections::HashSet::new(),
            selected_file: None,
            commit_msg: String::new(),
            plan_modal: None,
            tree_view: false,
            preview: None,
            unstaged_tree: Vec::new(),
            staged_tree: Vec::new(),
            unstaged_stat_index: std::collections::HashMap::new(),
            staged_stat_index: std::collections::HashMap::new(),
        };
        state.reload_status(repo_path);
        state
    }

    /// Returns true if the given unstaged file path is conflicted (cannot be staged).
    pub fn is_conflicted(&self, path: &PathBuf) -> bool {
        self.conflicted_paths.contains(path)
    }

    /// Reload unstaged/staged lists from the repository.
    pub fn reload_status(&mut self, repo_path: &Path) {
        let backend = match Backend::open(repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("commit_panel: repo open error: {}", e);
                return;
            }
        };
        match backend.working_tree_status() {
            Ok(status) => {
                // Cache the staged-commit preview here (NOT per render frame),
                // reusing this `status` so we don't run a second
                // working_tree_status walk. Done before `status` is consumed below.
                self.preview = backend.commit_preview_from_status(&status).ok();
                // Track conflicted paths for UI (these cannot be staged).
                self.conflicted_paths = status.conflicted.iter().cloned().collect();

                // Whether there are tracked modifications (the only thing
                // unstaged_diffstat covers) — captured before `status` is moved.
                let has_tracked_modifications = !status.unstaged.is_empty();

                // Unstaged = modified + untracked combined
                let mut unstaged = status.unstaged;
                // Append untracked as Added entries
                for p in &status.untracked {
                    unstaged.push(FileStatus {
                        path: p.clone(),
                        change: ChangeKind::Added,
                    });
                }
                // Append conflicted as non-stageable entries (shown in unstaged section)
                for p in &status.conflicted {
                    unstaged.push(FileStatus {
                        path: p.clone(),
                        change: ChangeKind::Modified, // displayed with "C" badge via is_conflicted()
                    });
                }
                self.unstaged = unstaged;
                self.staged = status.staged;
                // W16-DIFFSTAT: aggregate additions/deletions for both sides.
                // Best-effort: on error leave the lists empty (bar omitted).
                // unstaged_diffstat covers tracked modifications only — skip the
                // (working-tree-walking) call entirely when there are none, so a
                // dir full of untracked files costs nothing here.
                self.unstaged_stats = if has_tracked_modifications {
                    backend.unstaged_diffstat().unwrap_or_default()
                } else {
                    Vec::new()
                };
                self.staged_stats = backend.staged_diffstat().unwrap_or_default();
                // Clear selection on status change.
                self.selected_file = None;
                // PERF: recompute the cached tree rows and diffstat indices once
                // per status change (NOT per frame).
                self.rebuild_derived();
            }
            Err(e) => {
                klog!("commit_panel: working_tree_status error: {}", e);
            }
        }
    }

    /// PERF: rebuild the cached tree rows and diffstat path→index maps from the
    /// current `unstaged`/`staged`/`*_stats` lists.  Called once per status
    /// change from [`reload_status`], so render is O(visible rows) not O(N²).
    fn rebuild_derived(&mut self) {
        self.unstaged_tree = file_tree::build_file_tree(&self.unstaged);
        self.staged_tree = file_tree::build_file_tree(&self.staged);

        self.unstaged_stat_index = self
            .unstaged_stats
            .iter()
            .enumerate()
            .map(|(i, s)| (s.path.clone(), i))
            .collect();
        self.staged_stat_index = self
            .staged_stats
            .iter()
            .enumerate()
            .map(|(i, s)| (s.path.clone(), i))
            .collect();
    }

    /// O(1) lookup of the unstaged [`FileDiffStat`] for `path`.
    pub fn unstaged_stat(&self, path: &PathBuf) -> Option<&FileDiffStat> {
        self.unstaged_stat_index
            .get(path)
            .and_then(|&i| self.unstaged_stats.get(i))
    }

    /// O(1) lookup of the staged [`FileDiffStat`] for `path`.
    pub fn staged_stat(&self, path: &PathBuf) -> Option<&FileDiffStat> {
        self.staged_stat_index
            .get(path)
            .and_then(|&i| self.staged_stats.get(i))
    }

    /// Return true if commit is possible (staged > 0 and message non-empty).
    /// NOTE: T026 moves can_commit logic to render_commit_panel which reads InputState.
    /// This method is kept for the headless path.
    #[allow(dead_code)]
    pub fn can_commit(&self) -> bool {
        !self.staged.is_empty() && !self.commit_msg.trim().is_empty()
    }
}

// ──────────────────────────────────────────────────────────────
// CommitPanelView — ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001
// ──────────────────────────────────────────────────────────────

/// ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the Commit Panel promoted to
/// its own `Entity<T>`, mirroring the `ConflictView` fat-entity template.
///
/// The entity OWNS the `CommitPanelState` view data **and** the nested input
/// entities (`commit_input` + the six `commit_template_inputs`), the template
/// mode flag, the queued smart-commit message, the per-branch draft autosave
/// state (`last_draft_value` / `draft_save_gen` — moved OFF the parent so the
/// parent render never reads the child's input each frame), a smart-commit
/// generation guard (`gen`), and the two file-list scroll handles. Self-rendering
/// child with its own `cx.notify()` scope: file select/highlight, the
/// tree↔flat toggle, and the plain↔template toggle re-render only this subtree.
///
/// # Re-entrancy invariant (CRITICAL — proven by `ConflictView`)
/// A `CommitPanelView` listener leases this entity. NO listener may synchronously
/// call a `KagiApp` method that reads/updates `app.commit_panel` (directly or via
/// `refresh_wip_diffstat`'s neighbours / `reload()` / `finish_merge_commit`).
/// Every such path DEFERS to the parent via `cx.spawn_in(window, …)` +
/// `weak_app.update_in(acx, …)`, by which time the listener has returned and the
/// lease is released. Pure entity-internal mutations stay synchronous + a child
/// `cx.notify()`.
///
/// Parent-owned (NOT moved here): `commit_panel_open` (visibility gate set by the
/// graph `select`), `conflict_merge_pending`, the shared `file_menu` overlay, and
/// the cross-cutting `smart_commit` / `smart_commit_detected_for` state (read by
/// the Settings overlay + command palette, so it stays on `KagiApp`).
pub struct CommitPanelView {
    /// The staging lists / stats / trees / preview / selection / plan-modal data.
    pub state: CommitPanelState,
    /// gpui-component `InputState` for the plain commit message (IME/focus).
    /// Created lazily in a `Window` context; kept STABLE across status reloads.
    pub commit_input: Option<Entity<InputState>>,
    /// `true` when authoring via the six structured template fields.
    pub commit_template_mode: bool,
    /// Lazily-created `InputState`s for `[type, scope, summary, body, test, risk]`.
    pub commit_template_inputs: Option<[Entity<InputState>; 6]>,
    /// A smart-commit message generated on a background thread, queued for the
    /// next render to push into the input (which needs `&mut Window`).
    pub pending_smart_msg: Option<String>,
    /// Last commit-message value mirrored to the per-branch draft file
    /// (T-COMMIT-007). Compared each render to detect edits cheaply.
    pub last_draft_value: String,
    /// Debounce generation for the draft autosave writer.
    pub draft_save_gen: u64,
    /// T-ENTITY-COMMITPANEL-001 (correction #5): smart-commit generation guard.
    /// Bumped on each generate; a stale background result whose captured `gen`
    /// no longer matches is dropped instead of clobbering a newer input.
    pub gen: u64,
    /// PERF: scroll handle for the Unstaged `uniform_list`.
    pub unstaged_scroll_handle: UniformListScrollHandle,
    /// PERF: scroll handle for the Staged `uniform_list`.
    pub staged_scroll_handle: UniformListScrollHandle,
    /// WIP-highlight target derived from the parent's open main diff
    /// (`Some((staged, path))` when a WIP file is open in the center diff). Pushed
    /// in by the parent render (`render_body`) — the entity must not read the
    /// parent's `main_diff` from its own render path (re-entrancy).
    pub active_wip: Option<(bool, PathBuf)>,
    /// Scaled panel width pushed in by the parent each frame (the divider drag
    /// lives on the parent, so the width is parent-owned; mirrored here so the
    /// self-rendering entity can size itself without a render arg).
    pub panel_render_width: f32,
    /// Snapshot of the parent's `smart_commit` state, pushed in by the parent
    /// each frame. `smart_commit` stays on `KagiApp` (shared with Settings /
    /// command palette); the entity only READS it to render the toolbar, so a
    /// clone snapshot avoids reading the parent from the entity's render path.
    pub smart_snapshot: SmartCommitState,
    /// Weak back-reference to the parent. Used ONLY from deferred listener
    /// closures — NEVER read in a `Render` path.
    pub(crate) app: WeakEntity<KagiApp>,
    /// Repo root for this panel session; constant for the entity's life.
    pub(crate) repo_path: PathBuf,
}

impl CommitPanelView {
    /// Construct the entity for a freshly-opened commit panel. Created in
    /// `KagiApp::open_commit_panel` via `cx.new`; the caller seeds the input
    /// value / draft immediately after.
    pub fn new(state: CommitPanelState, app: WeakEntity<KagiApp>, repo_path: PathBuf) -> Self {
        Self {
            state,
            commit_input: None,
            commit_template_mode: false,
            commit_template_inputs: None,
            pending_smart_msg: None,
            last_draft_value: String::new(),
            draft_save_gen: 0,
            gen: 0,
            unstaged_scroll_handle: UniformListScrollHandle::new(),
            staged_scroll_handle: UniformListScrollHandle::new(),
            active_wip: None,
            panel_render_width: 0.0,
            smart_snapshot: SmartCommitState::default(),
            app,
            repo_path,
        }
    }

    /// Compute the effective single-message text for the current mode: the
    /// assembled template (template mode) or the plain Input value (plain mode).
    /// Mirrors the former `KagiApp::effective_commit_message`.
    pub fn effective_commit_message(&self, cx: &gpui::App) -> String {
        if self.commit_template_mode {
            kagi_git::assemble(&self.template_fields_from_inputs(cx))
        } else {
            self.commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default()
        }
    }

    /// Read the six template `InputState`s into a [`kagi_git::TemplateFields`].
    pub fn template_fields_from_inputs(&self, cx: &gpui::App) -> kagi_git::TemplateFields {
        match &self.commit_template_inputs {
            Some([ty, scope, summary, body, test, risk]) => kagi_git::TemplateFields::new(
                ty.read(cx).value().to_string(),
                scope.read(cx).value().to_string(),
                summary.read(cx).value().to_string(),
                body.read(cx).value().to_string(),
                test.read(cx).value().to_string(),
                risk.read(cx).value().to_string(),
            ),
            None => kagi_git::TemplateFields::default(),
        }
    }

    /// Lazily create the six template-field `InputState`s (requires `&mut
    /// Window`). Order: `[type, scope, summary, body, test, risk]`. No-op once
    /// created. (Moved verbatim from `KagiApp::ensure_template_inputs`.)
    fn ensure_template_inputs(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        if self.commit_template_inputs.is_some() {
            return;
        }
        let ty = cx.new(|cx| InputState::new(window, cx).placeholder("type (feat, fix, …)"));
        let scope = cx.new(|cx| InputState::new(window, cx).placeholder("scope (optional)"));
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder("summary"));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(2, 8)
                .placeholder("body (optional)")
        });
        let test =
            cx.new(|cx| InputState::new(window, cx).placeholder("Test: how verified (optional)"));
        let risk =
            cx.new(|cx| InputState::new(window, cx).placeholder("Risk: known risks (optional)"));
        self.commit_template_inputs = Some([ty, scope, summary, body, test, risk]);
    }

    /// Write a [`kagi_git::TemplateFields`] into the six template `InputState`s.
    /// (Moved verbatim from `KagiApp::set_template_inputs`.)
    pub fn set_template_inputs(
        &mut self,
        fields: &kagi_git::TemplateFields,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.ensure_template_inputs(window, cx);
        if let Some([ty, scope, summary, body, test, risk]) = self.commit_template_inputs.clone() {
            ty.update(cx, |s, cx| s.set_value(fields.r#type.clone(), window, cx));
            scope.update(cx, |s, cx| s.set_value(fields.scope.clone(), window, cx));
            summary.update(cx, |s, cx| s.set_value(fields.summary.clone(), window, cx));
            body.update(cx, |s, cx| s.set_value(fields.body.clone(), window, cx));
            test.update(cx, |s, cx| s.set_value(fields.test.clone(), window, cx));
            risk.update(cx, |s, cx| s.set_value(fields.risk.clone(), window, cx));
        }
    }

    /// Toggle between plain and template authoring modes, carrying the content
    /// across so a toggle never loses the user's work (T-COMMIT-009).
    /// Entity-internal: operates only on this entity's own inputs / draft state.
    /// (Moved from `KagiApp::toggle_commit_template_mode`.)
    pub fn toggle_template_mode(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.commit_template_mode {
            // template → plain: assemble + pour into the plain Input.
            let fields = self.template_fields_from_inputs(cx);
            let assembled = kagi_git::assemble(&fields);
            if self.commit_input.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
                self.commit_input = Some(st);
            }
            if let Some(input) = self.commit_input.clone() {
                input.update(cx, |s, cx| s.set_value(assembled, window, cx));
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            self.commit_template_mode = false;
        } else {
            // plain → template: parse the plain Input into the fields.
            let plain = self
                .commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default();
            let fields = kagi_git::parse_message(&plain);
            self.set_template_inputs(&fields, window, cx);
            self.commit_template_mode = true;
            // Focus the summary field (index 2) — the most-edited one.
            if let Some(inputs) = self.commit_template_inputs.clone() {
                inputs[2].update(cx, |s, cx| s.focus(window, cx));
            }
        }
        // Persist the new mode immediately (with the current effective message).
        self.bump_draft_for_mode_change(cx);
        cx.notify();
    }

    /// Force a draft save on the next debounce tick after a mode change, so the
    /// `mode` field is persisted even if the message text is unchanged.
    /// (Moved from `KagiApp::bump_draft_for_mode_change`; the branch is read off
    /// the parent via the weak handle inside the debounced task.)
    fn bump_draft_for_mode_change(&mut self, cx: &mut gpui::Context<Self>) {
        let msg = self.effective_commit_message(cx);
        self.last_draft_value = msg;
        self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
        let gen = self.draft_save_gen;
        let mode = if self.commit_template_mode {
            "template"
        } else {
            "plain"
        }
        .to_string();
        let repo_path = self.repo_path.clone();
        let weak_app = self.app.clone();
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            // Read the current branch off the parent (it owns status_summary).
            let branch = weak_app
                .read_with(acx, |app, _| app.active_view.status_summary.branch.clone())
                .unwrap_or_default();
            let _ = this.update(acx, |view, _cx| {
                if view.draft_save_gen != gen {
                    return;
                }
                let msg = view.last_draft_value.clone();
                if msg.trim().is_empty() {
                    let _ = kagi_git::clear_draft(&repo_path, &branch);
                } else {
                    let _ = kagi_git::save_draft(&repo_path, &branch, &msg, &mode);
                }
            });
        })
        .detach();
    }

    /// Render-time input sync: push a queued smart-commit message into the Input
    /// (correction #2 — was the parent `render.rs:196` block), then run the
    /// per-branch draft autosave (correction #1 — was the parent
    /// `sync_modal_inputs` half). Runs on this entity's own render path with
    /// `&mut Window`, so the parent never reads the child's input each frame.
    pub fn sync_inputs(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        // ── Queued smart-commit message → Input (needs `&mut Window`). ──
        if let Some(msg) = self.pending_smart_msg.take() {
            if self.commit_template_mode {
                let fields = kagi_git::parse_message(&msg);
                self.set_template_inputs(&fields, window, cx);
            } else if let Some(input) = self.commit_input.clone() {
                input.update(cx, |state, cx| {
                    state.set_value(msg, window, cx);
                });
            }
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        let has_input = self.commit_input.is_some()
            || (self.commit_template_mode && self.commit_template_inputs.is_some());
        if has_input {
            let v = self.effective_commit_message(cx);
            if v != self.last_draft_value {
                self.last_draft_value = v;
                self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
                let gen = self.draft_save_gen;
                let mode = if self.commit_template_mode {
                    "template"
                } else {
                    "plain"
                }
                .to_string();
                let repo_path = self.repo_path.clone();
                let weak_app = self.app.clone();
                cx.spawn(async move |this, acx| {
                    acx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    let branch = weak_app
                        .read_with(acx, |app, _| app.active_view.status_summary.branch.clone())
                        .unwrap_or_default();
                    let _ = this.update(acx, |view, _cx| {
                        if view.draft_save_gen != gen {
                            return;
                        }
                        let msg = view.last_draft_value.clone();
                        if msg.trim().is_empty() {
                            let _ = kagi_git::clear_draft(&repo_path, &branch);
                        } else {
                            let _ = kagi_git::save_draft(&repo_path, &branch, &msg, &mode);
                            klog!("draft: saved {}", branch);
                        }
                    });
                })
                .detach();
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Status badge helpers for staging panel
// ──────────────────────────────────────────────────────────────

// Moved to kagi-ui-core::file_tree (ADR-0121 C4) so the Editor Workspace
// crate can share it; re-exported here so call sites are unchanged.
pub use kagi_ui_core::file_tree::status_badge;
