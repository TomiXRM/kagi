//! Commit Panel — T025
//!
//! GitKraken 風の作業台: staging / unstaging / diff / commit message / commit button。
//! `src/git/staging.rs` (T024) の 6 API のみを使う。
//!
//! ## headless 検証 env vars
//! - `KAGI_COMMIT_PANEL=1`       起動時に Commit Panel を開き件数をログ
//! - `KAGI_STAGE_FILE=<path>`    起動時に1ファイル stage
//! - `KAGI_UNSTAGE_FILE=<path>`  起動時に1ファイル unstage
//! - `KAGI_COMMIT_MSG=<msg>`     commit メッセージ設定 + KAGI_AUTO_CONFIRM=1 で実際に commit

use std::path::{Path, PathBuf};

use gpui::{prelude::*, Entity, SharedString, UniformListScrollHandle, WeakEntity};
use gpui_component::input::InputState;

use kagi_git::{Backend, ChangeKind, FileDiffStat, FileStatus};
use kagi_ui_core::i18n::Msg;

/// settings.json key: whether the body is seeded from `commit.template`.
/// Absent means yes — a configured template is opt-out, not opt-in.
const KEY_COMMIT_TEMPLATE: &str = "commit_template_enabled";

/// Whether a configured `commit.template` should seed the body on open.
pub fn template_enabled() -> bool {
    crate::ui::settings::read_setting(KEY_COMMIT_TEMPLATE).as_deref() != Some("0")
}

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
    /// issue #348: indices into `unstaged` of files classified generated
    /// (folded under "Generated (N)"). Built in [`reload_status`].
    pub unstaged_gen_files: Vec<usize>,
    /// issue #348: indices into `unstaged` of NON-generated files, in order —
    /// the flat-list row → file-index map when generated files are folded out.
    pub unstaged_normal_files: Vec<usize>,
    /// issue #348: generated / normal index lists for the staged section.
    pub staged_gen_files: Vec<usize>,
    /// issue #348: non-generated staged file indices (flat-list row map).
    pub staged_normal_files: Vec<usize>,
    /// issue #348: whether the "Generated (N)" sections are expanded. Default
    /// `false` (folding on) — generated files start collapsed.
    pub generated_expanded: bool,
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
            unstaged_tree: Vec::new(),
            staged_tree: Vec::new(),
            unstaged_stat_index: std::collections::HashMap::new(),
            staged_stat_index: std::collections::HashMap::new(),
            unstaged_gen_files: Vec::new(),
            unstaged_normal_files: Vec::new(),
            staged_gen_files: Vec::new(),
            staged_normal_files: Vec::new(),
            generated_expanded: false,
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
                // issue #348: classify each side's files as generated / lockfile
                // (reads blob heads + .gitattributes) so the panel can fold them.
                let unstaged_gen = backend.wip_generated_flags(&self.unstaged, false);
                let staged_gen = backend.wip_generated_flags(&self.staged, true);
                // Clear selection on status change.
                self.selected_file = None;
                // PERF: recompute the cached tree rows and diffstat indices once
                // per status change (NOT per frame).
                self.rebuild_derived(&unstaged_gen, &staged_gen);
            }
            Err(e) => {
                klog!("commit_panel: working_tree_status error: {}", e);
            }
        }
    }

    /// PERF: rebuild the cached tree rows and diffstat path→index maps from the
    /// current `unstaged`/`staged`/`*_stats` lists.  Called once per status
    /// change from [`reload_status`], so render is O(visible rows) not O(N²).
    fn rebuild_derived(&mut self, unstaged_gen: &[bool], staged_gen: &[bool]) {
        // issue #348: split each side into generated / normal index lists and
        // prune generated files (and now-empty dirs) from the tree rows so the
        // main list shows only normal files; generated files fold separately.
        let ug = kagi_domain::generated::group_generated(unstaged_gen);
        let sg = kagi_domain::generated::group_generated(staged_gen);
        self.unstaged_gen_files = ug.generated;
        self.unstaged_normal_files = ug.normal;
        self.staged_gen_files = sg.generated;
        self.staged_normal_files = sg.normal;

        let unstaged_full = file_tree::build_file_tree(&self.unstaged);
        let staged_full = file_tree::build_file_tree(&self.staged);
        self.unstaged_tree = file_tree::retain_files(unstaged_full, |i| {
            !unstaged_gen.get(i).copied().unwrap_or(false)
        });
        self.staged_tree = file_tree::retain_files(staged_full, |i| {
            !staged_gen.get(i).copied().unwrap_or(false)
        });

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

    /// issue #348: number of extra rows the "Generated (N)" fold adds to a
    /// section — 0 when nothing is generated, else 1 (the disclosure header)
    /// plus the folded file rows when expanded.
    pub fn generated_extra_rows(&self, staged: bool) -> usize {
        let gen = if staged {
            &self.staged_gen_files
        } else {
            &self.unstaged_gen_files
        };
        if gen.is_empty() {
            0
        } else if self.generated_expanded {
            1 + gen.len()
        } else {
            1
        }
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
}

// ──────────────────────────────────────────────────────────────
// CommitPanelView — ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001
// ──────────────────────────────────────────────────────────────

/// ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the Commit Panel promoted to
/// its own `Entity<T>`, mirroring the `ConflictView` fat-entity template.
///
/// The entity OWNS the `CommitPanelState` view data **and** the nested input
/// entities (`title_input` + `body_input`), the queued smart-commit message,
/// the per-branch draft autosave state (`last_draft_value` / `draft_save_gen` —
/// moved OFF the parent so the parent render never reads the child's input each
/// frame), a smart-commit generation guard (`gen`), the co-author picker, and
/// the two file-list scroll handles. Self-rendering child with its own
/// `cx.notify()` scope: file select/highlight, the tree↔flat toggle and the
/// co-author picker re-render only this subtree.
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
    /// The staging lists / stats / trees / selection / plan-modal data.
    pub state: CommitPanelState,
    /// gpui-component `InputState` for the commit subject line (IME/focus).
    /// Created lazily in a `Window` context; kept STABLE across status reloads.
    pub title_input: Option<Entity<InputState>>,
    /// Multi-line `InputState` for the commit body. Seeded from the user's
    /// `commit.template` on first open (ADR-0134) and the home of the
    /// `Co-authored-by:` trailers the co-author picker appends.
    pub body_input: Option<Entity<InputState>>,
    /// Co-author picker: `Some(candidates)` while the popover is open. Loaded
    /// on click rather than per-frame — it walks recent history.
    pub coauthor_menu: Option<Vec<kagi_git::AuthorCandidate>>,
    /// The user's `commit.template`, verbatim, or `None` when unset. Kept so the
    /// footer's Template toggle can put it back after it has been removed.
    pub commit_template: Option<String>,
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
    /// UI language the two message inputs were built with. `InputState` bakes
    /// its placeholder at construction and exposes no setter, so switching
    /// language rebuilds them (carrying the text across) rather than leaving a
    /// stale-language placeholder on screen.
    pub input_lang: kagi_ui_core::i18n::Lang,
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
            title_input: None,
            body_input: None,
            coauthor_menu: None,
            commit_template: None,
            pending_smart_msg: None,
            last_draft_value: String::new(),
            draft_save_gen: 0,
            gen: 0,
            input_lang: kagi_ui_core::i18n::lang(),
            unstaged_scroll_handle: UniformListScrollHandle::new(),
            staged_scroll_handle: UniformListScrollHandle::new(),
            active_wip: None,
            panel_render_width: 0.0,
            smart_snapshot: SmartCommitState::default(),
            app,
            repo_path,
        }
    }

    /// Whether a body line is a git trailer (`Key: value` with a hyphenated,
    /// space-free key) — used to keep appended co-authors in one block.
    fn is_trailer_line(line: &str) -> bool {
        line.split_once(": ")
            .is_some_and(|(k, v)| !k.contains(' ') && k.contains('-') && !v.trim().is_empty())
    }

    /// The effective commit message: subject, blank line, body (see
    /// [`kagi_git::join_title_body`]). The single string every consumer wants —
    /// the split into two inputs is a UI concern only.
    pub fn effective_commit_message(&self, cx: &gpui::App) -> String {
        let read = |i: &Option<Entity<InputState>>| {
            i.as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default()
        };
        kagi_git::join_title_body(&read(&self.title_input), &read(&self.body_input))
    }

    /// The message as it will actually be committed: [`Self::effective_commit_message`]
    /// with the body's comment lines removed.
    ///
    /// Templates are mostly comments, so the two differ whenever one is loaded:
    /// the raw form is what the user sees and what the draft stores, this is what
    /// reaches git. Commit, amend and "is there a message yet?" all use this one.
    pub fn committable_message(&self, cx: &gpui::App) -> String {
        let read = |i: &Option<Entity<InputState>>| {
            i.as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default()
        };
        let body = kagi_domain::message::strip_template_comments(&read(&self.body_input));
        kagi_git::join_title_body(&read(&self.title_input), &body)
    }

    /// Whether the body currently carries the template's comment block.
    ///
    /// Derived from the text rather than tracked in a flag, so the toggle can
    /// never disagree with what is actually in the box (the user can delete the
    /// comments by hand).
    pub fn template_active(&self, cx: &gpui::App) -> bool {
        self.body_input
            .as_ref()
            .map(|i| {
                i.read(cx)
                    .value()
                    .lines()
                    .any(kagi_domain::message::is_comment_line)
            })
            .unwrap_or(false)
    }

    /// Add or remove the `commit.template` block in the body, and remember the
    /// choice for the next open.
    pub fn toggle_template(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let Some(body) = self.body_input.clone() else {
            return;
        };
        let current = body.read(cx).value().to_string();
        let on = !self.template_active(cx);
        let next = if on {
            let Some(tpl) = self.commit_template.clone() else {
                return;
            };
            let written = kagi_domain::message::strip_template_comments(&current);
            if written.trim().is_empty() {
                tpl
            } else {
                format!("{}\n\n{}", written.trim_end(), tpl)
            }
        } else {
            kagi_domain::message::strip_template_comments(&current)
        };
        body.update(cx, |s, cx| s.set_value(next, window, cx));
        crate::ui::settings::write_setting(KEY_COMMIT_TEMPLATE, Some(if on { "1" } else { "0" }));
        klog!("commit template: {}", if on { "on" } else { "off" });
        cx.notify();
    }

    /// Open or close the co-author picker.
    ///
    /// Candidates are walked on open, not per frame — the walk touches recent
    /// history and the panel re-renders at 60fps. Entity-internal (no parent
    /// read), so it stays synchronous.
    pub fn toggle_coauthor_menu(&mut self, cx: &mut gpui::Context<Self>) {
        self.coauthor_menu = match self.coauthor_menu {
            Some(_) => None,
            None => Some(
                Backend::open(&self.repo_path)
                    .map(|b| b.recent_authors(20))
                    .unwrap_or_default(),
            ),
        };
        cx.notify();
    }

    /// Append a `Co-authored-by:` trailer to the body, skipping duplicates.
    /// Trailers live in the body text itself rather than in a side list, so the
    /// draft file, the plan modal and `parse_coauthors` all keep working with no
    /// extra plumbing.
    pub fn add_coauthor(
        &mut self,
        candidate: &kagi_git::AuthorCandidate,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(body) = self.body_input.clone() else {
            return;
        };
        let trailer = candidate.trailer();
        let current = body.read(cx).value().to_string();
        if current
            .lines()
            .any(|l| l.trim().eq_ignore_ascii_case(&trailer))
        {
            return;
        }
        // Trailers are their own block at the end, separated by a blank line.
        let next = if current.trim().is_empty() {
            trailer
        } else if current.lines().last().is_some_and(Self::is_trailer_line) {
            format!("{}\n{}", current.trim_end(), trailer)
        } else {
            format!("{}\n\n{}", current.trim_end(), trailer)
        };
        body.update(cx, |s, cx| s.set_value(next, window, cx));
        cx.notify();
    }

    /// Render-time input sync: push a queued smart-commit message into the Input
    /// (correction #2 — was the parent `render.rs:196` block), then run the
    /// per-branch draft autosave (correction #1 — was the parent
    /// `sync_modal_inputs` half). Runs on this entity's own render path with
    /// `&mut Window`, so the parent never reads the child's input each frame.
    pub fn sync_inputs(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        // ── Language switch → rebuild the inputs for their placeholders. ──
        // `InputState` has no placeholder setter, so the text is carried across
        // a fresh pair. Rare (a menu action), and it only costs the caret.
        let lang = kagi_ui_core::i18n::lang();
        if lang != self.input_lang {
            self.input_lang = lang;
            if let Some(old) = self.title_input.clone() {
                let text = old.read(cx).value().to_string();
                let fresh =
                    cx.new(|cx| InputState::new(window, cx).placeholder(Msg::CommitTitle.t()));
                fresh.update(cx, |s, cx| s.set_value(text, window, cx));
                self.title_input = Some(fresh);
            }
            if let Some(old) = self.body_input.clone() {
                let text = old.read(cx).value().to_string();
                let fresh = cx.new(|cx| {
                    InputState::new(window, cx)
                        .multi_line(true)
                        .auto_grow(3, 12)
                        .placeholder(Msg::CommitBody.t())
                });
                fresh.update(cx, |s, cx| s.set_value(text, window, cx));
                self.body_input = Some(fresh);
            }
        }

        // ── Queued smart-commit message → Input (needs `&mut Window`). ──
        if let Some(msg) = self.pending_smart_msg.take() {
            let (title, body) = kagi_git::split_title_body(&msg);
            if let Some(input) = self.title_input.clone() {
                input.update(cx, |state, cx| state.set_value(title, window, cx));
            }
            if let Some(input) = self.body_input.clone() {
                input.update(cx, |state, cx| state.set_value(body, window, cx));
            }
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        if self.title_input.is_some() || self.body_input.is_some() {
            let v = self.effective_commit_message(cx);
            if v != self.last_draft_value {
                self.last_draft_value = v;
                self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
                let gen = self.draft_save_gen;
                let mode = "plain".to_string();
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

#[cfg(test)]
mod generated_fold_tests {
    use super::*;

    /// Build an empty state (no repo) so `rebuild_derived` can be exercised
    /// in isolation.
    fn empty_state() -> CommitPanelState {
        CommitPanelState {
            unstaged: Vec::new(),
            staged: Vec::new(),
            unstaged_stats: Vec::new(),
            staged_stats: Vec::new(),
            conflicted_paths: std::collections::HashSet::new(),
            selected_file: None,
            commit_msg: String::new(),
            plan_modal: None,
            tree_view: false,
            unstaged_tree: Vec::new(),
            staged_tree: Vec::new(),
            unstaged_stat_index: std::collections::HashMap::new(),
            staged_stat_index: std::collections::HashMap::new(),
            unstaged_gen_files: Vec::new(),
            unstaged_normal_files: Vec::new(),
            staged_gen_files: Vec::new(),
            staged_normal_files: Vec::new(),
            generated_expanded: false,
        }
    }

    fn modified(path: &str) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            change: ChangeKind::Modified,
        }
    }

    /// issue #348: rebuild splits generated files out of the flat/tree lists and
    /// the fold defaults collapsed (only the header row shows until expanded).
    #[test]
    fn rebuild_folds_generated_files() {
        let mut s = empty_state();
        // Matches the reported bug: Cargo.lock + main.rs unstaged.
        s.unstaged = vec![modified("Cargo.lock"), modified("src/main.rs")];
        // Cargo.lock (index 0) is generated; main.rs (index 1) is not.
        s.rebuild_derived(&[true, false], &[]);

        assert_eq!(s.unstaged_gen_files, vec![0]);
        assert_eq!(s.unstaged_normal_files, vec![1]);
        // Pruned tree (flat has 1 dir-less file 'main.rs'): no Cargo.lock row.
        assert!(s.unstaged_tree.iter().all(|r| !matches!(
            r,
            kagi_ui_core::file_tree::TreeRow::File { file_index: 0, .. }
        )));

        // Collapsed by default → 1 extra row (the header only).
        assert!(!s.generated_expanded);
        assert_eq!(s.generated_extra_rows(false), 1);
        // Expanded → header + the 1 generated file.
        s.generated_expanded = true;
        assert_eq!(s.generated_extra_rows(false), 2);
    }

    /// No generated files → no fold, no extra rows.
    #[test]
    fn no_generated_no_fold() {
        let mut s = empty_state();
        s.unstaged = vec![modified("src/a.rs"), modified("src/b.rs")];
        s.rebuild_derived(&[false, false], &[]);
        assert!(s.unstaged_gen_files.is_empty());
        assert_eq!(s.unstaged_normal_files, vec![0, 1]);
        assert_eq!(s.generated_extra_rows(false), 0);
    }
}
